// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::widget::{button, column, container, pick_list, row, scrollable, text, vertical_slider, Space, text_input, Id, mouse_area, stack, shader};
use iced::{Background, Color, Element, Length, Task, Subscription, event, keyboard, advanced::input_method, Font, font::Weight, mouse};
use iced::widget::operation::focus;
use iced::window;
use std::collections::HashSet;
use std::env;
use std::path::Path;
mod terminal;
mod connection;
mod platform;
mod remote_display;
use connection::rdp_input_policy::{
    current_keyboard_indicators, is_remote_secure_attention_key,
    is_remote_secure_attention_shortcut, remote_secure_attention_inputs, route_key_pressed,
    route_key_released, unicode_inputs_for_text, RoutedKeyEvent,
};
use remote_display::RemoteDisplayState;
use terminal::{TerminalEmulator, TerminalView, Selection};
use tokio::sync::mpsc;

pub const D2CODING: iced::Font = iced::Font {
    family: iced::font::Family::Name("D2Coding"),
    ..iced::Font::DEFAULT
};

pub fn main() -> iced::Result {
    iced::application(
        || {
            let font_task = iced::font::load(include_bytes!("../assets/fonts/D2Coding.ttf")).map(Message::FontLoaded);
            let win_id_task = window::oldest().map(|opt_id| {
                Message::WindowIdCaptured(opt_id.expect("No window found"))
            });
            (State::default(), Task::batch(vec![font_task, win_id_task]))
        },
        update,
        view,
    )
    .window(window::Settings {
        decorations: false,
        ..Default::default()
    })
    .subscription(subscription)
    .title("k_term")
    .run()
}

// ---------- Session ----------

#[derive(Debug)]
enum SessionKind {
    Welcome,
    Terminal,
    RemoteDisplay,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolMode {
    Ssh,
    Telnet,
    Serial,
    Local,
    Rdp,
}

#[derive(Debug, Clone)]
struct LocalShellOption {
    name: String,
    program: String,
    args: Vec<String>,
}

fn resolve_executable(exe_name: &str) -> Option<String> {
    let candidate = Path::new(exe_name);
    if candidate.is_absolute() && candidate.exists() {
        return Some(exe_name.to_string());
    }

    if let Ok(path_var) = env::var("PATH") {
        for dir in env::split_paths(&path_var) {
            let full = dir.join(exe_name);
            if full.exists() {
                return Some(full.to_string_lossy().into_owned());
            }
        }
    }
    None
}

fn detect_local_shells() -> Vec<LocalShellOption> {
    let mut shells = Vec::new();
    let mut dedup = HashSet::new();

    let mut push_shell = |name: &str, program: String, args: Vec<String>| {
        let key = program.to_lowercase();
        if dedup.insert(key) {
            shells.push(LocalShellOption {
                name: name.to_string(),
                program,
                args,
            });
        }
    };

    if let Ok(comspec) = env::var("COMSPEC") {
        if Path::new(&comspec).exists() {
            push_shell("Command Prompt (COMSPEC)", comspec, vec![]);
        }
    }

    let candidates = [
        ("PowerShell 7", "pwsh.exe", vec!["-NoLogo", "-NoExit"]),
        ("Windows PowerShell", "powershell.exe", vec!["-NoLogo", "-NoExit"]),
        ("Command Prompt", "cmd.exe", vec![]),
        ("Bash", "bash.exe", vec!["--login", "-i"]),
    ];

    for (name, exe, args) in candidates {
        if let Some(program) = resolve_executable(exe) {
            push_shell(name, program, args.iter().map(|s| s.to_string()).collect());
        }
    }

    if shells.is_empty() {
        shells.push(LocalShellOption {
            name: "Windows PowerShell (fallback)".to_string(),
            program: "powershell.exe".to_string(),
            args: vec!["-NoLogo".to_string(), "-NoExit".to_string()],
        });
    }

    shells
}

struct Session {
    id: u64,
    name: String,
    kind: SessionKind,
    terminal: TerminalEmulator,
    remote_display: Option<RemoteDisplayState>,
    sender: Option<mpsc::UnboundedSender<connection::ConnectionInput>>,
    rdp_secure_attention_active: bool,
}

impl Session {
    fn welcome(id: u64) -> Self {
        Self {
            id,
            name: "Welcome".to_string(),
            kind: SessionKind::Welcome,
            terminal: TerminalEmulator::new(24, 80),
            remote_display: None,
            sender: None,
            rdp_secure_attention_active: false,
        }
    }

    fn new_terminal(id: u64, name: String, rows: usize, cols: usize) -> Self {
        Self {
            id,
            name,
            kind: SessionKind::Terminal,
            terminal: TerminalEmulator::new(rows, cols),
            remote_display: None,
            sender: None,
            rdp_secure_attention_active: false,
        }
    }

