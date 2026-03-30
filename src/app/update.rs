// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::widget::operation::focus;
use iced::{keyboard, window, Task};

use crate::app::{LocalShellOption, Message, RemoteDisplayProtocol, Session, SessionKind, State};
use crate::connection;
use crate::connection::remote_input_policy::{
    current_keyboard_indicators, remote_secure_attention_inputs, unicode_inputs_for_text,
};
use crate::platform;
use crate::remote_display::RemoteDisplayState;
use crate::terminal::Selection;

pub fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::TabSelected(index) => {
            if index < state.sessions.len() {
                state.active_index = index;
            }
            Task::none()
        }
        Message::CloseTab(index) => {
            if state.sessions.len() <= 1 {
                return Task::none();
            }
            // Explicitly drop the sender before removing so the worker receives
            // the channel-closed signal and exits its recv() loop immediately.
            if let Some(session) = state.sessions.get_mut(index) {
                session.sender = None;
                // Clean up the native clipboard window for this session.
                platform::windows::remove_clipboard_for_session(session.id);
            }
            state.sessions.remove(index);
            if state.active_index >= state.sessions.len() {
                state.active_index = state.sessions.len() - 1;
            }
            Task::none()
        }
        Message::NewSshTab => {
            let id = state.next_session_id;
            state.next_session_id += 1;
            state.sessions.push(Session::welcome(id));
            state.active_index = state.sessions.len() - 1;
            Task::none()
        }
        Message::TerminalInput(bytes) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if let Some(ref sender) = session.sender {
                    let _ = sender.send(connection::ConnectionInput::Data(bytes));
                } else {
                    session.terminal.process_bytes(&bytes);
                }
            }
            Task::none()
        }
        Message::ImePreedit(preedit) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                session.terminal.ime_preedit = preedit;
                session.terminal.cache.clear();
            }
            Task::none()
        }
        Message::ImeCommit(text_str) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if matches!(session.kind, SessionKind::RemoteDisplay) {
                    if let Some(ref sender) = session.sender {
                        for input in unicode_inputs_for_text(&text_str) {
                            let _ = sender.send(connection::ConnectionInput::RemoteInput(input));
                        }
                    }
                } else {
                    let bytes = text_str.into_bytes();
                    if let Some(ref sender) = session.sender {
                        let _ = sender.send(connection::ConnectionInput::Data(bytes));
                    } else {
                        session.terminal.process_bytes(&bytes);
                    }
                }
                session.terminal.clear_preedit();
            }
            Task::none()
        }
        Message::FontLoaded(_) => Task::none(),
        Message::ConnectionMessage(target_id, event) => {
            let maybe_index = state.sessions.iter().position(|s| s.id == target_id);
            if let Some(target_index) = maybe_index {
                let session = &mut state.sessions[target_index];
                let mut schedule_redraw_pulse = false;
                match event {
                    connection::ConnectionEvent::Connected(sender) => {
                        session.sender = Some(sender.clone());
                        if matches!(session.kind, SessionKind::RemoteDisplay) {
                            let (win_w, win_h) = state.window_size;
                            let pixel_w = ((win_w - 181.0).max(200.0)) as u16;
                            let pixel_h = ((win_h - 66.0).max(200.0)) as u16;
                            let _ = sender.send(connection::ConnectionInput::Resize {
                                cols: pixel_w,
                                rows: pixel_h,
                            });
                            let _ = sender.send(connection::ConnectionInput::SyncKeyboardIndicators(
                                current_keyboard_indicators(),
                            ));
                        } else {
                            let _ = sender.send(connection::ConnectionInput::Resize {
                                cols: session.terminal.cols as u16,
                                rows: session.terminal.rows as u16,
                            });
                        }
                    }
                    connection::ConnectionEvent::Data(data) => {
                        if matches!(session.kind, SessionKind::RemoteDisplay) {
                            if !data.is_empty() {
                                let msg = String::from_utf8_lossy(&data).trim().to_string();
                                if !msg.is_empty() {
                                    log::info!("[REMOTE] {}", msg);
                                }
                            }
                        } else if !data.is_empty() {
                            session.terminal.process_bytes(&data);
                            session.terminal.cache.clear();
                            let responses: Vec<Vec<u8>> =
                                session.terminal.pending_responses.drain(..).collect();
                            for resp in responses {
                                if let Some(ref sender) = session.sender {
                                    let _ = sender.send(connection::ConnectionInput::Data(resp));
                                }
                            }
                        }
                    }
                    connection::ConnectionEvent::Frames(frames) => {
                        let remote_display = &mut session.remote_display;
                        let frame_batch_len = frames.len();

                        if remote_display.is_none() {
                            *remote_display = Some(RemoteDisplayState::new(1280, 720));
                        }
                        if let Some(display) = remote_display.as_mut() {
                            display.status_message = None;

                            let batch_stats = display.apply_batch(frames);

                            if crate::rdp_trace_enabled() {
                                log::info!(
                                    "[RDP-UI] session_id={} frames={} full={} rect={}",
                                    target_id,
                                    frame_batch_len,
                                    batch_stats.full_count,
                                    batch_stats.rect_count,
                                );
                                if let Some(reason) = batch_stats.forced_full_upload_reason {
                                    log::info!(
                                        "[RDP-UI] force_full_upload session_id={} reason={} rects={}",
                                        target_id,
                                        reason.as_str(),
                                        batch_stats.rect_count,
                                    );
                                }
                            }

                            schedule_redraw_pulse = true;
                            // No flush needed — shader widget reads state directly in view()
                        }
                    }
                    connection::ConnectionEvent::Disconnected => {
                        session.sender = None;
                        if let Some(display) = session.remote_display.as_mut() {
                            display.status_message = Some("Disconnected".to_string());
                        } else {
                            session
                                .terminal
                                .process_bytes(b"\r\n\x1b[31m[Disconnected]\x1b[0m\r\n");
                            session.terminal.cache.clear();
                        }
                    }
                    connection::ConnectionEvent::Error(e) => {
                        session.sender = None;
                        log::error!("[Connection Error] {}", e);
                        if let Some(display) = session.remote_display.as_mut() {
                            display.status_message = Some(format!("Error: {}", e));
                        } else {
                            let msg = format!("\r\n\x1b[31m[Error: {}]\x1b[0m\r\n", e);
                            session.terminal.process_bytes(msg.as_bytes());
                            session.terminal.cache.clear();
                        }
                    }
                }

                if schedule_redraw_pulse {
                    return Task::perform(
                        async {
                            tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                        },
                        |_| Message::RemoteDisplayRedrawPulse,
                    );
                }
            }
            Task::none()
        }
        Message::RemoteDisplayRedrawPulse => Task::none(),
        Message::TerminalResize(new_rows, new_cols) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if session.terminal.rows != new_rows || session.terminal.cols != new_cols {
                    session.terminal.resize(new_rows, new_cols);
                    if let Some(ref sender) = session.sender {
                        let _ = sender.send(connection::ConnectionInput::Resize {
                            cols: new_cols as u16,
                            rows: new_rows as u16,
                        });
                    }
                }
            }
            Task::none()
        }
        Message::TerminalScroll(delta) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                let max_scroll = session.terminal.history.len();
                if delta > 0.0 {
                    session.terminal.display_offset =
                        std::cmp::min(session.terminal.display_offset + 3, max_scroll);
                } else {
                    session.terminal.display_offset =
                        session.terminal.display_offset.saturating_sub(3);
                }
                session.terminal.cache.clear();
            }
            Task::none()
        }
        Message::TerminalScrollTo(offset) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                session.terminal.display_offset = offset;
                session.terminal.cache.clear();
            }
            Task::none()
        }
        Message::HostChanged(s) => {
            state.ssh_host = s;
            Task::none()
        }
        Message::PortChanged(s) => {
            state.ssh_port = s;
            Task::none()
        }
        Message::UserChanged(s) => {
            state.ssh_user = s;
            Task::none()
        }
        Message::PassChanged(s) => {
            state.ssh_pass = s;
            Task::none()
        }
        Message::SerialPortChanged(s) => {
            state.serial_port = s;
            Task::none()
        }
        Message::SerialBaudChanged(s) => {
            state.serial_baud = s;
            Task::none()
        }
        Message::RdpHostChanged(s) => {
            state.rdp_host = s;
            Task::none()
        }
        Message::RdpPortChanged(s) => {
            state.rdp_port = s;
            Task::none()
        }
        Message::RdpUserChanged(s) => {
            state.rdp_user = s;
            Task::none()
        }
        Message::RdpPassChanged(s) => {
            state.rdp_pass = s;
            Task::none()
        }
        Message::RdpResolutionSelected(index) => {
            state.selected_rdp_resolution_index = index;
            Task::none()
        }
        Message::VncHostChanged(s) => {
            state.vnc_host = s;
            Task::none()
        }
        Message::VncPortChanged(s) => {
            state.vnc_port = s;
            Task::none()
        }
        Message::VncPassChanged(s) => {
            state.vnc_pass = s;
            Task::none()
        }
        Message::SelectProtocol(mode) => {
            state.welcome_protocol = mode;
            state.focused_field = 0;
            let fields = state.current_field_ids();
            if let Some(first) = fields.first() {
                focus(first.clone())
            } else {
                Task::none()
            }
        }
        Message::SelectLocalShell(index) => {
            if index < state.local_shells.len() {
                state.selected_local_shell = index;
            }
            Task::none()
        }
        Message::ConnectSsh => {
            let host = state.ssh_host.clone();
            let port: u16 = state.ssh_port.parse().unwrap_or(22);
            let user = state.ssh_user.clone();
            let pass = state.ssh_pass.clone();
            let keepalive: u64 = state.settings_ssh_keepalive.parse().unwrap_or(0);
            let terminal_type = state.settings_ssh_terminal_type.clone();
            let name = format!("SSH: {}@{}", user, host);
            let target_index = state.active_index;
            let mut target_id = None;
            if let Some(session) = state.sessions.get_mut(target_index) {
                target_id = Some(session.id);
                *session =
                    Session::new_terminal(session.id, name, session.terminal.rows, session.terminal.cols);
            }
            if let Some(target_id) = target_id {
                Task::run(
                    connection::ssh::connect_and_subscribe(host, port, user, pass, keepalive, terminal_type),
                    move |event| Message::ConnectionMessage(target_id, event),
                )
            } else {
                Task::none()
            }
        }
        Message::ConnectTelnet => {
            let host = state.ssh_host.clone();
            let port: u16 = state.ssh_port.parse().unwrap_or(23); // Telnet default
            let name = format!("Telnet: {}:{}", host, port);
            let target_index = state.active_index;
            let mut target_id = None;
            if let Some(session) = state.sessions.get_mut(target_index) {
                target_id = Some(session.id);
                *session =
                    Session::new_terminal(session.id, name, session.terminal.rows, session.terminal.cols);
            }
            if let Some(target_id) = target_id {
                Task::run(connection::telnet::connect_and_subscribe(host, port), move |event| {
                    Message::ConnectionMessage(target_id, event)
                })
            } else {
                Task::none()
            }
        }
        Message::ConnectSerial => {
            let port_name = state.serial_port.clone();
            let baud: u32 = state.serial_baud.parse().unwrap_or(115200);
            let data_bits = state.settings_serial_data_bits.clone();
            let stop_bits = state.settings_serial_stop_bits.clone();
            let parity = state.settings_serial_parity.clone();
            let hw_flow = state.settings_serial_hardware_flow_control;
            let name = format!("Serial: {} ({}bps)", port_name, baud);
            let target_index = state.active_index;
            let mut target_id = None;
            if let Some(session) = state.sessions.get_mut(target_index) {
                target_id = Some(session.id);
                *session =
                    Session::new_terminal(session.id, name, session.terminal.rows, session.terminal.cols);
            }
            if let Some(target_id) = target_id {
                Task::run(
                    connection::serial::connect_and_subscribe(port_name, baud, data_bits, stop_bits, parity, hw_flow),
                    move |event| Message::ConnectionMessage(target_id, event),
                )
            } else {
                Task::none()
            }
        }
        Message::ConnectLocal => {
            let shell = state
                .local_shells
                .get(state.selected_local_shell)
                .cloned()
                .unwrap_or(LocalShellOption {
                    name: "Windows PowerShell".to_string(),
                    program: "powershell.exe".to_string(),
                    args: vec!["-NoLogo".to_string(), "-NoExit".to_string()],
                });

            let name = format!("Local: {}", shell.name);
            let target_index = state.active_index;
            let mut target_id = None;
            if let Some(session) = state.sessions.get_mut(target_index) {
                target_id = Some(session.id);
                *session =
                    Session::new_terminal(session.id, name, session.terminal.rows, session.terminal.cols);
            }
            if let Some(target_id) = target_id {
                Task::run(
                    platform::windows::spawn_local_shell(shell.program, shell.args),
                    move |event| Message::ConnectionMessage(target_id, event),
                )
            } else {
                Task::none()
            }
        }
        Message::ConnectRdp => {
            let host = state.rdp_host.clone();
            let port: u16 = state.rdp_port.parse().unwrap_or(3389);
            let user = state.rdp_user.clone();
            let pass = state.rdp_pass.clone();
            let (width, height) = crate::RDP_RESOLUTION_PRESETS
                .get(state.selected_rdp_resolution_index)
                .copied()
                .unwrap_or((1280, 720));
            let name = format!("RDP: {}@{}:{}", user, host, port);

            let target_index = state.active_index;
            let mut target_id = None;
            if let Some(session) = state.sessions.get_mut(target_index) {
                target_id = Some(session.id);
                *session = Session::new_remote_display(
                    session.id,
                    name,
                    width,
                    height,
                    RemoteDisplayProtocol::Rdp,
                );
            }

            if let Some(target_id) = target_id {
                let (cliprdr_factory, clipboard_rx_opt) =
                    platform::windows::create_cliprdr_backend(target_id);

                let enable_credssp = state.settings_rdp_nla;
                let enable_audio = state.settings_rdp_enable_audio;
                let color_depth: u32 = state.settings_rdp_color_depth.parse().unwrap_or(32);
                let font_smoothing = state.settings_rdp_font_smoothing;
                let desktop_composition = state.settings_rdp_desktop_composition;

                Task::run(
                    connection::rdp::connect_and_subscribe(
                        host,
                        port,
                        user,
                        pass,
                        width,
                        height,
                        cliprdr_factory,
                        clipboard_rx_opt,
                        enable_credssp,
                        enable_audio,
                        color_depth,
                        font_smoothing,
                        desktop_composition,
                    ),
                    move |event| Message::ConnectionMessage(target_id, event),
                )
            } else {
                Task::none()
            }
        }
        Message::ConnectVnc => {
            let host = state.vnc_host.clone();
            let port: u16 = state.vnc_port.parse().unwrap_or(5900);
            let pass = state.vnc_pass.clone();
            let pass_opt = if pass.is_empty() { None } else { Some(pass) };
            let remote_cursor = state.settings_vnc_remote_cursor;
            let shared_session = state.settings_vnc_shared_session;
            let view_only = state.settings_vnc_view_only;
            let timeout_secs: u64 = state.settings_vnc_timeout.parse().unwrap_or(10);
            let name = format!("VNC: {}:{}", host, port);

            let target_index = state.active_index;
            let mut target_id = None;
            if let Some(session) = state.sessions.get_mut(target_index) {
                target_id = Some(session.id);
                *session = Session::new_remote_display(
                    session.id,
                    name,
                    1280,
                    720,
                    RemoteDisplayProtocol::Vnc,
                );
                if let Some(display) = session.remote_display.as_mut() {
                    display.status_message = Some("Connecting to VNC server...".to_string());
                }
            }

            if let Some(target_id) = target_id {
                Task::run(
                    connection::vnc::connect_and_subscribe(
                        host, port, pass_opt,
                        remote_cursor, shared_session, view_only, timeout_secs,
                    ),
                    move |event| Message::ConnectionMessage(target_id, event),
                )
            } else {
                Task::none()
            }
        }

        Message::SelectionStart(col, row) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                session.terminal.selection = Some(Selection {
                    start: (col, row),
                    end: (col, row),
                });
                session.terminal.cache.clear();
            }
            Task::none()
        }
        Message::SelectionUpdate(col, row) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if let Some(ref mut sel) = session.terminal.selection {
                    sel.end = (col, row);
                    session.terminal.cache.clear();
                }
            }
            Task::none()
        }
        Message::CopyCurrentSelection => {
            if let Some(session) = state.sessions.get(state.active_index) {
                let text = session.terminal.get_selected_text();
                return Task::done(Message::CopyText(text));
            }
            Task::none()
        }
        Message::CopyText(text) => {
            if !text.is_empty() {
                return iced::clipboard::write(text);
            }
            Task::none()
        }
        Message::PasteFromClipboard => iced::clipboard::read().map(Message::PasteData),
        Message::PasteData(text_opt) => {
            if let Some(text) = text_opt {
                if let Some(session) = state.sessions.get_mut(state.active_index) {
                    let filtered_text: String = text.chars().filter(|&c| c != '\0').collect();
                    let bytes = filtered_text.into_bytes();
                    if let Some(ref sender) = session.sender {
                        let _ = sender.send(connection::ConnectionInput::Data(bytes));
                    } else {
                        session.terminal.process_bytes(&bytes);
                    }
                }
            }
            Task::none()
        }
        Message::ClearSelection => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                session.terminal.selection = None;
                session.terminal.cache.clear();
            }
            Task::none()
        }
        Message::TryHandleKey(key, modifiers) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                let has_sel = session.terminal.has_selection();
                let ctrl = modifiers.control();

                if ctrl && matches!(key, keyboard::Key::Character(ref c) if c == "c" || c == "C") {
                    if has_sel {
                        let text = session.terminal.get_selected_text();
                        return Task::done(Message::CopyText(text));
                    } else {
                        return Task::done(Message::TerminalInput(vec![3]));
                    }
                }

                if ctrl && matches!(key, keyboard::Key::Character(ref c) if c == "v" || c == "V") {
                    return Task::done(Message::PasteFromClipboard);
                }

                if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                    if has_sel {
                        return Task::done(Message::ClearSelection);
                    } else {
                        return Task::done(Message::TerminalInput(vec![27]));
                    }
                }
            }
            Task::none()
        }
        Message::RemoteDisplayInput(input) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if let Some(ref sender) = session.sender {
                    let input =
                        transform_remote_mouse(input, state.window_size, session.remote_display.as_ref());
                    let _ = sender.send(connection::ConnectionInput::RemoteInput(input));
                }
            }
            Task::none()
        }
        Message::RemoteDisplayInputs(inputs) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if let Some(ref sender) = session.sender {
                    for input in inputs {
                        let input = transform_remote_mouse(
                            input,
                            state.window_size,
                            session.remote_display.as_ref(),
                        );
                        let _ = sender.send(connection::ConnectionInput::RemoteInput(input));
                    }
                }
            }
            Task::none()
        }
        Message::RemoteSecureAttention(active) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if session.is_rdp_display() {
                    if let Some(ref sender) = session.sender {
                        for input in remote_secure_attention_inputs(active) {
                            let _ = sender.send(connection::ConnectionInput::RemoteInput(input));
                        }
                    }
                }
            }
            Task::none()
        }
        Message::SyncRemoteKeyboardIndicators => {
            if let Some(session) = state.sessions.get(state.active_index) {
                if matches!(session.kind, SessionKind::RemoteDisplay) {
                    if let Some(ref sender) = session.sender {
                        let _ = sender.send(connection::ConnectionInput::SyncKeyboardIndicators(
                            current_keyboard_indicators(),
                        ));
                    }
                }
            }
            Task::none()
        }
        Message::ReleaseRemoteModifiers => {
            if let Some(session) = state.sessions.get(state.active_index) {
                if matches!(session.kind, SessionKind::RemoteDisplay) {
                    if let Some(ref sender) = session.sender {
                        let _ = sender.send(connection::ConnectionInput::ReleaseAllModifiers);
                    }
                }
            }
            Task::none()
        }
        Message::WindowSizeChanged(w, h) => {
            state.window_size = (w, h);
            if let Some(session) = state.sessions.get(state.active_index) {
                if matches!(session.kind, SessionKind::RemoteDisplay) {
                    if let Some(ref sender) = session.sender {
                        let pixel_w = ((w - 181.0).max(200.0)) as u16;
                        let pixel_h = ((h - 66.0).max(200.0)) as u16;
                        let _ = sender.send(connection::ConnectionInput::Resize {
                            cols: pixel_w,
                            rows: pixel_h,
                        });
                    }
                }
            }
            Task::none()
        }
        Message::TabPressed(shift) => {
            let fields = state.current_field_ids();
            if fields.is_empty() {
                return Task::none();
            }
            let count = fields.len();
            if shift {
                state.focused_field = if state.focused_field == 0 {
                    count - 1
                } else {
                    state.focused_field - 1
                };
            } else {
                state.focused_field = (state.focused_field + 1) % count;
            }
            focus(fields[state.focused_field].clone())
        }
        Message::FieldFocused(index) => {
            state.focused_field = index;
            Task::none()
        }
        Message::WindowIdCaptured(id) => {
            if state.window_id.is_none() {
                state.window_id = Some(id);
            }
            Task::none()
        }
        Message::WindowDrag => {
            if let Some(id) = state.window_id {
                window::drag(id)
            } else {
                Task::none()
            }
        }
        Message::WindowResize(direction) => {
            state.resizing_direction = Some(direction);
            if let Some(id) = state.window_id {
                window::drag_resize(id, direction)
            } else {
                Task::none()
            }
        }
        Message::ResizeFinished => {
            state.resizing_direction = None;
            Task::none()
        }
        Message::MinimizeWindow => {
            if let Some(id) = state.window_id {
                window::minimize(id, true)
            } else {
                Task::none()
            }
        }
        Message::MaximizeWindow => {
            if let Some(id) = state.window_id {
                window::toggle_maximize(id)
            } else {
                Task::none()
            }
        }
        Message::CloseWindow => {
            if let Some(id) = state.window_id {
                window::close(id)
            } else {
                Task::none()
            }
        }
        Message::ToggleMenu(menu) => {
            if menu.is_empty() {
                state.dummy_menu_open = None;
            } else if state.dummy_menu_open == Some(menu) {
                state.dummy_menu_open = None;
            } else {
                state.dummy_menu_open = Some(menu);
            }
            Task::none()
        }
        Message::CloseMenuDeferred => Task::perform(async {}, |_| Message::CloseMenu),
        Message::CloseMenu => {
            state.dummy_menu_open = None;
            Task::none()
        }
        Message::OpenProtocolTab(mode) => {
            let id = state.next_session_id;
            state.next_session_id += 1;
            state.sessions.push(Session::welcome(id));
            state.active_index = state.sessions.len() - 1;
            state.welcome_protocol = mode;
            state.dummy_menu_open = None;
            Task::none()
        }
        Message::OpenSettingsTab(tab_kind) => {
            let id = state.next_session_id;
            state.next_session_id += 1;
            state.settings_selected_category = 0;
            state.sessions.push(Session::new_settings(id, tab_kind));
            state.active_index = state.sessions.len() - 1;
            state.dummy_menu_open = None;
            Task::none()
        }
        Message::SettingsCategorySelected(index) => {
            state.settings_selected_category = index;
            Task::none()
        }
        Message::ToggleSettingsCheckbox(key) => {
            state.toggle_settings_checkbox(key);
            super::settings_persistence::save_settings(state);
            Task::none()
        }
        Message::SettingsTextChanged(key, value) => {
            state.set_settings_text(key, value);
            super::settings_persistence::save_settings(state);
            Task::none()
        }
    }
}

