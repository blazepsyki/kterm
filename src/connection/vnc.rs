// SPDX-License-Identifier: MIT OR Apache-2.0

use std::time::Duration;

use iced::futures::{self, StreamExt};
use log::{debug, info, warn};
use tokio::sync::mpsc;
use tokio::time::MissedTickBehavior;
use vnc::{
    ClientKeyEvent, ClientMouseEvent, PixelFormat, VncClient, VncConnector, VncEncoding,
    VncError, VncEvent, X11Event,
};

use super::{
    ConnectionEvent, ConnectionInput, KeyboardIndicators, RemoteInput, RemoteMouseButton,
};
use crate::remote_display::FrameUpdate;

const VNC_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const VNC_REFRESH_INTERVAL: Duration = Duration::from_millis(16);
const VNC_AUTH_PASSWORD_LIMIT: usize = 8;
const VNC_CONSERVATIVE_FULL_UPLOAD: bool = false;
const VNC_HEAL_FULL_REFRESH_INTERVAL: Duration = Duration::from_secs(2);
const VNC_RECT_ONLY_STREAK_FORCE_THRESHOLD: u32 = 6;
const VNC_RECT_BATCH_FORCE_THRESHOLD: usize = 64;

#[derive(Debug, Default)]
struct VncFramebuffer {
    width: u16,
    height: u16,
    rgba: Vec<u8>,
}

impl VncFramebuffer {
    fn reset(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.rgba = vec![0; width as usize * height as usize * 4];
    }