    fn new_remote_display(id: u64, name: String, width: u16, height: u16) -> Self {
        Self {
            id,
            name,
            kind: SessionKind::RemoteDisplay,
            terminal: TerminalEmulator::new(24, 80),
            remote_display: Some(RemoteDisplayState::new(width, height)),
            sender: None,
            rdp_secure_attention_active: false,
        }
    }
}

// ---------- State ----------

struct State {
    sessions: Vec<Session>,
    next_session_id: u64,
    active_index: usize,
    welcome_protocol: ProtocolMode,
    ssh_host: String,
    ssh_port: String,
    ssh_user: String,
    ssh_pass: String,
    serial_port: String,
    serial_baud: String,
    rdp_host: String,
    rdp_port: String,
    rdp_user: String,
    rdp_pass: String,
    local_shells: Vec<LocalShellOption>,
    selected_local_shell: usize,
    ssh_id_host: Id,
    ssh_id_port: Id,
    ssh_id_user: Id,
    ssh_id_pass: Id,
    telnet_id_host: Id,
    telnet_id_port: Id,
    serial_id_port: Id,
    serial_id_baud: Id,
    rdp_id_host: Id,
    rdp_id_port: Id,
    rdp_id_user: Id,
    rdp_id_pass: Id,
    focused_field: usize,
    pub window_id: Option<window::Id>,
    pub dummy_menu_open: Option<&'static str>,
    pub resizing_direction: Option<window::Direction>,
    pub window_size: (f32, f32),
}

impl Default for State {
    fn default() -> Self {
        let local_shells = detect_local_shells();
        Self {
            sessions: vec![Session::welcome(0)],
            next_session_id: 1,
            active_index: 0,
            welcome_protocol: ProtocolMode::Ssh,
            ssh_host: "".to_string(),
            ssh_port: "22".to_string(),
            ssh_user: "".to_string(),
            ssh_pass: "".to_string(),
            serial_port: "COM1".to_string(),
            serial_baud: "115200".to_string(),
            rdp_host: "".to_string(),
            rdp_port: "3389".to_string(),
            rdp_user: "".to_string(),
            rdp_pass: "".to_string(),
            local_shells,
            selected_local_shell: 0,
            ssh_id_host: Id::new("ssh_host"),
            ssh_id_port: Id::new("ssh_port"),
            ssh_id_user: Id::new("ssh_user"),
            ssh_id_pass: Id::new("ssh_pass"),
            telnet_id_host: Id::new("telnet_host"),
            telnet_id_port: Id::new("telnet_port"),
            serial_id_port: Id::new("serial_port"),
            serial_id_baud: Id::new("serial_baud"),
            rdp_id_host: Id::new("rdp_host"),
            rdp_id_port: Id::new("rdp_port"),
            rdp_id_user: Id::new("rdp_user"),
            rdp_id_pass: Id::new("rdp_pass"),
            focused_field: 0,
            window_id: None,
            dummy_menu_open: None,
            resizing_direction: None,
            window_size: (1024.0, 768.0),
        }
    }
}

impl State {
    fn current_field_ids(&self) -> Vec<Id> {
        match self.welcome_protocol {
            ProtocolMode::Ssh => vec![
                self.ssh_id_host.clone(),
                self.ssh_id_port.clone(),
                self.ssh_id_user.clone(),
                self.ssh_id_pass.clone(),
            ],
            ProtocolMode::Telnet => vec![
                self.telnet_id_host.clone(),
                self.telnet_id_port.clone(),
            ],
            ProtocolMode::Serial => vec![
                self.serial_id_port.clone(),
                self.serial_id_baud.clone(),
            ],
            ProtocolMode::Rdp => vec![
                self.rdp_id_host.clone(),
                self.rdp_id_port.clone(),
                self.rdp_id_user.clone(),
                self.rdp_id_pass.clone(),
            ],
            ProtocolMode::Local => vec![],
        }
    }
}

// ---------- Messages ----------

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(usize),
    CloseTab(usize),
    NewSshTab,
    TerminalInput(Vec<u8>),
    ImePreedit(String),
    ImeCommit(String),
    FontLoaded(Result<(), iced::font::Error>),
    ConnectionMessage(u64, connection::ConnectionEvent),
    TerminalResize(usize, usize),
    TerminalScroll(f32),
    TerminalScrollTo(usize),
    HostChanged(String),
    PortChanged(String),
    UserChanged(String),
    PassChanged(String),
    SerialPortChanged(String),
    SerialBaudChanged(String),
    RdpHostChanged(String),
    RdpPortChanged(String),
    RdpUserChanged(String),
    RdpPassChanged(String),
    SelectProtocol(ProtocolMode),
    SelectLocalShell(usize),
    ConnectSsh,
    ConnectTelnet,
    ConnectLocal,
    ConnectSerial,
    ConnectRdp,
    TabPressed(bool),
    FieldFocused(usize),
    WindowIdCaptured(window::Id),
    WindowDrag,
    WindowResize(window::Direction),
    ResizeFinished,
    MinimizeWindow,
    MaximizeWindow,
    CloseWindow,
    ToggleMenu(&'static str),
    CloseMenuDeferred,
    CloseMenu,
    OpenProtocolTab(ProtocolMode),
    SelectionStart(usize, usize),
    SelectionUpdate(usize, usize),
    CopyText(String),
    CopyCurrentSelection,
    PasteFromClipboard,
    PasteData(Option<String>),
    ClearSelection,
    TryHandleKey(keyboard::Key, keyboard::Modifiers),
    RemoteRdpInput(connection::RdpInput),
    RemoteRdpInputs(Vec<connection::RdpInput>),
    RemoteSecureAttention(bool),
    SyncRdpKeyboardIndicators,
    ReleaseRdpModifiers,
    WindowSizeChanged(f32, f32),
}

// ---------- Update ----------

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::TabSelected(index) => {
            if index < state.sessions.len() { state.active_index = index; }
            Task::none()
        }
        Message::CloseTab(index) => {
            if state.sessions.len() <= 1 { return Task::none(); }
            state.sessions.remove(index);
            if state.active_index >= state.sessions.len() { state.active_index = state.sessions.len() - 1; }
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
                if let Some(ref sender) = session.sender { let _ = sender.send(connection::ConnectionInput::Data(bytes)); }
                else { session.terminal.process_bytes(&bytes); }
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
                            let _ = sender.send(connection::ConnectionInput::RdpInput(input));
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
                match event {
                    connection::ConnectionEvent::Connected(sender) => {
                        session.sender = Some(sender.clone());
                        if matches!(session.kind, SessionKind::RemoteDisplay) {
                            let (win_w, win_h) = state.window_size;
                            let pixel_w = ((win_w - 181.0).max(200.0)) as u16;
                            let pixel_h = ((win_h - 66.0).max(200.0)) as u16;
                            let _ = sender.send(connection::ConnectionInput::Resize { cols: pixel_w, rows: pixel_h });
                            let _ = sender.send(connection::ConnectionInput::SyncKeyboardIndicators(current_keyboard_indicators()));
                        } else {
                            let _ = sender.send(connection::ConnectionInput::Resize { cols: session.terminal.cols as u16, rows: session.terminal.rows as u16 });
                        }
                    }
                    connection::ConnectionEvent::Data(data) => {
                        if !data.is_empty() {
                            session.terminal.process_bytes(&data);
                            session.terminal.cache.clear();
                            let responses: Vec<Vec<u8>> = session.terminal.pending_responses.drain(..).collect();
                            for resp in responses {
                                if let Some(ref sender) = session.sender { let _ = sender.send(connection::ConnectionInput::Data(resp)); }
                            }
                        }
                    }
                    connection::ConnectionEvent::Frames(frames) => {
                        if session.remote_display.is_none() {
                            session.remote_display = Some(RemoteDisplayState::new(1280, 720));
                        }
                        if let Some(display) = session.remote_display.as_mut() {
                            display.status_message = None;
                            for frame in frames {
                                display.apply(frame);
                            }
                            // No flush needed — shader widget reads state directly in view()
                        }
                    }
                    connection::ConnectionEvent::Disconnected => {
                        session.sender = None;
                        if let Some(display) = session.remote_display.as_mut() {
                            display.status_message = Some("Disconnected".to_string());
                        } else {
                            session.terminal.process_bytes(b"\r\n\x1b[31m[Disconnected]\x1b[0m\r\n");
                            session.terminal.cache.clear();
                        }
                    }
                    connection::ConnectionEvent::Error(e) => {
                        session.sender = None;
                        eprintln!("[RDP Error] {}", e);
                        if let Some(display) = session.remote_display.as_mut() {
                            display.status_message = Some(format!("Error: {}", e));
                        } else {
                            let msg = format!("\r\n\x1b[31m[Error: {}]\x1b[0m\r\n", e);
                            session.terminal.process_bytes(msg.as_bytes());
                            session.terminal.cache.clear();
                        }
                    }
                }
            }
            Task::none()
        }
        Message::TerminalResize(new_rows, new_cols) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if session.terminal.rows != new_rows || session.terminal.cols != new_cols {
                    session.terminal.resize(new_rows, new_cols);
                    if let Some(ref sender) = session.sender { let _ = sender.send(connection::ConnectionInput::Resize { cols: new_cols as u16, rows: new_rows as u16 }); }
                }
            }
            Task::none()
        }
        Message::TerminalScroll(delta) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                let max_scroll = session.terminal.history.len();
                if delta > 0.0 { session.terminal.display_offset = std::cmp::min(session.terminal.display_offset + 3, max_scroll); }
                else { session.terminal.display_offset = session.terminal.display_offset.saturating_sub(3); }
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
        Message::HostChanged(s) => { state.ssh_host = s; Task::none() }
        Message::PortChanged(s) => { state.ssh_port = s; Task::none() }
        Message::UserChanged(s) => { state.ssh_user = s; Task::none() }
        Message::PassChanged(s) => { state.ssh_pass = s; Task::none() }
        Message::SerialPortChanged(s) => { state.serial_port = s; Task::none() }
        Message::SerialBaudChanged(s) => { state.serial_baud = s; Task::none() }
        Message::RdpHostChanged(s) => { state.rdp_host = s; Task::none() }
        Message::RdpPortChanged(s) => { state.rdp_port = s; Task::none() }
        Message::RdpUserChanged(s) => { state.rdp_user = s; Task::none() }
        Message::RdpPassChanged(s) => { state.rdp_pass = s; Task::none() }
        Message::SelectProtocol(mode) => {
            state.welcome_protocol = mode;
            state.focused_field = 0;
            let fields = state.current_field_ids();
            if let Some(first) = fields.first() { focus(first.clone()) } else { Task::none() }
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
            let name = format!("SSH: {}@{}", user, host);
            let target_index = state.active_index;
            let mut target_id = None;
            if let Some(session) = state.sessions.get_mut(target_index) {
                target_id = Some(session.id);
                *session = Session::new_terminal(session.id, name, session.terminal.rows, session.terminal.cols);
            }
            if let Some(target_id) = target_id {
                Task::run(connection::ssh::connect_and_subscribe(host, port, user, pass), move |event| Message::ConnectionMessage(target_id, event))
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
                *session = Session::new_terminal(session.id, name, session.terminal.rows, session.terminal.cols);
            }
            if let Some(target_id) = target_id {
                Task::run(connection::telnet::connect_and_subscribe(host, port), move |event| Message::ConnectionMessage(target_id, event))
            } else {
                Task::none()
            }
        }
        Message::ConnectSerial => {
            let port_name = state.serial_port.clone();
            let baud: u32 = state.serial_baud.parse().unwrap_or(115200);
            let name = format!("Serial: {} ({}bps)", port_name, baud);
            let target_index = state.active_index;
            let mut target_id = None;
            if let Some(session) = state.sessions.get_mut(target_index) {
                target_id = Some(session.id);
                *session = Session::new_terminal(session.id, name, session.terminal.rows, session.terminal.cols);
            }
            if let Some(target_id) = target_id {
                Task::run(connection::serial::connect_and_subscribe(port_name, baud), move |event| Message::ConnectionMessage(target_id, event))
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
                *session = Session::new_terminal(session.id, name, session.terminal.rows, session.terminal.cols);
            }
            if let Some(target_id) = target_id {
                Task::run(platform::windows::spawn_local_shell(shell.program, shell.args), move |event| Message::ConnectionMessage(target_id, event))
            } else {
                Task::none()
            }
        }
        Message::ConnectRdp => {
            let host = state.rdp_host.clone();
            let port: u16 = state.rdp_port.parse().unwrap_or(3389);
            let user = state.rdp_user.clone();
            let pass = state.rdp_pass.clone();
            let name = format!("RDP: {}@{}:{}", user, host, port);

            let target_index = state.active_index;
            let mut target_id = None;
            if let Some(session) = state.sessions.get_mut(target_index) {
                target_id = Some(session.id);
                *session = Session::new_remote_display(session.id, name, 1280, 720);
            }

            if let Some(target_id) = target_id {
                Task::run(
                    connection::rdp::connect_and_subscribe(host, port, user, pass),
                    move |event| Message::ConnectionMessage(target_id, event),
                )
            } else {
                Task::none()
            }
        }

        Message::SelectionStart(col, row) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                session.terminal.selection = Some(Selection { start: (col, row), end: (col, row) });
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
        Message::PasteFromClipboard => {
            iced::clipboard::read().map(Message::PasteData)
        }
        Message::PasteData(text_opt) => {
            if let Some(text) = text_opt {
                if let Some(session) = state.sessions.get_mut(state.active_index) {
                    let filtered_text: String = text.chars().filter(|&c| c != '\0').collect();
                    let bytes = filtered_text.into_bytes();
                    if let Some(ref sender) = session.sender { let _ = sender.send(connection::ConnectionInput::Data(bytes)); }
                    else { session.terminal.process_bytes(&bytes); }
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

                // Ctrl+C
                if ctrl && matches!(key, keyboard::Key::Character(ref c) if c == "c" || c == "C") {
                    if has_sel {
                        let text = session.terminal.get_selected_text();
                        return Task::done(Message::CopyText(text));
                    } else {
                        return Task::done(Message::TerminalInput(vec![3]));
                    }
                }

                // Ctrl+V
                if ctrl && matches!(key, keyboard::Key::Character(ref c) if c == "v" || c == "V") {
                    return Task::done(Message::PasteFromClipboard);
                }

                // ESC
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
        Message::RemoteRdpInput(input) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if let Some(ref sender) = session.sender {
                    let input = transform_rdp_mouse(input, state.window_size, session.remote_display.as_ref());
                    let _ = sender.send(connection::ConnectionInput::RdpInput(input));
                }
            }
            Task::none()
        }
        Message::RemoteRdpInputs(inputs) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if let Some(ref sender) = session.sender {
                    for input in inputs {
                        let input = transform_rdp_mouse(input, state.window_size, session.remote_display.as_ref());
                        let _ = sender.send(connection::ConnectionInput::RdpInput(input));
                    }
                }
            }
            Task::none()
        }
        Message::RemoteSecureAttention(active) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                session.rdp_secure_attention_active = active;
                if let Some(ref sender) = session.sender {
                    for input in remote_secure_attention_inputs(active) {
                        let _ = sender.send(connection::ConnectionInput::RdpInput(input));
                    }
                }
            }
            Task::none()
        }
        Message::SyncRdpKeyboardIndicators => {
            if let Some(session) = state.sessions.get(state.active_index) {
                if matches!(session.kind, SessionKind::RemoteDisplay) {
                    if let Some(ref sender) = session.sender {
                        let _ = sender.send(connection::ConnectionInput::SyncKeyboardIndicators(current_keyboard_indicators()));
                    }
                }
            }
            Task::none()
        }
        Message::ReleaseRdpModifiers => {
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
            // Resize active RemoteDisplay session
            if let Some(session) = state.sessions.get(state.active_index) {
                if matches!(session.kind, SessionKind::RemoteDisplay) {
                    if let Some(ref sender) = session.sender {
                        let pixel_w = ((w - 181.0).max(200.0)) as u16;
                        let pixel_h = ((h - 66.0).max(200.0)) as u16;
                        let _ = sender.send(connection::ConnectionInput::Resize { cols: pixel_w, rows: pixel_h });
                    }
                }
            }
            Task::none()
        }
        Message::TabPressed(shift) => {
            let fields = state.current_field_ids();
            if fields.is_empty() { return Task::none(); }
            let count = fields.len();
            if shift { state.focused_field = if state.focused_field == 0 { count - 1 } else { state.focused_field - 1 }; }
            else { state.focused_field = (state.focused_field + 1) % count; }
            focus(fields[state.focused_field].clone())
        }
        Message::FieldFocused(index) => { state.focused_field = index; Task::none() }
        Message::WindowIdCaptured(id) => { if state.window_id.is_none() { state.window_id = Some(id); } Task::none() }
        Message::WindowDrag => { if let Some(id) = state.window_id { window::drag(id) } else { Task::none() } }
        Message::WindowResize(direction) => {
            state.resizing_direction = Some(direction);
            if let Some(id) = state.window_id { window::drag_resize(id, direction) } else { Task::none() }
        }
        Message::ResizeFinished => {
            state.resizing_direction = None;
            Task::none()
        }
        Message::MinimizeWindow => { if let Some(id) = state.window_id { window::minimize(id, true) } else { Task::none() } }
        Message::MaximizeWindow => { if let Some(id) = state.window_id { window::toggle_maximize(id) } else { Task::none() } }
        Message::CloseWindow => { if let Some(id) = state.window_id { window::close(id) } else { Task::none() } }
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
        Message::CloseMenuDeferred => {
            Task::perform(async {}, |_| Message::CloseMenu)
        }
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
    }
}

