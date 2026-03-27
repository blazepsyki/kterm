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
    let mut refresh = tokio::time::interval(VNC_REFRESH_INTERVAL);
    refresh.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            input = rx_from_iced.recv() => {
                match input {
                    Some(input) => {
                        handle_connection_input(&vnc, &mut pointer, &mut remote_lock_state, input).await?
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
    input: ConnectionInput,
) -> Result<(), String> {
    match input {
        ConnectionInput::RemoteInput(remote) => handle_remote_input(vnc, pointer, remote).await,
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
    remote: RemoteInput,
) -> Result<(), String> {
    match remote {
        RemoteInput::KeyboardScancode {
            code,
            extended,
            down,
        } => {
            if let Some(keysym) = keysym_from_scancode(code, extended) {
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

fn keysym_from_scancode(code: u8, extended: bool) -> Option<u32> {
    let keysym = match (code, extended) {
        (0x01, _) => 0xff1b, // Escape
        (0x02, _) => '1' as u32,
        (0x03, _) => '2' as u32,
        (0x04, _) => '3' as u32,
        (0x05, _) => '4' as u32,
        (0x06, _) => '5' as u32,
        (0x07, _) => '6' as u32,
        (0x08, _) => '7' as u32,
        (0x09, _) => '8' as u32,
        (0x0a, _) => '9' as u32,
        (0x0b, _) => '0' as u32,
        (0x0c, _) => '-' as u32,
        (0x0d, _) => '=' as u32,
        (0x0e, _) => 0xff08, // BackSpace
        (0x0f, _) => 0xff09, // Tab
        (0x10, _) => 'q' as u32,
        (0x11, _) => 'w' as u32,
        (0x12, _) => 'e' as u32,
        (0x13, _) => 'r' as u32,
        (0x14, _) => 't' as u32,
        (0x15, _) => 'y' as u32,
        (0x16, _) => 'u' as u32,
        (0x17, _) => 'i' as u32,
        (0x18, _) => 'o' as u32,
        (0x19, _) => 'p' as u32,
        (0x1a, _) => '[' as u32,
        (0x1b, _) => ']' as u32,
        (0x1c, false) => 0xff0d, // Return
        (0x1c, true) => 0xff8d,  // KP_Enter
        (0x1d, false) => 0xffe3, // Control_L
        (0x1d, true) => 0xffe4,  // Control_R
        (0x1e, _) => 'a' as u32,
        (0x1f, _) => 's' as u32,
        (0x20, _) => 'd' as u32,
        (0x21, _) => 'f' as u32,
        (0x22, _) => 'g' as u32,
        (0x23, _) => 'h' as u32,
        (0x24, _) => 'j' as u32,
        (0x25, _) => 'k' as u32,
        (0x26, _) => 'l' as u32,
        (0x27, _) => ';' as u32,
        (0x28, _) => '\'' as u32,
        (0x29, _) => '`' as u32,
        (0x2a, _) => 0xffe1, // Shift_L
        (0x2b, _) => '\\' as u32,
        (0x2c, _) => 'z' as u32,
        (0x2d, _) => 'x' as u32,
        (0x2e, _) => 'c' as u32,
        (0x2f, _) => 'v' as u32,
        (0x30, _) => 'b' as u32,
        (0x31, _) => 'n' as u32,
        (0x32, _) => 'm' as u32,
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
        (0x45, _) => 0xff7f,     // Num_Lock
        (0x46, _) => 0xff14,     // Scroll_Lock
        (0x47, false) => 0xffb7, // KP_7
        (0x47, true) => 0xff50,  // Home
        (0x48, false) => 0xffb8, // KP_8
        (0x48, true) => 0xff52,  // Up
        (0x49, false) => 0xffb9, // KP_9
        (0x49, true) => 0xff55,  // Page_Up
        (0x4a, _) => 0xffad,     // KP_Subtract
        (0x4b, false) => 0xffb4, // KP_4
        (0x4b, true) => 0xff51,  // Left
        (0x4c, false) => 0xffb5, // KP_5
        (0x4d, false) => 0xffb6, // KP_6
        (0x4d, true) => 0xff53,  // Right
        (0x4e, _) => 0xffab,     // KP_Add
        (0x4f, false) => 0xffb1, // KP_1
        (0x4f, true) => 0xff57,  // End
        (0x50, false) => 0xffb2, // KP_2
        (0x50, true) => 0xff54,  // Down
        (0x51, false) => 0xffb3, // KP_3
        (0x51, true) => 0xff56,  // Page_Down
        (0x52, false) => 0xffb0, // KP_0
        (0x52, true) => 0xff63,  // Insert
        (0x53, false) => 0xffae, // KP_Decimal
        (0x53, true) => 0xffff,  // Delete
        (0x57, _) => 0xffc8,     // F11
        (0x58, _) => 0xffc9,     // F12
        (0x5b, _) => 0xffeb,     // Super_L
        (0x5c, _) => 0xffec,     // Super_R
        (0x5d, _) => 0xff67,     // Menu
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

    if state.caps_lock != local.caps_lock {
        send_lock_toggle(vnc, 0xffe5).await?; // Caps_Lock
        state.caps_lock = local.caps_lock;
    }
    if state.num_lock != local.num_lock {
        send_lock_toggle(vnc, 0xff7f).await?; // Num_Lock
        state.num_lock = local.num_lock;
    }
    if state.scroll_lock != local.scroll_lock {
        send_lock_toggle(vnc, 0xff14).await?; // Scroll_Lock
        state.scroll_lock = local.scroll_lock;
    }

    Ok(())
}

async fn send_lock_toggle(vnc: &VncClient, keysym: u32) -> Result<(), String> {
    debug!("[VNC] lock toggle keysym=0x{:x}", keysym);
    send_key(vnc, keysym, true).await?;
    send_key(vnc, keysym, false).await
}