    fn as_full_update(&self) -> Option<FrameUpdate> {
        if self.width == 0 || self.height == 0 {
            return None;
        }

        Some(FrameUpdate::Full {
            width: self.width,
            height: self.height,
            rgba: self.rgba.clone(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CursorRect {
    x: u16,
    y: u16,
    width: u16,
    height: u16,
}

#[derive(Debug, Default)]
struct VncCursorState {
    hot_x: u16,
    hot_y: u16,
    width: u16,
    height: u16,
    rgba: Vec<u8>,
    last_rect: Option<CursorRect>,
    needs_cursor_sync_full_refresh: bool,
}

impl VncCursorState {
    fn has_shape(&self) -> bool {
        self.width > 0
            && self.height > 0
            && self.rgba.len() >= self.width as usize * self.height as usize * 4
    }
}

pub fn connect_and_subscribe(
    host: String,
    port: u16,
    password: Option<String>,
) -> futures::stream::BoxStream<'static, ConnectionEvent> {
    let (tx_to_vnc, rx_from_iced) = mpsc::unbounded_channel::<ConnectionInput>();
    let (tx_from_worker, rx_from_worker) = mpsc::unbounded_channel::<ConnectionEvent>();

    tokio::spawn(async move {
        run_vnc_worker(host, port, password, rx_from_iced, tx_from_worker, tx_to_vnc).await;
    });

    // Merge consecutive frame batches to reduce UI handle churn.
    struct VncStream {
        rx: mpsc::UnboundedReceiver<ConnectionEvent>,
        pending: Option<ConnectionEvent>,
    }

    futures::stream::unfold(
        VncStream {
            rx: rx_from_worker,
            pending: None,
        },
        |mut s| async move {
            if let Some(ev) = s.pending.take() {
                return Some((ev, s));
            }

            let first = s.rx.recv().await?;

            let ConnectionEvent::Frames(mut merged) = first else {
                return Some((first, s));
            };

            loop {
                match s.rx.try_recv() {
                    Ok(ConnectionEvent::Frames(more)) => merged.extend(more),
                    Ok(other) => {
                        s.pending = Some(other);
                        break;
                    }
                    Err(_) => break,
                }
            }

            Some((ConnectionEvent::Frames(merged), s))
        },
    )
    .boxed()
}

async fn run_vnc_worker(
    host: String,
    port: u16,
    password: Option<String>,
    rx_from_iced: mpsc::UnboundedReceiver<ConnectionInput>,
    tx_from_worker: mpsc::UnboundedSender<ConnectionEvent>,
    tx_to_vnc: mpsc::UnboundedSender<ConnectionInput>,
) {
    let tx_err = tx_from_worker.clone();
    if let Err(err) = run_vnc_worker_inner(host, port, password, rx_from_iced, tx_from_worker, tx_to_vnc).await {
        let _ = tx_err.send(ConnectionEvent::Error(err));
    }
}

async fn run_vnc_worker_inner(
    host: String,
    port: u16,
    password: Option<String>,
    mut rx_from_iced: mpsc::UnboundedReceiver<ConnectionInput>,
    tx_from_worker: mpsc::UnboundedSender<ConnectionEvent>,
    tx_to_vnc: mpsc::UnboundedSender<ConnectionInput>,
) -> Result<(), String> {
    info!("[VNC] connecting to {}:{}", host, port);

    let tcp = tokio::time::timeout(
        VNC_CONNECT_TIMEOUT,
        tokio::net::TcpStream::connect((host.as_str(), port)),
    )
    .await
    .map_err(|_| {
        format!(
            "VNC TCP connect timed out after {}s",
            VNC_CONNECT_TIMEOUT.as_secs()
        )
    })?
    .map_err(|e| format!("VNC TCP connect failed: {}", e))?;

    let auth_password = password.unwrap_or_default();
    if auth_password.chars().count() > VNC_AUTH_PASSWORD_LIMIT {
        let _ = tx_from_worker.send(ConnectionEvent::Data(
            format!(
                "\r\n[VNC] Warning: passwords longer than {} chars may fail on VNCAuth servers.\r\n",
                VNC_AUTH_PASSWORD_LIMIT
            )
            .into_bytes(),
        ));
    }

    let vnc = VncConnector::new(tcp)
        .set_auth_method(async move { Ok::<String, VncError>(auth_password) })
        .set_pixel_format(PixelFormat::rgba())
        .allow_shared(true)
        .add_encoding(VncEncoding::CopyRect)
        .add_encoding(VncEncoding::Raw)
        .add_encoding(VncEncoding::DesktopSizePseudo)
        .add_encoding(VncEncoding::CursorPseudo)
        .build()
        .map_err(|e| format!("VNC connector build failed: {}", e))?
        .try_start()
        .await
        .map_err(|e| format!("VNC handshake/auth failed: {}", e))?
        .finish()
        .map_err(|e| format!("VNC client finalize failed: {}", e))?;

    let _ = tx_from_worker.send(ConnectionEvent::Connected(tx_to_vnc));

    let summary = format!(
        "\r\n[VNC] Connected: {}:{} (encodings: CopyRect, Raw, DesktopSize, CursorPseudo)\r\n",
        host, port
    );
    let _ = tx_from_worker.send(ConnectionEvent::Data(summary.into_bytes()));

    // Force the first full framebuffer so UI starts from a known consistent image.
    vnc.input(X11Event::FullRefresh)
        .await
        .map_err(|e| format!("VNC initial full refresh failed: {}", e))?;

    let mut pointer = PointerState::default();
    let mut framebuffer = VncFramebuffer::default();
    let mut cursor = VncCursorState::default();
    let mut remote_lock_state: Option<KeyboardIndicators> = None;
    let mut key_state = VncKeyState::default();
    let mut refresh = tokio::time::interval(VNC_REFRESH_INTERVAL);
    let mut last_full_refresh = tokio::time::Instant::now();
    let mut rect_only_streak = 0u32;
    refresh.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            input = rx_from_iced.recv() => {
                match input {
                    Some(input) => {
                        handle_connection_input(
                            &vnc,
                            &tx_from_worker,
                            &framebuffer,
                            &mut cursor,
                            &mut pointer,
                            &mut remote_lock_state,
                            &mut key_state,
                            input,
                        )
                        .await?
                    }
                    None => {
                        info!("[VNC] input channel closed; closing session");
                        break;
                    }
                }
            }
            _ = refresh.tick() => {
                let use_full_refresh = last_full_refresh.elapsed() >= VNC_HEAL_FULL_REFRESH_INTERVAL;
                let req = if use_full_refresh {
                    X11Event::FullRefresh
                } else {
                    X11Event::Refresh
                };

                vnc.input(req)
                    .await
                    .map_err(|e| format!("VNC refresh request failed: {}", e))?;

                if use_full_refresh {
                    last_full_refresh = tokio::time::Instant::now();
                }

                let mut pending_updates = Vec::new();
                let mut request_sync_refresh = false;

                loop {
                    match vnc.poll_event().await {
                        Ok(Some(event)) => {
                            let effect = handle_vnc_event(
                                &tx_from_worker,
                                &mut framebuffer,
                                &mut cursor,
                                &pointer,
                                event,
                            )
                            .await?;
                            pending_updates.extend(effect.updates);
                            request_sync_refresh |= effect.request_sync_refresh;
                        }
                        Ok(None) => break,
                        Err(e) => {
                            return Err(format!("VNC event polling failed: {}", e));
                        }
                    }
                }

                maybe_promote_vnc_updates_to_full(
                    &mut pending_updates,
                    &framebuffer,
                    &mut cursor,
                    &pointer,
                    &mut rect_only_streak,
                );

                if !pending_updates.is_empty() {
                    let _ = tx_from_worker.send(ConnectionEvent::Frames(pending_updates));
                }

                if request_sync_refresh {
                    vnc.input(X11Event::FullRefresh)
                        .await
                        .map_err(|e| format!("VNC cursor sync full refresh failed: {}", e))?;
                    last_full_refresh = tokio::time::Instant::now();
                }
            }
        }
    }

    let _ = vnc.close().await;
    let _ = tx_from_worker.send(ConnectionEvent::Disconnected);
    Ok(())
}

#[derive(Debug, Default)]
struct PointerState {
    x: u16,
    y: u16,
    buttons: u8,
}

#[derive(Debug, Default)]
struct VncKeyState {
    shift_left: bool,
    shift_right: bool,
    caps_lock: bool,
    num_lock: bool,
    scroll_lock: bool,
}

impl VncKeyState {
    fn shift_active(&self) -> bool {
        self.shift_left || self.shift_right
    }

    fn update_from_scancode(&mut self, code: u8, extended: bool, down: bool) {
        match (code, extended) {
            (0x2A, false) => self.shift_left = down,
            (0x36, false) => self.shift_right = down,
            (0x3A, _) if down => self.caps_lock = !self.caps_lock,
            (0x45, false) if down => self.num_lock = !self.num_lock,
            (0x46, _) if down => self.scroll_lock = !self.scroll_lock,
            _ => {}
        }
    }
}

#[derive(Debug, Default)]
struct VncEventEffect {
    updates: Vec<FrameUpdate>,
    request_sync_refresh: bool,
}

async fn handle_vnc_event(
    tx_from_worker: &mpsc::UnboundedSender<ConnectionEvent>,
    framebuffer: &mut VncFramebuffer,
    cursor: &mut VncCursorState,
    pointer: &PointerState,
    event: VncEvent,
) -> Result<VncEventEffect, String> {
    match event {
        VncEvent::SetResolution(screen) => {
            framebuffer.reset(screen.width, screen.height);
            cursor.last_rect = None;
            cursor.needs_cursor_sync_full_refresh = true;

            let rgba = vec![0; screen.width as usize * screen.height as usize * 4];
            debug!("[VNC] resolution {}x{}", screen.width, screen.height);
            return Ok(VncEventEffect {
                updates: vec![FrameUpdate::Full {
                    width: screen.width,
                    height: screen.height,
                    rgba,
                }],
                request_sync_refresh: false,
            });
        }
        VncEvent::RawImage(rect, data) => {
            let expected = rect.width as usize * rect.height as usize * 4;
            if data.len() < expected {
                warn!(
                    "[VNC] raw image too small: got {} bytes, expected {}",
                    data.len(),
                    expected
                );
                return Ok(VncEventEffect::default());
            }

            let rgba = if data.len() == expected {
                data
            } else {
                data[..expected].to_vec()
            };

            write_raw_rect_to_framebuffer(framebuffer, rect.x, rect.y, rect.width, rect.height, &rgba);

            let mut updates = if VNC_CONSERVATIVE_FULL_UPLOAD {
                framebuffer
                    .as_full_update()
                    .map(|f| vec![f])
                    .unwrap_or_else(Vec::new)
            } else {
                vec![FrameUpdate::Rect {
                    x: rect.x,
                    y: rect.y,
                    width: rect.width,
                    height: rect.height,
                    rgba,
                }]
            };

            if let Some(overlay) = draw_cursor_overlay_update(framebuffer, cursor, pointer) {
                updates.push(overlay);
            }

            return Ok(VncEventEffect {
                updates,
                request_sync_refresh: false,
            });
        }
        VncEvent::SetPixelFormat(format) => {
            info!(
                "[VNC] pixel format: bpp={} depth={} shifts=({},{},{})",
                format.bits_per_pixel,
                format.depth,
                format.red_shift,
                format.green_shift,
                format.blue_shift
            );
            return Ok(VncEventEffect::default());
        }
        VncEvent::Text(text) => {
            let _ = tx_from_worker.send(ConnectionEvent::Data(
                format!("\r\n[VNC] Clipboard text from server: {}\r\n", text).into_bytes(),
            ));
            return Ok(VncEventEffect::default());
        }
        VncEvent::Bell => {
            info!("[VNC] bell");
            return Ok(VncEventEffect::default());
        }
        VncEvent::SetCursor(rect, image) => {
            cursor.hot_x = rect.x;
            cursor.hot_y = rect.y;
            cursor.width = rect.width;
            cursor.height = rect.height;
            cursor.rgba = image;

            let request_sync_refresh = cursor.needs_cursor_sync_full_refresh;
            cursor.needs_cursor_sync_full_refresh = false;

            let previous = cursor.last_rect;
            let next = compute_cursor_rect(framebuffer, cursor, pointer);

            let mut updates = Vec::new();
            if previous != next {
                if let Some(old) = previous {
                    if let Some(restore) = framebuffer_rect_update(framebuffer, old) {
                        updates.push(restore);
                    }
                }
            }

            if let Some(overlay) = draw_cursor_overlay_update(framebuffer, cursor, pointer) {
                updates.push(overlay);
            }

            return Ok(VncEventEffect {
                updates,
                request_sync_refresh,
            });
        }
        VncEvent::Copy(dst, src) => {
            if let Some(copied) = copy_rect_in_framebuffer(
                framebuffer,
                dst.x,
                dst.y,
                src.x,
                src.y,
                dst.width.min(src.width),
                dst.height.min(src.height),
            ) {
                let mut updates = if VNC_CONSERVATIVE_FULL_UPLOAD {
                    framebuffer
                        .as_full_update()
                        .map(|f| vec![f])
                        .unwrap_or_else(Vec::new)
                } else {
                    vec![copied]
                };
                if let Some(overlay) = draw_cursor_overlay_update(framebuffer, cursor, pointer) {
                    updates.push(overlay);
                }
                return Ok(VncEventEffect {
                    updates,
                    request_sync_refresh: false,
                });
            }
            return Ok(VncEventEffect::default());
        }
        VncEvent::JpegImage(_, _) => {
            // Tight/JPEG is not negotiated in the MVP stage.
            return Ok(VncEventEffect::default());
        }
        VncEvent::Error(msg) => {
            if msg.to_ascii_lowercase().contains("password") {
                return Err(format!("VNC authentication failed: {}", msg));
            }
            if msg.to_ascii_lowercase().contains("security") {
                return Err(format!("VNC security negotiation failed: {}", msg));
            }
            return Err(format!("VNC engine error: {}", msg));
        }
        _ => {
            // vnc-rs marks VncEvent as non-exhaustive; ignore future events safely.
            return Ok(VncEventEffect::default());
        }
    }
}

async fn handle_connection_input(
    vnc: &VncClient,
    tx_from_worker: &mpsc::UnboundedSender<ConnectionEvent>,
    framebuffer: &VncFramebuffer,
    cursor: &mut VncCursorState,
    pointer: &mut PointerState,
    remote_lock_state: &mut Option<KeyboardIndicators>,
    key_state: &mut VncKeyState,
    input: ConnectionInput,
) -> Result<(), String> {
    match input {
        ConnectionInput::RemoteInput(remote) => {
            handle_remote_input(vnc, tx_from_worker, framebuffer, cursor, pointer, key_state, remote)
                .await
        }
        ConnectionInput::Data(bytes) => {
            // Paste path: send UTF-8 text as keysyms.
            let text = String::from_utf8_lossy(&bytes);
            for ch in text.chars() {
                if ch == '\0' {
                    continue;
                }
                let code = ch as u32;
                send_key(vnc, code, true).await?;
                send_key(vnc, code, false).await?;
            }
            Ok(())
        }
        ConnectionInput::Resize { cols, rows } => {
            debug!("[VNC] resize hint received: {}x{} (requesting full refresh)", cols, rows);
            vnc.input(X11Event::FullRefresh)
                .await
                .map_err(|e| format!("VNC full refresh on resize failed: {}", e))
        }
        ConnectionInput::SyncKeyboardIndicators(indicators) => {
            info!(
                "[VNC][LOCK] sync request local num={} caps={} scroll={}",
                indicators.num_lock, indicators.caps_lock, indicators.scroll_lock
            );
            key_state.num_lock = indicators.num_lock;
            key_state.caps_lock = indicators.caps_lock;
            key_state.scroll_lock = indicators.scroll_lock;
            sync_keyboard_indicators(vnc, remote_lock_state, indicators).await
        }
        ConnectionInput::ReleaseAllModifiers => {
            for key in [0xffe1_u32, 0xffe2, 0xffe3, 0xffe4, 0xffe9, 0xffea, 0xffeb, 0xffec] {
                send_key(vnc, key, false).await?;
            }
            Ok(())
        }
    }
}

async fn handle_remote_input(
    vnc: &VncClient,
    tx_from_worker: &mpsc::UnboundedSender<ConnectionEvent>,
    framebuffer: &VncFramebuffer,
    cursor: &mut VncCursorState,
    pointer: &mut PointerState,
    key_state: &mut VncKeyState,
    remote: RemoteInput,
) -> Result<(), String> {
    match remote {
        RemoteInput::KeyboardScancode {
            code,
            extended,
            down,
        } => {
            key_state.update_from_scancode(code, extended, down);

            if is_lock_scancode(code, extended) {
                info!(
                    "[VNC][LOCK] scancode input code=0x{:02X} extended={} down={}",
                    code, extended, down
                );
            } else if is_modifier_scancode(code, extended) {
                debug!(
                    "[VNC][MOD] scancode input code=0x{:02X} extended={} down={}",
                    code, extended, down
                );
            }

            if let Some(keysym) = keysym_from_scancode_with_state(code, extended, key_state) {
                if is_lock_scancode(code, extended) {
                    info!(
                        "[VNC][LOCK] mapped scancode code=0x{:02X} extended={} -> keysym=0x{:X}",
                        code, extended, keysym
                    );
                }
                send_key(vnc, keysym, down).await?;
            } else {
                debug!("[VNC] unmapped scancode code={} extended={}", code, extended);
            }
            Ok(())
        }
        RemoteInput::KeyboardUnicode { codepoint, down } => {
            send_key(vnc, codepoint as u32, down).await
        }
        RemoteInput::MouseMove { x, y } => {
            pointer.x = x;
            pointer.y = y;
            send_pointer(vnc, pointer.x, pointer.y, pointer.buttons).await?;

            let previous = cursor.last_rect;
            let next = compute_cursor_rect(framebuffer, cursor, pointer);
            if previous == next {
                return Ok(());
            }

            let mut updates = Vec::new();
            if let Some(old) = previous {
                if let Some(restore) = framebuffer_rect_update(framebuffer, old) {
                    updates.push(restore);
                }
            }
            if let Some(overlay) = draw_cursor_overlay_update(framebuffer, cursor, pointer) {
                updates.push(overlay);
            }
            if !updates.is_empty() {
                let _ = tx_from_worker.send(ConnectionEvent::Frames(updates));
            }
            Ok(())
        }
        RemoteInput::MouseButton { button, down } => {
            let mask = match button {
                RemoteMouseButton::Left => 0b001,
                RemoteMouseButton::Middle => 0b010,
                RemoteMouseButton::Right => 0b100,
            };
            if down {
                pointer.buttons |= mask;
            } else {
                pointer.buttons &= !mask;
            }
            send_pointer(vnc, pointer.x, pointer.y, pointer.buttons).await
        }
        RemoteInput::MouseWheel { delta } => {
            let wheel_bit = if delta > 0 { 0b1000 } else { 0b1_0000 };
            for _ in 0..wheel_steps(delta) {
                send_pointer(vnc, pointer.x, pointer.y, pointer.buttons | wheel_bit).await?;
                send_pointer(vnc, pointer.x, pointer.y, pointer.buttons).await?;
            }
            Ok(())
        }
        RemoteInput::MouseHorizontalWheel { delta } => {
            // Buttons 6/7 are commonly used for horizontal scrolling in VNC servers.
            let wheel_bit = if delta > 0 { 0b10_0000 } else { 0b100_0000 };
            for _ in 0..wheel_steps(delta) {
                send_pointer(vnc, pointer.x, pointer.y, pointer.buttons | wheel_bit).await?;
                send_pointer(vnc, pointer.x, pointer.y, pointer.buttons).await?;
            }
            Ok(())
        }
    }
}

fn wheel_steps(delta: i16) -> usize {
    let abs = i32::from(delta).unsigned_abs();
    let steps = abs.div_ceil(120);
    usize::try_from(steps.clamp(1, 8)).unwrap_or(1)
}

fn maybe_promote_vnc_updates_to_full(
    updates: &mut Vec<FrameUpdate>,
    framebuffer: &VncFramebuffer,
    cursor: &mut VncCursorState,
    pointer: &PointerState,
    rect_only_streak: &mut u32,
) {
    let full_count = updates
        .iter()
        .filter(|update| matches!(update, FrameUpdate::Full { .. }))
        .count();
    let rect_count = updates
        .iter()
        .filter(|update| matches!(update, FrameUpdate::Rect { .. }))
        .count();

    if full_count > 0 {
        *rect_only_streak = 0;
        return;
    }

    if rect_count == 0 {
        return;
    }

    *rect_only_streak = rect_only_streak.saturating_add(1);

    let reason = if rect_count >= VNC_RECT_BATCH_FORCE_THRESHOLD {
        Some("vnc_rect_batch")
    } else if *rect_only_streak >= VNC_RECT_ONLY_STREAK_FORCE_THRESHOLD {
        Some("vnc_rect_only_streak")
    } else {
        None
    };

    let Some(reason) = reason else {
        return;
    };

    let Some(full_update) = framebuffer.as_full_update() else {
        return;
    };

    let mut promoted_updates = vec![full_update];
    if let Some(overlay) = draw_cursor_overlay_update(framebuffer, cursor, pointer) {
        promoted_updates.push(overlay);
    }

    *updates = promoted_updates;
    *rect_only_streak = 0;

    if crate::rdp_trace_enabled() {
        info!(
            "[VNC] promote_frame_batch_to_full reason={} rects={}",
            reason,
            rect_count,
        );
    }
}

async fn send_key(vnc: &VncClient, keycode: u32, down: bool) -> Result<(), String> {
    vnc.input(X11Event::KeyEvent(ClientKeyEvent { keycode, down }))
        .await
        .map_err(|e| format!("VNC key input failed: {}", e))
}

async fn send_pointer(vnc: &VncClient, x: u16, y: u16, buttons: u8) -> Result<(), String> {
    vnc.input(X11Event::PointerEvent(ClientMouseEvent {
        position_x: x,
        position_y: y,
        bottons: buttons,
    }))
    .await
    .map_err(|e| format!("VNC pointer input failed: {}", e))
}

fn keysym_from_scancode_with_state(
    code: u8,
    extended: bool,
    key_state: &VncKeyState,
) -> Option<u32> {
    if !extended {
        if let Some(letter) = alpha_from_scancode(code) {
            let upper = key_state.caps_lock ^ key_state.shift_active();
            return Some(if upper {
                letter.to_ascii_uppercase() as u32
            } else {
                letter as u32
            });
        }

        if let Some((normal, shifted)) = printable_symbol_from_scancode(code) {
            return Some(if key_state.shift_active() {
                shifted as u32
            } else {
                normal as u32
            });
        }

        if let Some(keysym) = numpad_keysym_from_scancode(code, key_state.num_lock) {
            return Some(keysym);
        }
    }

    let keysym = match (code, extended) {
        (0x01, _) => 0xff1b, // Escape
        (0x0e, _) => 0xff08, // BackSpace
        (0x0f, _) => 0xff09, // Tab
        (0x1a, _) => '[' as u32,
        (0x1b, _) => ']' as u32,
        (0x1c, false) => 0xff0d, // Return
        (0x1c, true) => 0xff8d,  // KP_Enter
        (0x1d, false) => 0xffe3, // Control_L
        (0x1d, true) => 0xffe4,  // Control_R
        (0x27, _) => ';' as u32,
        (0x28, _) => '\'' as u32,
        (0x29, _) => '`' as u32,
        (0x2a, _) => 0xffe1, // Shift_L
        (0x2b, _) => '\\' as u32,
        (0x33, _) => ',' as u32,
        (0x34, _) => '.' as u32,
        (0x35, false) => '/' as u32,
        (0x35, true) => 0xffaf, // KP_Divide
        (0x36, _) => 0xffe2,    // Shift_R
        (0x37, false) => 0xffaa, // KP_Multiply
        (0x38, false) => 0xffe9, // Alt_L
        (0x38, true) => 0xffea,  // Alt_R
        (0x39, _) => 0x20,       // Space
        (0x3a, _) => 0xffe5,     // Caps_Lock
        (0x3b, _) => 0xffbe,     // F1
        (0x3c, _) => 0xffbf,     // F2
        (0x3d, _) => 0xffc0,     // F3
        (0x3e, _) => 0xffc1,     // F4
        (0x3f, _) => 0xffc2,     // F5
        (0x40, _) => 0xffc3,     // F6
        (0x41, _) => 0xffc4,     // F7
        (0x42, _) => 0xffc5,     // F8
        (0x43, _) => 0xffc6,     // F9
        (0x44, _) => 0xffc7,     // F10
        (0x45, false) => 0xff7f, // Num_Lock
        (0x46, _) => 0xff14,     // Scroll_Lock
        (0x37, true) => 0xff61,  // Print
        (0x45, true) => 0xff13,  // Pause
        (0x47, true) => 0xff50,  // Home
        (0x48, true) => 0xff52,  // Up
        (0x49, true) => 0xff55,  // Page_Up
        (0x4a, _) => 0xffad,     // KP_Subtract
        (0x4b, true) => 0xff51,  // Left
        (0x4d, true) => 0xff53,  // Right
        (0x4e, _) => 0xffab,     // KP_Add
        (0x4f, true) => 0xff57,  // End
        (0x50, true) => 0xff54,  // Down
        (0x51, true) => 0xff56,  // Page_Down
        (0x52, true) => 0xff63,  // Insert
        (0x53, true) => 0xffff,  // Delete
        (0x57, _) => 0xffc8,     // F11
        (0x58, _) => 0xffc9,     // F12
        (0x5b, _) => 0xffeb,     // Super_L
        (0x5c, _) => 0xffec,     // Super_R
        (0x5d, _) => 0xff67,     // Menu
        (0x10, true) => 0x1008FF16, // XF86AudioPrev
        (0x19, true) => 0x1008FF17, // XF86AudioNext
        (0x20, true) => 0x1008FF12, // XF86AudioMute
        (0x22, true) => 0x1008FF14, // XF86AudioPlay
        (0x24, true) => 0x1008FF15, // XF86AudioStop
        (0x2e, true) => 0x1008FF11, // XF86AudioLowerVolume
        (0x30, true) => 0x1008FF13, // XF86AudioRaiseVolume
        _ => return None,
    };

    Some(keysym)
}

fn alpha_from_scancode(code: u8) -> Option<char> {
    match code {
        0x10 => Some('q'),
        0x11 => Some('w'),
        0x12 => Some('e'),
        0x13 => Some('r'),
        0x14 => Some('t'),
        0x15 => Some('y'),
        0x16 => Some('u'),
        0x17 => Some('i'),
        0x18 => Some('o'),
        0x19 => Some('p'),
        0x1E => Some('a'),
        0x1F => Some('s'),
        0x20 => Some('d'),
        0x21 => Some('f'),
        0x22 => Some('g'),
        0x23 => Some('h'),
        0x24 => Some('j'),
        0x25 => Some('k'),
        0x26 => Some('l'),
        0x2C => Some('z'),
        0x2D => Some('x'),
        0x2E => Some('c'),
        0x2F => Some('v'),
        0x30 => Some('b'),
        0x31 => Some('n'),
        0x32 => Some('m'),
        _ => None,
    }
}

fn printable_symbol_from_scancode(code: u8) -> Option<(char, char)> {
    match code {
        0x02 => Some(('1', '!')),
        0x03 => Some(('2', '@')),
        0x04 => Some(('3', '#')),
        0x05 => Some(('4', '$')),
        0x06 => Some(('5', '%')),
        0x07 => Some(('6', '^')),
        0x08 => Some(('7', '&')),
        0x09 => Some(('8', '*')),
        0x0A => Some(('9', '(')),
        0x0B => Some(('0', ')')),
        0x0C => Some(('-', '_')),
        0x0D => Some(('=', '+')),
        0x1A => Some(('[', '{')),
        0x1B => Some((']', '}')),
        0x27 => Some((';', ':')),
        0x28 => Some(('\'', '"')),
        0x29 => Some(('`', '~')),
        0x2B => Some(('\\', '|')),
        0x33 => Some((',', '<')),
        0x34 => Some(('.', '>')),
        0x35 => Some(('/', '?')),
        _ => None,
    }
}

fn numpad_keysym_from_scancode(code: u8, num_lock: bool) -> Option<u32> {
    let keysym = match code {
        0x47 => if num_lock { 0xffb7 } else { 0xff50 },
        0x48 => if num_lock { 0xffb8 } else { 0xff52 },
        0x49 => if num_lock { 0xffb9 } else { 0xff55 },
        0x4B => if num_lock { 0xffb4 } else { 0xff51 },
        0x4C => 0xffb5,
        0x4D => if num_lock { 0xffb6 } else { 0xff53 },
        0x4F => if num_lock { 0xffb1 } else { 0xff57 },
        0x50 => if num_lock { 0xffb2 } else { 0xff54 },
        0x51 => if num_lock { 0xffb3 } else { 0xff56 },
        0x52 => if num_lock { 0xffb0 } else { 0xff63 },
        0x53 => if num_lock { 0xffae } else { 0xffff },
        _ => return None,
    };

    Some(keysym)
}

async fn sync_keyboard_indicators(
    vnc: &VncClient,
    remote_lock_state: &mut Option<KeyboardIndicators>,
    local: KeyboardIndicators,
) -> Result<(), String> {
    // VNC does not provide absolute lock-state sync, only key toggles.
    // Use a conservative baseline and actively toggle toward local state.
    let state = remote_lock_state.get_or_insert(KeyboardIndicators::default());

    info!(
        "[VNC][LOCK] sync compare remote(num={} caps={} scroll={}) local(num={} caps={} scroll={})",
        state.num_lock,
        state.caps_lock,
        state.scroll_lock,
        local.num_lock,
        local.caps_lock,
        local.scroll_lock
    );

    if state.caps_lock != local.caps_lock {
        info!(
            "[VNC][LOCK] toggling CapsLock remote={} -> local={}",
            state.caps_lock, local.caps_lock
        );
        send_lock_toggle(vnc, 0xffe5).await?; // Caps_Lock
        state.caps_lock = local.caps_lock;
    }
    if state.num_lock != local.num_lock {
        info!(
            "[VNC][LOCK] toggling NumLock remote={} -> local={}",
            state.num_lock, local.num_lock
        );
        send_lock_toggle(vnc, 0xff7f).await?; // Num_Lock
        state.num_lock = local.num_lock;
    }
    if state.scroll_lock != local.scroll_lock {
        info!(
            "[VNC][LOCK] toggling ScrollLock remote={} -> local={}",
            state.scroll_lock, local.scroll_lock
        );
        send_lock_toggle(vnc, 0xff14).await?; // Scroll_Lock
        state.scroll_lock = local.scroll_lock;
    }

    info!(
        "[VNC][LOCK] sync result remote(num={} caps={} scroll={})",
        state.num_lock, state.caps_lock, state.scroll_lock
    );

    Ok(())
}

async fn send_lock_toggle(vnc: &VncClient, keysym: u32) -> Result<(), String> {
    info!("[VNC][LOCK] toggle keysym=0x{:X} down=true", keysym);
    send_key(vnc, keysym, true).await?;
    info!("[VNC][LOCK] toggle keysym=0x{:X} down=false", keysym);
    send_key(vnc, keysym, false).await
}

fn is_lock_scancode(code: u8, extended: bool) -> bool {
    matches!((code, extended), (0x3A, _) | (0x45, false) | (0x46, _))
}

fn is_modifier_scancode(code: u8, extended: bool) -> bool {
    matches!(
        (code, extended),
        (0x2A, false)
            | (0x36, false)
            | (0x1D, false)
            | (0x1D, true)
            | (0x38, false)
            | (0x38, true)
            | (0x5B, true)
            | (0x5C, true)
    )
}

fn write_raw_rect_to_framebuffer(
    framebuffer: &mut VncFramebuffer,
    x: u16,
    y: u16,
    width: u16,
    height: u16,
    rgba: &[u8],
) {
    if framebuffer.width == 0 || framebuffer.height == 0 {
        return;
    }

    let max_w = framebuffer.width.saturating_sub(x);
    let max_h = framebuffer.height.saturating_sub(y);
    let rect_w = width.min(max_w);
    let rect_h = height.min(max_h);
    if rect_w == 0 || rect_h == 0 {
        return;
    }

    let dst_stride = framebuffer.width as usize * 4;
    let row_bytes = rect_w as usize * 4;
    let expected = row_bytes * rect_h as usize;
    if rgba.len() < expected {
        return;
    }

    for row in 0..rect_h as usize {
        let src_start = row * row_bytes;
        let src_end = src_start + row_bytes;

        let dst_y = y as usize + row;
        let dst_start = dst_y * dst_stride + x as usize * 4;
        let dst_end = dst_start + row_bytes;

        if dst_end <= framebuffer.rgba.len() {
            framebuffer.rgba[dst_start..dst_end].copy_from_slice(&rgba[src_start..src_end]);
        }
    }
}

fn framebuffer_rect_update(framebuffer: &VncFramebuffer, rect: CursorRect) -> Option<FrameUpdate> {
    let len = rect.width as usize * rect.height as usize * 4;
    let mut out = Vec::with_capacity(len);
    let stride = framebuffer.width as usize * 4;
    let row_bytes = rect.width as usize * 4;

    for row in 0..rect.height as usize {
        let src_y = rect.y as usize + row;
        let src_start = src_y * stride + rect.x as usize * 4;
        let src_end = src_start + row_bytes;
        if src_end > framebuffer.rgba.len() {
            return None;
        }
        out.extend_from_slice(&framebuffer.rgba[src_start..src_end]);
    }

    Some(FrameUpdate::Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        rgba: out,
    })
}

fn compute_cursor_rect(
    framebuffer: &VncFramebuffer,
    cursor: &VncCursorState,
    pointer: &PointerState,
) -> Option<CursorRect> {
    if framebuffer.width == 0 || framebuffer.height == 0 || !cursor.has_shape() {
        return None;
    }

    let origin_x = pointer.x as i32 - cursor.hot_x as i32;
    let origin_y = pointer.y as i32 - cursor.hot_y as i32;
    let end_x = origin_x + cursor.width as i32;
    let end_y = origin_y + cursor.height as i32;

    let clipped_x0 = origin_x.clamp(0, framebuffer.width as i32);
    let clipped_y0 = origin_y.clamp(0, framebuffer.height as i32);
    let clipped_x1 = end_x.clamp(0, framebuffer.width as i32);
    let clipped_y1 = end_y.clamp(0, framebuffer.height as i32);

    if clipped_x1 <= clipped_x0 || clipped_y1 <= clipped_y0 {
        return None;
    }

    Some(CursorRect {
        x: clipped_x0 as u16,
        y: clipped_y0 as u16,
        width: (clipped_x1 - clipped_x0) as u16,
        height: (clipped_y1 - clipped_y0) as u16,
    })
}

fn draw_cursor_overlay_update(
    framebuffer: &VncFramebuffer,
    cursor: &mut VncCursorState,
    pointer: &PointerState,
) -> Option<FrameUpdate> {
    let rect = compute_cursor_rect(framebuffer, cursor, pointer)?;

    let origin_x = pointer.x as i32 - cursor.hot_x as i32;
    let origin_y = pointer.y as i32 - cursor.hot_y as i32;
    let src_x0 = (rect.x as i32 - origin_x) as usize;
    let src_y0 = (rect.y as i32 - origin_y) as usize;

    let mut out = vec![0; rect.width as usize * rect.height as usize * 4];
    let fb_stride = framebuffer.width as usize * 4;
    let cursor_stride = cursor.width as usize * 4;

    for row in 0..rect.height as usize {
        for col in 0..rect.width as usize {
            let fb_x = rect.x as usize + col;
            let fb_y = rect.y as usize + row;
            let bg_idx = fb_y * fb_stride + fb_x * 4;

            let cx = src_x0 + col;
            let cy = src_y0 + row;
            let cur_idx = cy * cursor_stride + cx * 4;

            if bg_idx + 3 >= framebuffer.rgba.len() || cur_idx + 3 >= cursor.rgba.len() {
                continue;
            }

            let src_r = cursor.rgba[cur_idx] as f32;
            let src_g = cursor.rgba[cur_idx + 1] as f32;
            let src_b = cursor.rgba[cur_idx + 2] as f32;
            let src_a = cursor.rgba[cur_idx + 3] as f32 / 255.0;

            let dst_r = framebuffer.rgba[bg_idx] as f32;
            let dst_g = framebuffer.rgba[bg_idx + 1] as f32;
            let dst_b = framebuffer.rgba[bg_idx + 2] as f32;

            let out_idx = (row * rect.width as usize + col) * 4;
            out[out_idx] = (src_r * src_a + dst_r * (1.0 - src_a)).round() as u8;
            out[out_idx + 1] = (src_g * src_a + dst_g * (1.0 - src_a)).round() as u8;
            out[out_idx + 2] = (src_b * src_a + dst_b * (1.0 - src_a)).round() as u8;
            out[out_idx + 3] = 255;
        }
    }

    cursor.last_rect = Some(rect);

    Some(FrameUpdate::Rect {
        x: rect.x,
        y: rect.y,
        width: rect.width,
        height: rect.height,
        rgba: out,
    })
}

fn copy_rect_in_framebuffer(
    framebuffer: &mut VncFramebuffer,
    dst_x: u16,
    dst_y: u16,
    src_x: u16,
    src_y: u16,
    width: u16,
    height: u16,
) -> Option<FrameUpdate> {
    if framebuffer.width == 0 || framebuffer.height == 0 {
        return None;
    }

    let copy_w = width
        .min(framebuffer.width.saturating_sub(src_x))
        .min(framebuffer.width.saturating_sub(dst_x));
    let copy_h = height
        .min(framebuffer.height.saturating_sub(src_y))
        .min(framebuffer.height.saturating_sub(dst_y));

    if copy_w == 0 || copy_h == 0 {
        return None;
    }

    let fb_stride = framebuffer.width as usize * 4;
    let row_bytes = copy_w as usize * 4;
    let mut temp = vec![0u8; row_bytes * copy_h as usize];

    for row in 0..copy_h as usize {
        let src_start = (src_y as usize + row) * fb_stride + src_x as usize * 4;
        let src_end = src_start + row_bytes;
        if src_end > framebuffer.rgba.len() {
            return None;
        }

        let tmp_start = row * row_bytes;
        let tmp_end = tmp_start + row_bytes;
        temp[tmp_start..tmp_end].copy_from_slice(&framebuffer.rgba[src_start..src_end]);
    }

    for row in 0..copy_h as usize {
        let dst_start = (dst_y as usize + row) * fb_stride + dst_x as usize * 4;
        let dst_end = dst_start + row_bytes;
        if dst_end > framebuffer.rgba.len() {
            return None;
        }

        let tmp_start = row * row_bytes;
        let tmp_end = tmp_start + row_bytes;
        framebuffer.rgba[dst_start..dst_end].copy_from_slice(&temp[tmp_start..tmp_end]);
    }

    Some(FrameUpdate::Rect {
        x: dst_x,
        y: dst_y,
        width: copy_w,
        height: copy_h,
        rgba: temp,
    })
}