fn subscription(state: &State) -> Subscription<Message> {
    let active_kind = state.sessions.get(state.active_index).map(|s| &s.kind);
    let is_welcome = matches!(active_kind, Some(SessionKind::Welcome));
    let is_terminal = matches!(active_kind, Some(SessionKind::Terminal));
    
    let mouse_sub = event::listen_with(|event, _status, _window| {
        match event {
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Some(Message::ResizeFinished),
            iced::Event::Window(window::Event::Resized(size)) => Some(Message::WindowSizeChanged(size.width, size.height)),
            iced::Event::Window(window::Event::Focused) => Some(Message::SyncRdpKeyboardIndicators),
            iced::Event::Window(window::Event::Unfocused) => Some(Message::ReleaseRdpModifiers),
            _ => None
        }
    });

    let tab_sub = event::listen_with(|event, _status, _window| {
        match event {
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                if key == keyboard::Key::Named(keyboard::key::Named::Tab) { Some(Message::TabPressed(modifiers.shift())) }
                else if key == keyboard::Key::Named(keyboard::key::Named::Escape) { Some(Message::TabPressed(false)) }
                else { None }
            }
            _ => None
        }
    });

    let menu_close_sub = if matches!(state.dummy_menu_open, Some(menu) if !menu.is_empty()) {
        event::listen_with(|event, _status, _window| {
            match event {
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => Some(Message::CloseMenuDeferred),
                _ => None,
            }
        })
    } else {
        Subscription::none()
    };

    if is_welcome {
        let mut subs = vec![tab_sub, mouse_sub, menu_close_sub];
        if state.window_id.is_none() { subs.push(window::open_events().map(Message::WindowIdCaptured)); }
        Subscription::batch(subs)
    } else if is_terminal {
        let term_sub = event::listen_with(|event, _status, _window| {
            match event {
                iced::Event::InputMethod(ime) => {
                    match ime {
                        input_method::Event::Preedit(text, _) => Some(Message::ImePreedit(text)),
                        input_method::Event::Commit(text) => Some(Message::ImeCommit(text)),
                        _ => None,
                    }
                }
                iced::Event::Keyboard(keyboard::Event::KeyPressed { key, text: key_text, location, modifiers, .. }) => {
                    let ctrl = modifiers.control();
                    
                    // Priority shortcuts
                    if ctrl && matches!(key, keyboard::Key::Character(ref c) if c == "c" || c == "C" || c == "v" || c == "V") {
                        return Some(Message::TryHandleKey(key.clone(), modifiers));
                    }
                    if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                        return Some(Message::TryHandleKey(key.clone(), modifiers));
                    }

                    let mut bytes = Vec::new();
                    let is_numpad = matches!(location, keyboard::Location::Numpad);
                    let numpad_text = if is_numpad { key_text.as_deref().filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || ".-+*/".contains(c))) } else { None };
                    if let Some(s) = numpad_text { bytes.extend_from_slice(s.as_bytes()); }
                    else {
                        match &key {
                            keyboard::Key::Named(keyboard::key::Named::Enter) => bytes.extend_from_slice(b"\r"),
                            keyboard::Key::Named(keyboard::key::Named::Backspace) => bytes.push(b'\x7f'),
                            keyboard::Key::Named(keyboard::key::Named::Tab) => bytes.extend_from_slice(b"\t"),
                            keyboard::Key::Named(keyboard::key::Named::ArrowUp) => bytes.extend_from_slice(b"\x1b[A"),
                            keyboard::Key::Named(keyboard::key::Named::ArrowDown) => bytes.extend_from_slice(b"\x1b[B"),
                            keyboard::Key::Named(keyboard::key::Named::ArrowRight) => bytes.extend_from_slice(b"\x1b[C"),
                            keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => bytes.extend_from_slice(b"\x1b[D"),
                            keyboard::Key::Named(keyboard::key::Named::Home) => bytes.extend_from_slice(b"\x1b[H"),
                            keyboard::Key::Named(keyboard::key::Named::End) => bytes.extend_from_slice(b"\x1b[F"),
                            keyboard::Key::Named(keyboard::key::Named::Delete) => bytes.extend_from_slice(b"\x1b[3~"),
                            keyboard::Key::Named(keyboard::key::Named::PageUp) => bytes.extend_from_slice(b"\x1b[5~"),
                            keyboard::Key::Named(keyboard::key::Named::PageDown) => bytes.extend_from_slice(b"\x1b[6~"),
                            keyboard::Key::Named(keyboard::key::Named::Escape) => bytes.extend_from_slice(b"\x1b"),
                            _ => {}
                        }
                        if bytes.is_empty() { if let Some(t) = key_text { let s = t.as_str(); if s.is_ascii() { bytes.extend_from_slice(s.as_bytes()); } } }
                    }
                    if !bytes.is_empty() { Some(Message::TerminalInput(bytes)) } else { None }
                }
                _ => None
            }
        });
        let mut subs = vec![term_sub, mouse_sub, menu_close_sub];
        if state.window_id.is_none() { subs.push(window::open_events().map(Message::WindowIdCaptured)); }
        Subscription::batch(subs)
    } else {
        let remote_sub = event::listen_with(|event, status, _window| {
            match event {
                iced::Event::InputMethod(ime) => match ime {
                    input_method::Event::Commit(text) => Some(Message::ImeCommit(text)),
                    _ => None,
                },
                iced::Event::Keyboard(keyboard::Event::KeyPressed { key, text, physical_key, modifiers, .. })
                    if status == event::Status::Ignored =>
                {
                    if is_remote_secure_attention_shortcut(&physical_key, modifiers) {
                        return Some(Message::RemoteSecureAttention(true));
                    }

                    match route_key_pressed(&key, text.as_deref(), &physical_key) {
                        RoutedKeyEvent::Ignore => None,
                        RoutedKeyEvent::SyncIndicators => {
                            Some(Message::SyncRdpKeyboardIndicators)
                        }
                        RoutedKeyEvent::Input(input) => Some(Message::RemoteRdpInput(input)),
                    }
                }
                iced::Event::Keyboard(keyboard::Event::KeyReleased { key, physical_key, modifiers, .. })
                    if status == event::Status::Ignored =>
                {
                    if is_remote_secure_attention_shortcut(&physical_key, modifiers)
                        || (is_remote_secure_attention_key(&physical_key) && modifiers.control() && modifiers.alt())
                    {
                        return Some(Message::RemoteSecureAttention(false));
                    }

                    match route_key_released(&key, &physical_key) {
                        RoutedKeyEvent::Ignore => None,
                        RoutedKeyEvent::SyncIndicators => {
                            Some(Message::SyncRdpKeyboardIndicators)
                        }
                        RoutedKeyEvent::Input(input) => Some(Message::RemoteRdpInput(input)),
                    }
                }
                iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Message::RemoteRdpInput(connection::RdpInput::MouseMove {
                        x: position.x.max(0.0).min(u16::MAX as f32) as u16,
                        y: position.y.max(0.0).min(u16::MAX as f32) as u16,
                    }))
                }
                iced::Event::Mouse(mouse::Event::ButtonPressed(button))
                    if status == event::Status::Ignored =>
                {
                    match button {
                        mouse::Button::Left => Some(Message::RemoteRdpInput(connection::RdpInput::MouseButton { button: connection::RdpMouseButton::Left, down: true })),
                        mouse::Button::Right => Some(Message::RemoteRdpInput(connection::RdpInput::MouseButton { button: connection::RdpMouseButton::Right, down: true })),
                        mouse::Button::Middle => Some(Message::RemoteRdpInput(connection::RdpInput::MouseButton { button: connection::RdpMouseButton::Middle, down: true })),
                        _ => None,
                    }
                }
                iced::Event::Mouse(mouse::Event::ButtonReleased(button))
                    if status == event::Status::Ignored =>
                {
                    match button {
                        mouse::Button::Left => Some(Message::RemoteRdpInput(connection::RdpInput::MouseButton { button: connection::RdpMouseButton::Left, down: false })),
                        mouse::Button::Right => Some(Message::RemoteRdpInput(connection::RdpInput::MouseButton { button: connection::RdpMouseButton::Right, down: false })),
                        mouse::Button::Middle => Some(Message::RemoteRdpInput(connection::RdpInput::MouseButton { button: connection::RdpMouseButton::Middle, down: false })),
                        _ => None,
                    }
                }
                iced::Event::Mouse(mouse::Event::WheelScrolled { delta })
                    if status == event::Status::Ignored =>
                {
                    let (hx, vy) = match delta {
                        mouse::ScrollDelta::Lines { x, y } => (x, y),
                        mouse::ScrollDelta::Pixels { x, y } => (x / 40.0, y / 40.0),
                    };
                    let vy_step = (vy * 120.0).round();
                    let hx_step = (hx * 120.0).round();
                    if vy_step != 0.0 {
                        Some(Message::RemoteRdpInput(connection::RdpInput::MouseWheel {
                            delta: vy_step.max(i16::MIN as f32).min(i16::MAX as f32) as i16,
                        }))
                    } else if hx_step != 0.0 {
                        Some(Message::RemoteRdpInput(connection::RdpInput::MouseHorizontalWheel {
                            delta: hx_step.max(i16::MIN as f32).min(i16::MAX as f32) as i16,
                        }))
                    } else {
                        None
                    }
                }
                _ => None,
            }
        });

        let mut subs = vec![mouse_sub, menu_close_sub, remote_sub];
        if state.window_id.is_none() { subs.push(window::open_events().map(Message::WindowIdCaptured)); }
        Subscription::batch(subs)
    }
}

