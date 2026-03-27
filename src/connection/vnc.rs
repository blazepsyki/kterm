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
        .add_encoding(VncEncoding::Raw)
        .add_encoding(VncEncoding::DesktopSizePseudo)
        .build()
        .map_err(|e| format!("VNC connector build failed: {}", e))?
        .try_start()
        .await
        .map_err(|e| format!("VNC handshake/auth failed: {}", e))?
        .finish()
        .map_err(|e| format!("VNC client finalize failed: {}", e))?;

    let _ = tx_from_worker.send(ConnectionEvent::Connected(tx_to_vnc));

    let summary = format!(
        "\r\n[VNC] Connected: {}:{} (encodings: Raw, DesktopSize)\r\n",
        host, port
    );
    let _ = tx_from_worker.send(ConnectionEvent::Data(summary.into_bytes()));

    // Force the first full framebuffer so UI starts from a known consistent image.
    vnc.input(X11Event::FullRefresh)
        .await
        .map_err(|e| format!("VNC initial full refresh failed: {}", e))?;

    let mut pointer = PointerState::default();
    let mut remote_lock_state: Option<KeyboardIndicators> = None;
    let mut key_state = VncKeyState::default();
    let mut refresh = tokio::time::interval(VNC_REFRESH_INTERVAL);
    refresh.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            input = rx_from_iced.recv() => {
                match input {
                    Some(input) => {
                        handle_connection_input(
                            &vnc,
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
                vnc.input(X11Event::Refresh)
                    .await
                    .map_err(|e| format!("VNC refresh request failed: {}", e))?;

                loop {
                    match vnc.poll_event().await {
                        Ok(Some(event)) => {
                            if handle_vnc_event(&tx_from_worker, event).await? {
                                let _ = vnc.close().await;
                                let _ = tx_from_worker.send(ConnectionEvent::Disconnected);
                                return Ok(());
                            }
                        }
                        Ok(None) => break,
                        Err(e) => {
                            return Err(format!("VNC event polling failed: {}", e));
                        }
                    }
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

async fn handle_vnc_event(
    tx_from_worker: &mpsc::UnboundedSender<ConnectionEvent>,
    event: VncEvent,
) -> Result<bool, String> {
    match event {
        VncEvent::SetResolution(screen) => {
            let rgba = vec![0; screen.width as usize * screen.height as usize * 4];
            let update = FrameUpdate::Full {
                width: screen.width,
                height: screen.height,
                rgba,
            };
            let _ = tx_from_worker.send(ConnectionEvent::Frames(vec![update]));
            debug!("[VNC] resolution {}x{}", screen.width, screen.height);
        }
        VncEvent::RawImage(rect, data) => {
            let expected = rect.width as usize * rect.height as usize * 4;
            if data.len() < expected {
                warn!(
                    "[VNC] raw image too small: got {} bytes, expected {}",
                    data.len(),
                    expected
                );
                return Ok(false);
            }

            let rgba = if data.len() == expected {
                data
            } else {
                data[..expected].to_vec()
            };

            let update = FrameUpdate::Rect {
                x: rect.x,
                y: rect.y,
                width: rect.width,
                height: rect.height,
                rgba,
            };
            let _ = tx_from_worker.send(ConnectionEvent::Frames(vec![update]));
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
        }
        VncEvent::Text(text) => {
            let _ = tx_from_worker.send(ConnectionEvent::Data(
                format!("\r\n[VNC] Clipboard text from server: {}\r\n", text).into_bytes(),
            ));
        }
        VncEvent::Bell => {
            info!("[VNC] bell");
        }
        VncEvent::SetCursor(_, _) => {
            // CursorPseudo integration will be handled in a later phase.
        }
        VncEvent::Copy(_, _) => {
            // CopyRect is not negotiated in the MVP stage.
        }
        VncEvent::JpegImage(_, _) => {
            // Tight/JPEG is not negotiated in the MVP stage.
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
        }
    }

    Ok(false)
}

async fn handle_connection_input(
    vnc: &VncClient,
    pointer: &mut PointerState,
    remote_lock_state: &mut Option<KeyboardIndicators>,
    key_state: &mut VncKeyState,
    input: ConnectionInput,
) -> Result<(), String> {
    match input {
        ConnectionInput::RemoteInput(remote) => {
            handle_remote_input(vnc, pointer, key_state, remote).await
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
            send_pointer(vnc, pointer.x, pointer.y, pointer.buttons).await
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