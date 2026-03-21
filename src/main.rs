// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::widget::{button, column, container, image as iced_image, pick_list, row, scrollable, text, vertical_slider, Space, text_input, Id, mouse_area, stack};
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
    id_host: Id,
    id_port: Id,
    id_user: Id,
    id_pass: Id,
    focused_field: usize,
    pub window_id: Option<window::Id>,
    pub dummy_menu_open: Option<&'static str>,
    pub resizing_direction: Option<window::Direction>,
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
            id_host: Id::new("host"),
            id_port: Id::new("port"),
            id_user: Id::new("user"),
            id_pass: Id::new("pass"),
            focused_field: 0,
            window_id: None,
            dummy_menu_open: None,
            resizing_direction: None,
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
            let bytes = text_str.into_bytes();
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if let Some(ref sender) = session.sender { let _ = sender.send(connection::ConnectionInput::Data(bytes)); }
                else { session.terminal.process_bytes(&bytes); }
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
                        let _ = sender.send(connection::ConnectionInput::Resize { cols: session.terminal.cols as u16, rows: session.terminal.rows as u16 });
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
                    connection::ConnectionEvent::Frame(frame) => {
                        if session.remote_display.is_none() {
                            session.remote_display = Some(RemoteDisplayState::new(1280, 1024));
                        }
                        if let Some(display) = session.remote_display.as_mut() {
                            display.apply(frame);
                        }
                    }
                    connection::ConnectionEvent::Disconnected => {
                        session.sender = None;
                        session.terminal.process_bytes(b"\r\n\x1b[31m[Disconnected]\x1b[0m\r\n");
                        session.terminal.cache.clear();
                    }
                    connection::ConnectionEvent::Error(e) => {
                        session.sender = None;
                        let msg = format!("\r\n\x1b[31m[Error: {}]\x1b[0m\r\n", e);
                        session.terminal.process_bytes(msg.as_bytes());
                        session.terminal.cache.clear();
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
        Message::SelectProtocol(mode) => { state.welcome_protocol = mode; Task::none() }
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
                *session = Session::new_remote_display(session.id, name, 1280, 1024);
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
                *session = Session::new_terminal(session.id, name, session.terminal.rows, session.terminal.cols);
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
                    let _ = sender.send(connection::ConnectionInput::RdpInput(input));
                }
            }
            Task::none()
        }
        Message::TabPressed(shift) => {
            if shift { state.focused_field = if state.focused_field == 0 { 3 } else { state.focused_field - 1 }; }
            else { state.focused_field = (state.focused_field + 1) % 4; }
            let target_id = match state.focused_field { 0 => state.id_host.clone(), 1 => state.id_port.clone(), 2 => state.id_user.clone(), _ => state.id_pass.clone() };
            focus(target_id)
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
        let remote_sub = event::listen_with(|event, _status, _window| {
            match event {
                iced::Event::Keyboard(keyboard::Event::KeyPressed { key, text, .. }) => {
                    if let Some(ch) = text.and_then(|t| t.chars().next()) {
                        let codepoint = ch as u32;
                        if codepoint <= 0xFFFF {
                            return Some(Message::RemoteRdpInput(connection::RdpInput::KeyboardUnicode {
                                codepoint: codepoint as u16,
                                down: true,
                            }));
                        }
                    }

                    map_key_to_rdp_scancode(&key).map(|(code, extended)| {
                        Message::RemoteRdpInput(connection::RdpInput::KeyboardScancode {
                            code,
                            extended,
                            down: true,
                        })
                    })
                }
                iced::Event::Keyboard(keyboard::Event::KeyReleased { key, .. }) => {
                    map_key_to_rdp_scancode(&key).map(|(code, extended)| {
                        Message::RemoteRdpInput(connection::RdpInput::KeyboardScancode {
                            code,
                            extended,
                            down: false,
                        })
                    })
                }
                iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Message::RemoteRdpInput(connection::RdpInput::MouseMove {
                        x: position.x.max(0.0).min(u16::MAX as f32) as u16,
                        y: position.y.max(0.0).min(u16::MAX as f32) as u16,
                    }))
                }
                iced::Event::Mouse(mouse::Event::ButtonPressed(button)) => {
                    match button {
                        mouse::Button::Left => Some(Message::RemoteRdpInput(connection::RdpInput::MouseButton { button: connection::RdpMouseButton::Left, down: true })),
                        mouse::Button::Right => Some(Message::RemoteRdpInput(connection::RdpInput::MouseButton { button: connection::RdpMouseButton::Right, down: true })),
                        mouse::Button::Middle => Some(Message::RemoteRdpInput(connection::RdpInput::MouseButton { button: connection::RdpMouseButton::Middle, down: true })),
                        _ => None,
                    }
                }
                iced::Event::Mouse(mouse::Event::ButtonReleased(button)) => {
                    match button {
                        mouse::Button::Left => Some(Message::RemoteRdpInput(connection::RdpInput::MouseButton { button: connection::RdpMouseButton::Left, down: false })),
                        mouse::Button::Right => Some(Message::RemoteRdpInput(connection::RdpInput::MouseButton { button: connection::RdpMouseButton::Right, down: false })),
                        mouse::Button::Middle => Some(Message::RemoteRdpInput(connection::RdpInput::MouseButton { button: connection::RdpMouseButton::Middle, down: false })),
                        _ => None,
                    }
                }
                iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                    let v = match delta {
                        mouse::ScrollDelta::Lines { y, .. } => y,
                        mouse::ScrollDelta::Pixels { y, .. } => y / 40.0,
                    };
                    let step = (v * 120.0).round();
                    if step == 0.0 {
                        None
                    } else {
                        Some(Message::RemoteRdpInput(connection::RdpInput::MouseWheel {
                            delta: step.max(i16::MIN as f32).min(i16::MAX as f32) as i16,
                        }))
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

fn map_key_to_rdp_scancode(key: &keyboard::Key) -> Option<(u8, bool)> {
    match key {
        keyboard::Key::Named(keyboard::key::Named::Enter) => Some((0x1C, false)),
        keyboard::Key::Named(keyboard::key::Named::Backspace) => Some((0x0E, false)),
        keyboard::Key::Named(keyboard::key::Named::Tab) => Some((0x0F, false)),
        keyboard::Key::Named(keyboard::key::Named::Escape) => Some((0x01, false)),
        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => Some((0x48, true)),
        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => Some((0x50, true)),
        keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => Some((0x4B, true)),
        keyboard::Key::Named(keyboard::key::Named::ArrowRight) => Some((0x4D, true)),
        keyboard::Key::Named(keyboard::key::Named::Home) => Some((0x47, true)),
        keyboard::Key::Named(keyboard::key::Named::End) => Some((0x4F, true)),
        keyboard::Key::Named(keyboard::key::Named::PageUp) => Some((0x49, true)),
        keyboard::Key::Named(keyboard::key::Named::PageDown) => Some((0x51, true)),
        keyboard::Key::Named(keyboard::key::Named::Insert) => Some((0x52, true)),
        keyboard::Key::Named(keyboard::key::Named::Delete) => Some((0x53, true)),
        keyboard::Key::Named(keyboard::key::Named::F1) => Some((0x3B, false)),
        keyboard::Key::Named(keyboard::key::Named::F2) => Some((0x3C, false)),
        keyboard::Key::Named(keyboard::key::Named::F3) => Some((0x3D, false)),
        keyboard::Key::Named(keyboard::key::Named::F4) => Some((0x3E, false)),
        keyboard::Key::Named(keyboard::key::Named::F5) => Some((0x3F, false)),
        keyboard::Key::Named(keyboard::key::Named::F6) => Some((0x40, false)),
        keyboard::Key::Named(keyboard::key::Named::F7) => Some((0x41, false)),
        keyboard::Key::Named(keyboard::key::Named::F8) => Some((0x42, false)),
        keyboard::Key::Named(keyboard::key::Named::F9) => Some((0x43, false)),
        keyboard::Key::Named(keyboard::key::Named::F10) => Some((0x44, false)),
        keyboard::Key::Named(keyboard::key::Named::F11) => Some((0x57, false)),
        keyboard::Key::Named(keyboard::key::Named::F12) => Some((0x58, false)),
        _ => None,
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
                        row![text("Host: ").width(100), text_input("IP Address", &state.ssh_host).id(state.id_host.clone()).on_input(Message::HostChanged).width(300)],
                        row![text("Port: ").width(100), text_input("22", &state.ssh_port).id(state.id_port.clone()).on_input(Message::PortChanged).width(150)],
                        row![text("Username: ").width(100), text_input("user", &state.ssh_user).id(state.id_user.clone()).on_input(Message::UserChanged).width(300)],
                        row![text("Password: ").width(100), text_input("pass", &state.ssh_pass).id(state.id_pass.clone()).on_input(Message::PassChanged).secure(true).width(300).on_submit(Message::ConnectSsh)],
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Connect")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectSsh)
                    ].spacing(15).into(),
                    ProtocolMode::Telnet => column![
                        text("Telnet Connection").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        row![text("Host: ").width(100), text_input("IP Address", &state.ssh_host).on_input(Message::HostChanged).width(300)],
                        row![text("Port: ").width(100), text_input("23", &state.ssh_port).on_input(Message::PortChanged).width(150).on_submit(Message::ConnectTelnet)],
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Connect")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectTelnet)
                    ].spacing(15).into(),
                    ProtocolMode::Serial => column![
                        text("Serial Connection").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        row![text("COM Port: ").width(100), text_input("COM1", &state.serial_port).on_input(Message::SerialPortChanged).width(300)],
                        row![text("Baud Rate: ").width(100), text_input("115200", &state.serial_baud).on_input(Message::SerialBaudChanged).width(150).on_submit(Message::ConnectSerial)],
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
                        row![text("Host: ").width(100), text_input("IP Address", &state.rdp_host).on_input(Message::RdpHostChanged).width(300)],
                        row![text("Port: ").width(100), text_input("3389", &state.rdp_port).on_input(Message::RdpPortChanged).width(150)],
                        row![text("Username: ").width(100), text_input("user", &state.rdp_user).on_input(Message::RdpUserChanged).width(300)],
                        row![text("Password: ").width(100), text_input("pass", &state.rdp_pass).on_input(Message::RdpPassChanged).secure(true).width(300).on_submit(Message::ConnectRdp)],
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
                    if let Some(handle) = &display.handle {
                        container(
                            iced_image(handle.clone())
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