fn transform_rdp_mouse(
    input: connection::RdpInput,
    window_size: (f32, f32),
    display: Option<&RemoteDisplayState>,
) -> connection::RdpInput {
    match input {
        connection::RdpInput::MouseMove { x, y } => {
            const CONTENT_X: f32 = 181.0; // sidebar(180) + vr(1)
            const CONTENT_Y: f32 = 66.0;  // title_bar(35) + tab_bar(30) + hr(1)

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

                let rdp_x = ((rel_x - offset_x) / rendered_w * desk_w).clamp(0.0, desk_w - 1.0) as u16;
                let rdp_y = ((rel_y - offset_y) / rendered_h * desk_h).clamp(0.0, desk_h - 1.0) as u16;
                connection::RdpInput::MouseMove { x: rdp_x, y: rdp_y }
            } else {
                connection::RdpInput::MouseMove { x: rel_x as u16, y: rel_y as u16 }
            }
        }
        other => other,
    }
}

fn hr() -> Element<'static, Message> {
    container(Space::new().width(Length::Fill).height(Length::Fixed(1.0))).style(|_| iced::widget::container::Style { background: Some(Background::Color(Color::from_rgb(0.5, 0.5, 0.5))), ..Default::default() }).into()
}

fn vr() -> Element<'static, Message> {
    container(Space::new().width(Length::Fixed(1.0)).height(Length::Fill)).style(|_| iced::widget::container::Style { background: Some(Background::Color(Color::from_rgb(0.5, 0.5, 0.5))), ..Default::default() }).into()
}