fn transform_remote_mouse(
    input: connection::RemoteInput,
    window_size: (f32, f32),
    display: Option<&RemoteDisplayState>,
) -> connection::RemoteInput {
    match input {
        connection::RemoteInput::MouseMove { x, y } => {
            const CONTENT_X: f32 = 181.0; // sidebar(180) + vr(1)
            const CONTENT_Y: f32 = 66.0; // title_bar(35) + tab_bar(30) + hr(1)

            let (win_w, win_h) = window_size;
            let content_w = (win_w - CONTENT_X).max(1.0);
            let content_h = (win_h - CONTENT_Y).max(1.0);

            let rel_x = (x as f32 - CONTENT_X).max(0.0);
            let rel_y = (y as f32 - CONTENT_Y).max(0.0);

            if let Some(display) = display {
                let desk_w = display.width as f32;
                let desk_h = display.height as f32;

                // Compute contain-fit offset & scale (mirrors the WGSL shader logic)
                let vp_aspect = content_w / content_h;
                let tex_aspect = desk_w / desk_h;
                let (scale_x, scale_y) = if tex_aspect > vp_aspect {
                    (1.0, vp_aspect / tex_aspect)
                } else {
                    (tex_aspect / vp_aspect, 1.0)
                };
                let rendered_w = content_w * scale_x;
                let rendered_h = content_h * scale_y;
                let offset_x = (content_w - rendered_w) * 0.5;
                let offset_y = (content_h - rendered_h) * 0.5;

                let remote_x = ((rel_x - offset_x) / rendered_w * desk_w).clamp(0.0, desk_w - 1.0) as u16;
                let remote_y = ((rel_y - offset_y) / rendered_h * desk_h).clamp(0.0, desk_h - 1.0) as u16;
                connection::RemoteInput::MouseMove {
                    x: remote_x,
                    y: remote_y,
                }
            } else {
                connection::RemoteInput::MouseMove {
                    x: rel_x as u16,
                    y: rel_y as u16,
                }
            }
        }
        other => other,
    }
}