fn view(state: &State) -> Element<'_, Message> {
    let active_session_name = state.sessions.get(state.active_index).map(|s| s.name.clone()).unwrap_or_else(|| "kterm".to_string());
    let menu_bar = row![
        button(text("Session ▾").size(12)).padding([4, 8]).style(button::text).on_press(Message::ToggleMenu("Session")),
        button(text("Settings ▾").size(12)).padding([4, 8]).style(button::text).on_press(Message::ToggleMenu("Settings")),
        button(text("View ▾").size(12)).padding([4, 8]).style(button::text).on_press(Message::ToggleMenu("View")),
        button(text("Help ▾").size(12)).padding([4, 8]).style(button::text).on_press(Message::ToggleMenu("Help")),
    ].spacing(2).align_y(iced::Alignment::Center);

    let title_bar = container(
        row![
            container(text(" ◈ kterm").size(14).font(Font { weight: Weight::Bold, ..Default::default() })).padding([0, 15]).center_y(Length::Fill),
            menu_bar,
            mouse_area(container(text(active_session_name).size(12)).width(Length::Fill).center_x(Length::Fill).center_y(Length::Fill))
                .on_press(Message::WindowDrag).on_release(Message::CloseMenu),
            row![
                button(container(text("—").size(12)).center_x(Length::Fill).center_y(Length::Fill)).width(Length::Fixed(46.0)).height(Length::Fill).style(button::text).on_press(Message::MinimizeWindow),
                button(container(text("▢").size(14)).center_x(Length::Fill).center_y(Length::Fill)).width(Length::Fixed(46.0)).height(Length::Fill).style(button::text).on_press(Message::MaximizeWindow),
                button(container(text("✕").size(14)).center_x(Length::Fill).center_y(Length::Fill)).width(Length::Fixed(46.0)).height(Length::Fill).style(|t, s| {
                    let mut style = button::text(t, s);
                    if matches!(s, button::Status::Hovered) { style.background = Some(Background::Color(Color::from_rgb(0.7, 0.15, 0.15))); }
                    style
                }).on_press(Message::CloseWindow),
            ].height(Length::Fill)
        ].height(Length::Fixed(35.0)).align_y(iced::Alignment::Center)
    ).style(|_| container::Style { background: Some(Background::Color(Color::from_rgb(0.12, 0.12, 0.12))), ..Default::default() });

    let mut tab_bar = row![].spacing(0).padding(0);
    for (i, session) in state.sessions.iter().enumerate() {
        let is_active = i == state.active_index;
        let tab_height = 30.0;
        let tab_bg = if is_active { Color::from_rgb(0.08, 0.08, 0.08) } else { Color::from_rgb(0.12, 0.12, 0.12) };
        let border_color = if is_active { Color::from_rgb(0.35, 0.35, 0.35) } else { Color::from_rgb(0.22, 0.22, 0.22) };

        // 내부 버튼은 완전 투명 → 부모 컨테이너가 배경/외곽선 일괄 담당
        let label_btn = button(
            container(text(session.name.clone()).size(12))
                .height(Length::Fill)
                .center_y(Length::Fill)
        )
            .height(Length::Fixed(tab_height))
            .padding([0, 12])
            .style(move |_t, _s| {
                button::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    text_color: if is_active { Color::WHITE } else { Color::from_rgb(0.6, 0.6, 0.6) },
                    ..Default::default()
                }
            }).on_press(Message::TabSelected(i));

        let tab_item: Element<'_, Message> = if state.sessions.len() > 1 {
            let close_btn = button(
                container(text("×").size(13))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
            )
                .width(Length::Fixed(24.0))
                .height(Length::Fixed(tab_height))
                .padding(0)
                .style(move |_t, s| {
                    button::Style {
                        background: Some(Background::Color(Color::TRANSPARENT)),
                        text_color: if matches!(s, button::Status::Hovered) { Color::from_rgb(0.9, 0.4, 0.4) } else { Color::from_rgb(0.45, 0.45, 0.45) },
                        ..Default::default()
                    }
                }).on_press(Message::CloseTab(i));

            container(row![label_btn, close_btn].height(Length::Fixed(tab_height)).align_y(iced::Alignment::Center))
                .style(move |_| container::Style {
                    background: Some(Background::Color(tab_bg)),
                    border: iced::Border { radius: iced::border::Radius { top_left: 6.0, top_right: 6.0, ..Default::default() }, width: 1.0, color: border_color },
                    ..Default::default()
                }).into()
        } else {
            container(label_btn)
                .style(move |_| container::Style {
                    background: Some(Background::Color(tab_bg)),
                    border: iced::Border { radius: iced::border::Radius { top_left: 6.0, top_right: 6.0, ..Default::default() }, width: 1.0, color: border_color },
                    ..Default::default()
                }).into()
        };

        tab_bar = tab_bar.push(tab_item);
    }
    tab_bar = tab_bar.push(button(text("+").size(14)).padding([4, 8]).style(|_t, s| {
        let mut style = button::text(_t, s);
        if matches!(s, button::Status::Hovered) { style.background = Some(Background::Color(Color::from_rgb(0.2, 0.2, 0.2))); }
        style.text_color = Color::from_rgb(0.6, 0.6, 0.6);
        style.border.radius = 4.0.into();
        style
    }).on_press(Message::NewSshTab));

    let sidebar = container(column![text("SESSIONS").size(12).font(Font { weight: Weight::Bold, ..Default::default() }), hr(), scrollable(column![button(text("+ New SSH").size(13)).width(Length::Fill).style(button::secondary).on_press(Message::NewSshTab)].spacing(8)).height(Length::Fill)].spacing(10)).padding(10).width(Length::Fixed(180.0)).style(|_| container::Style { background: Some(Background::Color(Color::from_rgb(0.1, 0.1, 0.1))), ..Default::default() });

    let tab_content: Element<'_, Message> = if let Some(session) = state.sessions.get(state.active_index) {
        match session.kind {
            SessionKind::Welcome => {
                let protocol_btn = |mode: ProtocolMode, label: &str, current: &ProtocolMode| {
                    let is_active = mode == *current;
                    button(container(text(label.to_string()).size(15)).center_x(Length::Fill))
                        .width(Length::Fixed(110.0))
                        .padding([8, 0])
                        .style(move |_t, s| {
                            let mut st = button::secondary(_t, s);
                            if is_active {
                                st.background = Some(Background::Color(Color::from_rgb(0.2, 0.4, 0.6)));
                                st.text_color = Color::WHITE;
                            } else {
                                st.background = Some(Background::Color(if matches!(s, button::Status::Hovered) { Color::from_rgb(0.18, 0.18, 0.18) } else { Color::from_rgb(0.12, 0.12, 0.12) }));
                                st.text_color = Color::from_rgb(0.7, 0.7, 0.7);
                            }
                            st.border.radius = 4.0.into();
                            st
                        })
                        .on_press(Message::SelectProtocol(mode.clone()))
                };

                let protocol_tabs = row![
                    protocol_btn(ProtocolMode::Ssh, "SSH", &state.welcome_protocol),
                    protocol_btn(ProtocolMode::Telnet, "Telnet", &state.welcome_protocol),
                    protocol_btn(ProtocolMode::Serial, "Serial", &state.welcome_protocol),
                    protocol_btn(ProtocolMode::Local, "Local Shell", &state.welcome_protocol),
                    protocol_btn(ProtocolMode::Rdp, "RDP", &state.welcome_protocol),
                ].spacing(10);

                let form_content: Element<'_, Message> = match state.welcome_protocol {
                    ProtocolMode::Ssh => column![
                        text("SSH Connection").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        row![text("Host: ").width(100), text_input("IP Address", &state.ssh_host).id(state.ssh_id_host.clone()).on_input(Message::HostChanged).width(300)],
                        row![text("Port: ").width(100), text_input("22", &state.ssh_port).id(state.ssh_id_port.clone()).on_input(Message::PortChanged).width(150)],
                        row![text("Username: ").width(100), text_input("user", &state.ssh_user).id(state.ssh_id_user.clone()).on_input(Message::UserChanged).width(300)],
                        row![text("Password: ").width(100), text_input("pass", &state.ssh_pass).id(state.ssh_id_pass.clone()).on_input(Message::PassChanged).secure(true).width(300).on_submit(Message::ConnectSsh)],
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Connect")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectSsh)
                    ].spacing(15).into(),
                    ProtocolMode::Telnet => column![
                        text("Telnet Connection").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        row![text("Host: ").width(100), text_input("IP Address", &state.ssh_host).id(state.telnet_id_host.clone()).on_input(Message::HostChanged).width(300)],
                        row![text("Port: ").width(100), text_input("23", &state.ssh_port).id(state.telnet_id_port.clone()).on_input(Message::PortChanged).width(150).on_submit(Message::ConnectTelnet)],
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Connect")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectTelnet)
                    ].spacing(15).into(),
                    ProtocolMode::Serial => column![
                        text("Serial Connection").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        row![text("COM Port: ").width(100), text_input("COM1", &state.serial_port).id(state.serial_id_port.clone()).on_input(Message::SerialPortChanged).width(300)],
                        row![text("Baud Rate: ").width(100), text_input("115200", &state.serial_baud).id(state.serial_id_baud.clone()).on_input(Message::SerialBaudChanged).width(150).on_submit(Message::ConnectSerial)],
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Connect")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectSerial)
                    ].spacing(15).into(),
                    ProtocolMode::Local => column![
                        text("Local System").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        text("Detected shells on this system").size(14),
                        {
                            let options: Vec<String> = state.local_shells.iter().map(|s| s.name.clone()).collect();
                            let selected = state.local_shells.get(state.selected_local_shell).map(|s| s.name.clone());

                            pick_list(options, selected, |name| {
                                let index = state
                                    .local_shells
                                    .iter()
                                    .position(|s| s.name == name)
                                    .unwrap_or(0);
                                Message::SelectLocalShell(index)
                            })
                            .width(Length::Fill)
                            .padding([8, 12])
                        },
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Launch Selected Shell")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectLocal)
                    ].spacing(15).into(),
                    ProtocolMode::Rdp => column![
                        text("RDP Connection").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        row![text("Host: ").width(100), text_input("IP Address", &state.rdp_host).id(state.rdp_id_host.clone()).on_input(Message::RdpHostChanged).width(300)],
                        row![text("Port: ").width(100), text_input("3389", &state.rdp_port).id(state.rdp_id_port.clone()).on_input(Message::RdpPortChanged).width(150)],
                        row![text("Username: ").width(100), text_input("user", &state.rdp_user).id(state.rdp_id_user.clone()).on_input(Message::RdpUserChanged).width(300)],
                        row![text("Password: ").width(100), text_input("pass", &state.rdp_pass).id(state.rdp_id_pass.clone()).on_input(Message::RdpPassChanged).secure(true).width(300).on_submit(Message::ConnectRdp)],
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Connect")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectRdp)
                    ].spacing(15).into(),
                };

                let card_container = container(form_content)
                    .width(Length::Fixed(500.0))
                    .padding(30)
                    .style(|_| container::Style {
                        background: Some(Background::Color(Color::from_rgb(0.14, 0.14, 0.14))),
                        border: iced::Border { radius: 8.0.into(), width: 1.0, color: Color::from_rgb(0.25, 0.25, 0.25) },
                        ..Default::default()
                    });

                container(scrollable(
                    column![
                        text("Start a new session").size(28).font(Font { weight: Weight::Bold, ..Default::default() }),
                        Space::new().height(Length::Fixed(30.0)),
                        protocol_tabs,
                        Space::new().height(Length::Fixed(15.0)),
                        card_container,
                    ].align_x(iced::alignment::Horizontal::Center)
                )).center(Length::Fill).into()
            }
            SessionKind::Terminal => {
                let hist_len = session.terminal.history.len();
                let offset = session.terminal.display_offset;
                row![
                    container(TerminalView::new(
                        &session.terminal,
                        Message::TerminalScroll,
                        Message::TerminalResize,
                        Message::SelectionStart,
                        Message::SelectionUpdate,
                        || Message::CopyCurrentSelection,
                    ))
                    .width(Length::Fill)
                    .height(Length::Fill),
                    container(vertical_slider(0.0..=(hist_len as f32).max(1.0), offset as f32, |v| Message::TerminalScrollTo(v as usize)).step(1.0).style(|_, _| iced::widget::slider::Style { rail: iced::widget::slider::Rail { backgrounds: (iced::Background::Color(Color::from_rgb(0.1, 0.1, 0.1)), iced::Background::Color(Color::from_rgb(0.1, 0.1, 0.1))), width: 4.0, border: Default::default() }, handle: iced::widget::slider::Handle { shape: iced::widget::slider::HandleShape::Rectangle { width: 10, border_radius: 2.0f32.into() }, background: iced::Background::Color(Color::from_rgb(0.4, 0.4, 0.4)), border_width: 0.0, border_color: Color::TRANSPARENT } })).width(Length::Fixed(12.0)).height(Length::Fill).padding(2)
                ].into()
            }
            SessionKind::RemoteDisplay => {
                if let Some(display) = &session.remote_display {
                    if let Some(ref msg) = display.status_message {
                        scrollable(
                            container(
                                text(msg.clone()).size(12).color(Color::from_rgb(0.0, 1.0, 0.4))
                            )
                            .padding(10)
                            .width(Length::Fill)
                        )
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                    } else if display.width > 0 {
                        let program = remote_display::renderer::RdpDisplayProgram {
                            frame: std::sync::Arc::clone(&display.rgba),
                            tex_width: display.width as u32,
                            tex_height: display.height as u32,
                            dirty_rects: Vec::new(),
                            full_upload: true,
                        };
                        container(
                            shader(program)
                                .width(Length::Fill)
                                .height(Length::Fill)
                        )
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                    } else {
                        container(
                            column![
                                text("RDP session connected"),
                                text("Waiting for first frame...").size(14),
                            ]
                            .spacing(10)
                        )
                        .center(Length::Fill)
                        .into()
                    }
                } else {
                    container(text("Remote display state is not initialized"))
                        .center(Length::Fill)
                        .into()
                }
            }
        }
    } else { text("No active tab").into() };

    let main_content = column![row![tab_bar, Space::new().width(Length::Fill)].align_y(iced::Alignment::Center), hr(), container(tab_content).width(Length::Fill).height(Length::Fill)].width(Length::Fill).height(Length::Fill);
    let body = row![sidebar, vr(), main_content].height(Length::Fill);
    let base_layout: Element<'_, Message> = column![title_bar, body].into();

    // --- Overlay & Resize Handles (Transparent) ---
    let resize_handle = |dir: window::Direction, w: Length, h: Length| {
        let interaction = match dir {
            window::Direction::North | window::Direction::South => mouse::Interaction::ResizingVertically,
            window::Direction::West | window::Direction::East => mouse::Interaction::ResizingHorizontally,
            window::Direction::NorthWest | window::Direction::SouthEast => mouse::Interaction::ResizingDiagonallyDown,
            window::Direction::NorthEast | window::Direction::SouthWest => mouse::Interaction::ResizingDiagonallyUp,
        };
        mouse_area(container(Space::new()).width(w).height(h).style(|_| container::Style { background: Some(Background::Color(Color::TRANSPARENT)), ..Default::default() }))
            .on_press(Message::WindowResize(dir)).interaction(interaction)
    };

    let content_with_resize = stack![
        container(base_layout).width(Length::Fill).height(Length::Fill),
        container(resize_handle(window::Direction::North, Length::Fill, Length::Fixed(8.0))).width(Length::Fill).height(Length::Fill).padding([0, 20]).align_y(iced::alignment::Vertical::Top),
        container(resize_handle(window::Direction::South, Length::Fill, Length::Fixed(8.0))).width(Length::Fill).height(Length::Fill).padding([0, 20]).align_y(iced::alignment::Vertical::Bottom),
        container(resize_handle(window::Direction::West, Length::Fixed(8.0), Length::Fill)).width(Length::Fill).height(Length::Fill).padding([20, 0]).align_x(iced::alignment::Horizontal::Left),
        container(resize_handle(window::Direction::East, Length::Fixed(8.0), Length::Fill)).width(Length::Fill).height(Length::Fill).padding([20, 0]).align_x(iced::alignment::Horizontal::Right),
        container(resize_handle(window::Direction::NorthWest, Length::Fixed(15.0), Length::Fixed(15.0))).width(Length::Fill).height(Length::Fill).align_x(iced::alignment::Horizontal::Left).align_y(iced::alignment::Vertical::Top),
        container(resize_handle(window::Direction::NorthEast, Length::Fixed(15.0), Length::Fixed(15.0))).width(Length::Fill).height(Length::Fill).align_x(iced::alignment::Horizontal::Right).align_y(iced::alignment::Vertical::Top),
        container(resize_handle(window::Direction::SouthWest, Length::Fixed(15.0), Length::Fixed(15.0))).width(Length::Fill).height(Length::Fill).align_x(iced::alignment::Horizontal::Left).align_y(iced::alignment::Vertical::Bottom),
        container(resize_handle(window::Direction::SouthEast, Length::Fixed(15.0), Length::Fixed(15.0))).width(Length::Fill).height(Length::Fill).align_x(iced::alignment::Horizontal::Right).align_y(iced::alignment::Vertical::Bottom),
    ];

    // 드래그 중일 때 커서를 고정하기 위한 최상단 레이어
    let final_content: Element<'_, Message> = if let Some(dir) = state.resizing_direction {
        let interaction = match dir {
            window::Direction::North | window::Direction::South => mouse::Interaction::ResizingVertically,
            window::Direction::West | window::Direction::East => mouse::Interaction::ResizingHorizontally,
            window::Direction::NorthWest | window::Direction::SouthEast => mouse::Interaction::ResizingDiagonallyDown,
            window::Direction::NorthEast | window::Direction::SouthWest => mouse::Interaction::ResizingDiagonallyUp,
        };
        stack![
            content_with_resize,
            mouse_area(container(Space::new()).width(Length::Fill).height(Length::Fill)).interaction(interaction)
        ].into()
    } else {
        content_with_resize.into()
    };

    let overlay_layer: Element<'_, Message> = if let Some(menu) = state.dummy_menu_open {
        if !menu.is_empty() {
            let dropdown = match menu {
                "Session" => container(
                    column![
                        button(text("New SSH Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenProtocolTab(ProtocolMode::Ssh)),
                        button(text("New Telnet Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenProtocolTab(ProtocolMode::Telnet)),
                        button(text("New Serial Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenProtocolTab(ProtocolMode::Serial)),
                        button(text("New Local Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenProtocolTab(ProtocolMode::Local)),
                        button(text("New RDP Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenProtocolTab(ProtocolMode::Rdp)),
                        hr(),
                        button(text("Close Current Tab").size(12)).width(Length::Fill).style(button::text).on_press(Message::CloseTab(state.active_index))
                    ]
                    .spacing(2)
                    .width(Length::Fixed(180.0))
                ),
                "Settings" => container(column![button(text("App Settings").size(12)).width(Length::Fill).style(button::text), button(text("Terminal Theme").size(12)).width(Length::Fill).style(button::text), button(text("Font Settings").size(12)).width(Length::Fill).style(button::text)].spacing(2).width(Length::Fixed(160.0))),
                _ => container(column![button(text(format!("{} Option 1", menu)).size(12)).width(Length::Fill).style(button::text), button(text(format!("{} Option 2", menu)).size(12)).width(Length::Fill).style(button::text)].spacing(2).width(Length::Fixed(160.0))),
            }.padding(4).style(|_| container::Style { background: Some(Background::Color(Color::from_rgb(0.18, 0.18, 0.18))), border: iced::Border { width: 1.0, color: Color::from_rgb(0.3, 0.3, 0.3), radius: 4.0f32.into() }, ..Default::default() });
            let h_offset: f32 = match menu { "Session" => 95.0, "Settings" => 95.0 + 72.0, "View" => 95.0 + 72.0 * 2.0, "Help" => 95.0 + 72.0 * 3.0, _ => 95.0 };
            column![Space::new().height(Length::Fixed(35.0)), row![Space::new().width(Length::Fixed(h_offset)), dropdown]].into()
        } else {
            container(Space::new().width(Length::Shrink).height(Length::Shrink)).into()
        }
    } else {
        container(Space::new().width(Length::Shrink).height(Length::Shrink)).into()
    };

    let final_layout: Element<'_, Message> = stack![final_content, overlay_layer].into();

    container(final_layout).width(Length::Fill).height(Length::Fill).style(|_| container::Style { background: Some(Background::Color(Color::from_rgb(0.08, 0.08, 0.08))), text_color: Some(Color::WHITE), ..Default::default() }).into()
}


