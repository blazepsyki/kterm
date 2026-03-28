// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::widget::{button, column, container, pick_list, row, scrollable, text, vertical_slider, Space, text_input, Id, mouse_area, stack, shader};
use iced::{Background, Color, Element, Length, Task, Subscription, event, keyboard, advanced::input_method, Font, font::Weight, mouse};
use iced::widget::operation::focus;
use env_logger::Env;
use iced::window;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::collections::HashSet;
use std::env;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
mod terminal;
mod connection;
mod platform;
mod remote_display;
use connection::remote_input_policy::{
    current_keyboard_indicators, is_remote_secure_attention_key,
    is_remote_secure_attention_shortcut, remote_secure_attention_inputs, route_key_pressed,
    route_key_released, unicode_inputs_for_text, RoutedKeyEvent,
};
use remote_display::RemoteDisplayState;
use terminal::{TerminalEmulator, TerminalView, Selection};
use tokio::sync::mpsc;

// ── Clipboard (Windows native CLIPRDR backend) ────────────────────────────────
#[cfg(windows)]
use ironrdp_cliprdr::backend::{ClipboardMessage, ClipboardMessageProxy};
#[cfg(windows)]
use ironrdp_cliprdr_native::WinClipboard;

/// Proxy implementation that forwards ClipboardMessage events from the
/// WinClipboard OS message loop to the RDP worker's async channel.
#[cfg(windows)]
#[derive(Debug)]
struct ChannelProxy {
    tx: mpsc::UnboundedSender<ClipboardMessage>,
}

#[cfg(windows)]
impl ClipboardMessageProxy for ChannelProxy {
    fn send_clipboard_message(&self, message: ClipboardMessage) {
        let _ = self.tx.send(message);
    }
}

// Per-session WinClipboard instances stored in thread-local storage so they
// can hold !Send types while living on the iced main thread.
#[cfg(windows)]
thread_local! {
    static WIN_CLIPBOARDS: std::cell::RefCell<std::collections::HashMap<u64, WinClipboard>>
        = std::cell::RefCell::new(std::collections::HashMap::new());
}
// ─────────────────────────────────────────────────────────────────────────────

pub const D2CODING: iced::Font = iced::Font {
    family: iced::font::Family::Name("D2Coding"),
    ..iced::Font::DEFAULT
};

// RDP Resolution Presets
const RDP_RESOLUTION_PRESETS: &[(u16, u16)] = &[
    (1024, 768),
    (1280, 720),
    (1280, 1024),
    (1366, 768),
    (1600, 900),
    (1920, 1080),
    (2560, 1440),
];

static RDP_TRACE_ENABLED: OnceLock<bool> = OnceLock::new();
static SESSION_LOG_PATH: OnceLock<std::path::PathBuf> = OnceLock::new();
static SESSION_LOG_FILE: OnceLock<Arc<Mutex<File>>> = OnceLock::new();
const VNC_RECT_ONLY_STREAK_FORCE_THRESHOLD: u32 = 6;
const VNC_RECT_BATCH_FORCE_THRESHOLD: usize = 64;

struct TeeLoggerWriter {
    file: Arc<Mutex<File>>,
}

impl Write for TeeLoggerWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if let Ok(mut f) = self.file.lock() {
            f.write_all(buf)?;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if let Ok(mut f) = self.file.lock() {
            f.flush()?;
        }
        Ok(())
    }
}

fn session_log_path() -> &'static std::path::Path {
    SESSION_LOG_PATH
        .get_or_init(|| {
            let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
            Path::new("logs").join(format!("kterm_{}.log", timestamp))
        })
        .as_path()
}

pub fn runtime_log_path() -> std::path::PathBuf {
    session_log_path().to_path_buf()
}

fn init_session_log_file() -> Result<(), String> {
    let path = session_log_path().to_path_buf();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("cannot create log dir: {}", e))?;
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("cannot open runtime log: {}", e))?;

    writeln!(
        file,
        "\n=== kterm run start {} ===",
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
    )
    .map_err(|e| format!("cannot write runtime log header: {}", e))?;

    let _ = SESSION_LOG_FILE.set(Arc::new(Mutex::new(file)));
    Ok(())
}

fn rdp_trace_enabled() -> bool {
    *RDP_TRACE_ENABLED.get_or_init(|| {
        std::env::var("KTERM_RDP_TRACE")
            .map(|v| {
                let v = v.to_ascii_lowercase();
                v == "1" || v == "true" || v == "yes" || v == "on"
            })
            .unwrap_or(false)
    })
}

pub fn main() -> iced::Result {
    let _ = init_session_log_file();

    let mut logger = env_logger::Builder::from_env(Env::default().default_filter_or("info"));
    logger.format_timestamp_millis();
    if let Some(file) = SESSION_LOG_FILE.get().cloned() {
        logger.target(env_logger::Target::Pipe(Box::new(TeeLoggerWriter {
            file,
        })));
    }
    let _ = logger.try_init();

    log::info!("[LOG] unified session log file: {}", session_log_path().display());

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
    Settings,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolMode {
    Ssh,
    Telnet,
    Serial,
    Local,
    Rdp,
    Vnc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTabKind {
    Preferences,
    Theme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsToggleKey {
    AutoReconnect,
    UseAgentForwarding,
    EchoLocally,
    HardwareFlowControl,
    LaunchInLoginMode,
    RdpNla,
    VncRemoteCursor,
    CompactTabStyle,
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
    vnc_rect_only_streak: u32,
    settings_tab_kind: Option<SettingsTabKind>,
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
            vnc_rect_only_streak: 0,
            settings_tab_kind: None,
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
            vnc_rect_only_streak: 0,
            settings_tab_kind: None,
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
            vnc_rect_only_streak: 0,
            settings_tab_kind: None,
        }
    }

    fn new_settings(id: u64, tab_kind: SettingsTabKind) -> Self {
        let name = match tab_kind {
            SettingsTabKind::Preferences => "Settings".to_string(),
            SettingsTabKind::Theme => "Theme".to_string(),
        };

        Self {
            id,
            name,
            kind: SessionKind::Settings,
            terminal: TerminalEmulator::new(24, 80),
            remote_display: None,
            sender: None,
            rdp_secure_attention_active: false,
            vnc_rect_only_streak: 0,
            settings_tab_kind: Some(tab_kind),
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
    selected_rdp_resolution_index: usize,
    vnc_host: String,
    vnc_port: String,
    vnc_pass: String,
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
    rdp_id_resolution: Id,
    vnc_id_host: Id,
    vnc_id_port: Id,
    vnc_id_pass: Id,
    focused_field: usize,
    pub window_id: Option<window::Id>,
    pub dummy_menu_open: Option<&'static str>,
    pub resizing_direction: Option<window::Direction>,
    pub window_size: (f32, f32),
    // Settings UI state
    settings_selected_category: usize,  // 선택된 설정 카테고리 인덱스
    settings_auto_reconnect: bool,
    settings_ssh_use_agent_forwarding: bool,
    settings_telnet_echo_locally: bool,
    settings_serial_hardware_flow_control: bool,
    settings_local_launch_in_login_mode: bool,
    settings_rdp_nla: bool,
    settings_vnc_remote_cursor: bool,
    settings_theme_compact_tab_style: bool,
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
            selected_rdp_resolution_index: 1,  // Default to 1280x720
            vnc_host: "".to_string(),
            vnc_port: "5900".to_string(),
            vnc_pass: "".to_string(),
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
            rdp_id_resolution: Id::new("rdp_resolution"),
            vnc_id_host: Id::new("vnc_host"),
            vnc_id_port: Id::new("vnc_port"),
            vnc_id_pass: Id::new("vnc_pass"),
            focused_field: 0,
            window_id: None,
            dummy_menu_open: None,
            resizing_direction: None,
            window_size: (1024.0, 768.0),
            settings_selected_category: 0,
            settings_auto_reconnect: false,
            settings_ssh_use_agent_forwarding: false,
            settings_telnet_echo_locally: true,
            settings_serial_hardware_flow_control: false,
            settings_local_launch_in_login_mode: true,
            settings_rdp_nla: true,
            settings_vnc_remote_cursor: true,
            settings_theme_compact_tab_style: false,
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
                self.rdp_id_resolution.clone(),
            ],
            ProtocolMode::Vnc => vec![
                self.vnc_id_host.clone(),
                self.vnc_id_port.clone(),
                self.vnc_id_pass.clone(),
            ],
            ProtocolMode::Local => vec![],
        }
    }

    fn settings_checkbox_value(&self, key: SettingsToggleKey) -> bool {
        match key {
            SettingsToggleKey::AutoReconnect => self.settings_auto_reconnect,
            SettingsToggleKey::UseAgentForwarding => self.settings_ssh_use_agent_forwarding,
            SettingsToggleKey::EchoLocally => self.settings_telnet_echo_locally,
            SettingsToggleKey::HardwareFlowControl => self.settings_serial_hardware_flow_control,
            SettingsToggleKey::LaunchInLoginMode => self.settings_local_launch_in_login_mode,
            SettingsToggleKey::RdpNla => self.settings_rdp_nla,
            SettingsToggleKey::VncRemoteCursor => self.settings_vnc_remote_cursor,
            SettingsToggleKey::CompactTabStyle => self.settings_theme_compact_tab_style,
        }
    }

    fn toggle_settings_checkbox(&mut self, key: SettingsToggleKey) {
        match key {
            SettingsToggleKey::AutoReconnect => self.settings_auto_reconnect = !self.settings_auto_reconnect,
            SettingsToggleKey::UseAgentForwarding => self.settings_ssh_use_agent_forwarding = !self.settings_ssh_use_agent_forwarding,
            SettingsToggleKey::EchoLocally => self.settings_telnet_echo_locally = !self.settings_telnet_echo_locally,
            SettingsToggleKey::HardwareFlowControl => self.settings_serial_hardware_flow_control = !self.settings_serial_hardware_flow_control,
            SettingsToggleKey::LaunchInLoginMode => self.settings_local_launch_in_login_mode = !self.settings_local_launch_in_login_mode,
            SettingsToggleKey::RdpNla => self.settings_rdp_nla = !self.settings_rdp_nla,
            SettingsToggleKey::VncRemoteCursor => self.settings_vnc_remote_cursor = !self.settings_vnc_remote_cursor,
            SettingsToggleKey::CompactTabStyle => self.settings_theme_compact_tab_style = !self.settings_theme_compact_tab_style,
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
    RdpResolutionSelected(usize),
    VncHostChanged(String),
    VncPortChanged(String),
    VncPassChanged(String),
    SelectProtocol(ProtocolMode),
    SelectLocalShell(usize),
    ConnectSsh,
    ConnectTelnet,
    ConnectLocal,
    ConnectSerial,
    ConnectRdp,
    ConnectVnc,
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
    RemoteDisplayInput(connection::RemoteInput),
    RemoteDisplayInputs(Vec<connection::RemoteInput>),
    RemoteSecureAttention(bool),
    SyncRdpKeyboardIndicators,
    ReleaseRdpModifiers,
    WindowSizeChanged(f32, f32),
    RemoteDisplayRedrawPulse,
    // Settings UI messages
    OpenSettingsTab(SettingsTabKind),
    SettingsCategorySelected(usize),
    ToggleSettingsCheckbox(SettingsToggleKey),
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
            // Explicitly drop the sender before removing so the worker receives
            // the channel-closed signal and exits its recv() loop immediately.
            if let Some(session) = state.sessions.get_mut(index) {
                session.sender = None;
                // Clean up the native clipboard window for this session.
                #[cfg(windows)]
                WIN_CLIPBOARDS.with(|m| m.borrow_mut().remove(&session.id));
            }
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
                            let _ = sender.send(connection::ConnectionInput::Resize { cols: pixel_w, rows: pixel_h });
                            let _ = sender.send(connection::ConnectionInput::SyncKeyboardIndicators(current_keyboard_indicators()));
                        } else {
                            let _ = sender.send(connection::ConnectionInput::Resize { cols: session.terminal.cols as u16, rows: session.terminal.rows as u16 });
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
                            let responses: Vec<Vec<u8>> = session.terminal.pending_responses.drain(..).collect();
                            for resp in responses {
                                if let Some(ref sender) = session.sender { let _ = sender.send(connection::ConnectionInput::Data(resp)); }
                            }
                        }
                    }
                    connection::ConnectionEvent::Frames(frames) => {
                        let is_vnc_session = session.name.starts_with("VNC:");
                        let frame_batch_len = frames.len();
                        let mut full_count = 0usize;
                        let mut rect_count = 0usize;
                        for f in &frames {
                            match f {
                                remote_display::FrameUpdate::Full { .. } => full_count += 1,
                                remote_display::FrameUpdate::Rect { .. } => rect_count += 1,
                            }
                        }
                        if rdp_trace_enabled() {
                            log::info!(
                                "[RDP-UI] session_id={} frames={} full={} rect={}",
                                target_id,
                                frame_batch_len,
                                full_count,
                                rect_count,
                            );
                        }

                        if session.remote_display.is_none() {
                            session.remote_display = Some(RemoteDisplayState::new(1280, 720));
                        }
                        if let Some(display) = session.remote_display.as_mut() {
                            let frame_seq_before = display.frame_seq;
                            display.status_message = None;

                            // Start a fresh dirty batch for this UI event.
                            if !display.full_upload {
                                display.dirty_rects.clear();
                            }

                            for frame in frames {
                                display.apply(frame);
                            }

                            if is_vnc_session {
                                if full_count > 0 {
                                    session.vnc_rect_only_streak = 0;
                                } else if rect_count > 0 {
                                    session.vnc_rect_only_streak = session
                                        .vnc_rect_only_streak
                                        .saturating_add(1);
                                }
                            }

                            // RDP bootstrap: if the first meaningful update is rect-only,
                            // force one full upload so the accumulated CPU buffer is presented.
                            // Also force full upload for very large rect batches.
                            let force_bootstrap = frame_seq_before == 0 && full_count == 0 && rect_count > 0;
                            let force_large_batch = full_count == 0 && rect_count > 256;
                            let force_vnc_streak = is_vnc_session
                                && full_count == 0
                                && rect_count > 0
                                && session.vnc_rect_only_streak >= VNC_RECT_ONLY_STREAK_FORCE_THRESHOLD;
                            let force_vnc_batch = is_vnc_session
                                && full_count == 0
                                && rect_count >= VNC_RECT_BATCH_FORCE_THRESHOLD;

                            if force_bootstrap || force_large_batch || force_vnc_streak || force_vnc_batch
                            {
                                display.full_upload = true;
                                display.dirty_rects.clear();
                                if is_vnc_session {
                                    session.vnc_rect_only_streak = 0;
                                }
                                if rdp_trace_enabled() {
                                    let reason = if force_bootstrap {
                                        "bootstrap"
                                    } else if force_large_batch {
                                        "large_rect_batch"
                                    } else if force_vnc_streak {
                                        "vnc_rect_only_streak"
                                    } else {
                                        "vnc_rect_batch"
                                    };
                                    log::info!(
                                        "[RDP-UI] force_full_upload session_id={} reason={} rects={} vnc_streak={}",
                                        target_id,
                                        reason,
                                        rect_count,
                                        session.vnc_rect_only_streak,
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
                            session.terminal.process_bytes(b"\r\n\x1b[31m[Disconnected]\x1b[0m\r\n");
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
        Message::RdpResolutionSelected(index) => { state.selected_rdp_resolution_index = index; Task::none() }
        Message::VncHostChanged(s) => { state.vnc_host = s; Task::none() }
        Message::VncPortChanged(s) => { state.vnc_port = s; Task::none() }
        Message::VncPassChanged(s) => { state.vnc_pass = s; Task::none() }
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
            let (width, height) = RDP_RESOLUTION_PRESETS.get(state.selected_rdp_resolution_index).copied().unwrap_or((1280, 720));
            let name = format!("RDP: {}@{}:{}", user, host, port);

            let target_index = state.active_index;
            let mut target_id = None;
            if let Some(session) = state.sessions.get_mut(target_index) {
                target_id = Some(session.id);
                *session = Session::new_remote_display(session.id, name, width, height);
            }

            if let Some(target_id) = target_id {
                // ── Windows native clipboard (CLIPRDR) ───────────────────────
                #[cfg(windows)]
                let (cliprdr_factory, clipboard_rx_opt) = {
                    let (cb_tx, cb_rx) = mpsc::unbounded_channel::<ClipboardMessage>();
                    let proxy = Box::new(ChannelProxy { tx: cb_tx });
                    match WinClipboard::new(*proxy) {
                        Ok(win_clipboard) => {
                            let factory = win_clipboard.backend_factory();
                            WIN_CLIPBOARDS.with(|m| m.borrow_mut().insert(target_id, win_clipboard));
                            (Some(factory), Some(cb_rx))
                        }
                        Err(e) => {
                            log::info!("[CLIPRDR] WinClipboard creation failed: {}", e);
                            (None, None)
                        }
                    }
                };
                #[cfg(not(windows))]
                let (cliprdr_factory, clipboard_rx_opt) = {
                    let f: Option<Box<dyn ironrdp_cliprdr::backend::CliprdrBackendFactory + Send>> = None;
                    let r: Option<mpsc::UnboundedReceiver<ironrdp_cliprdr::backend::ClipboardMessage>> = None;
                    (f, r)
                };
                // ─────────────────────────────────────────────────────────────

                Task::run(
                    connection::rdp::connect_and_subscribe(host, port, user, pass, width, height, cliprdr_factory, clipboard_rx_opt),
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
            let name = format!("VNC: {}:{}", host, port);

            let target_index = state.active_index;
            let mut target_id = None;
            if let Some(session) = state.sessions.get_mut(target_index) {
                target_id = Some(session.id);
                *session = Session::new_remote_display(session.id, name, 1280, 720);
                if let Some(display) = session.remote_display.as_mut() {
                    display.status_message = Some("Connecting to VNC server...".to_string());
                }
            }

            if let Some(target_id) = target_id {
                Task::run(
                    connection::vnc::connect_and_subscribe(host, port, pass_opt),
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
        Message::RemoteDisplayInput(input) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if let Some(ref sender) = session.sender {
                    let input = transform_remote_mouse(input, state.window_size, session.remote_display.as_ref());
                    let _ = sender.send(connection::ConnectionInput::RemoteInput(input));
                }
            }
            Task::none()
        }
        Message::RemoteDisplayInputs(inputs) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if let Some(ref sender) = session.sender {
                    for input in inputs {
                        let input = transform_remote_mouse(input, state.window_size, session.remote_display.as_ref());
                        let _ = sender.send(connection::ConnectionInput::RemoteInput(input));
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
                        let _ = sender.send(connection::ConnectionInput::RemoteInput(input));
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
        event::listen_with(|event, status, _window| {
            match event {
                // 메뉴 항목 클릭(캡처된 이벤트)은 유지하고, 외부 클릭(무시된 이벤트)에서만 닫기
                iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) if matches!(status, event::Status::Ignored) => Some(Message::CloseMenuDeferred),
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
                =>
                {
                    if is_remote_secure_attention_shortcut(&physical_key, modifiers) {
                        return Some(Message::RemoteSecureAttention(true));
                    }

                    match route_key_pressed(&key, text.as_deref(), &physical_key) {
                        RoutedKeyEvent::Ignore => None,
                        RoutedKeyEvent::SyncIndicators => {
                            Some(Message::SyncRdpKeyboardIndicators)
                        }
                        RoutedKeyEvent::Input(input) => Some(Message::RemoteDisplayInput(input)),
                    }
                }
                iced::Event::Keyboard(keyboard::Event::KeyReleased { key, physical_key, modifiers, .. })
                =>
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
                        RoutedKeyEvent::Input(input) => Some(Message::RemoteDisplayInput(input)),
                    }
                }
                iced::Event::Mouse(mouse::Event::CursorMoved { position }) => {
                    Some(Message::RemoteDisplayInput(connection::RemoteInput::MouseMove {
                        x: position.x.max(0.0).min(u16::MAX as f32) as u16,
                        y: position.y.max(0.0).min(u16::MAX as f32) as u16,
                    }))
                }
                iced::Event::Mouse(mouse::Event::ButtonPressed(button))
                    if status == event::Status::Ignored =>
                {
                    match button {
                        mouse::Button::Left => Some(Message::RemoteDisplayInput(connection::RemoteInput::MouseButton { button: connection::RemoteMouseButton::Left, down: true })),
                        mouse::Button::Right => Some(Message::RemoteDisplayInput(connection::RemoteInput::MouseButton { button: connection::RemoteMouseButton::Right, down: true })),
                        mouse::Button::Middle => Some(Message::RemoteDisplayInput(connection::RemoteInput::MouseButton { button: connection::RemoteMouseButton::Middle, down: true })),
                        _ => None,
                    }
                }
                iced::Event::Mouse(mouse::Event::ButtonReleased(button))
                    if status == event::Status::Ignored =>
                {
                    match button {
                        mouse::Button::Left => Some(Message::RemoteDisplayInput(connection::RemoteInput::MouseButton { button: connection::RemoteMouseButton::Left, down: false })),
                        mouse::Button::Right => Some(Message::RemoteDisplayInput(connection::RemoteInput::MouseButton { button: connection::RemoteMouseButton::Right, down: false })),
                        mouse::Button::Middle => Some(Message::RemoteDisplayInput(connection::RemoteInput::MouseButton { button: connection::RemoteMouseButton::Middle, down: false })),
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
                        Some(Message::RemoteDisplayInput(connection::RemoteInput::MouseWheel {
                            delta: vy_step.max(i16::MIN as f32).min(i16::MAX as f32) as i16,
                        }))
                    } else if hx_step != 0.0 {
                        Some(Message::RemoteDisplayInput(connection::RemoteInput::MouseHorizontalWheel {
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

fn transform_remote_mouse(
    input: connection::RemoteInput,
    window_size: (f32, f32),
    display: Option<&RemoteDisplayState>,
) -> connection::RemoteInput {
    match input {
        connection::RemoteInput::MouseMove { x, y } => {
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

                let remote_x = ((rel_x - offset_x) / rendered_w * desk_w).clamp(0.0, desk_w - 1.0) as u16;
                let remote_y = ((rel_y - offset_y) / rendered_h * desk_h).clamp(0.0, desk_h - 1.0) as u16;
                connection::RemoteInput::MouseMove { x: remote_x, y: remote_y }
            } else {
                connection::RemoteInput::MouseMove { x: rel_x as u16, y: rel_y as u16 }
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

// ─── Settings 탭 헬퍼 함수들 ───────────────────────────────────────────────────

fn get_settings_categories(tab_kind: SettingsTabKind) -> Vec<&'static str> {
    match tab_kind {
        SettingsTabKind::Preferences => vec![
            "Common",
            "SSH",
            "Telnet",
            "Serial",
            "Local Shell",
            "RDP",
            "VNC",
        ],
        SettingsTabKind::Theme => vec!["Theme"],
    }
}

fn settings_header(tab_kind: SettingsTabKind) -> &'static str {
    match tab_kind {
        SettingsTabKind::Preferences => "Preferences",
        SettingsTabKind::Theme => "Theme Settings",
    }
}

fn render_settings_sidebar(tab_kind: SettingsTabKind, selected_index: usize) -> Element<'static, Message> {
    let categories = get_settings_categories(tab_kind);
    
    let mut category_buttons = column![].spacing(2);
    
    for (idx, category) in categories.iter().enumerate() {
        let is_selected = idx == selected_index;

        category_buttons = category_buttons.push(
            button(
                container(text(*category).size(13))
                    .width(Length::Fill)
                    .padding([10, 0])
            )
            .width(Length::Fill)
            .padding([6, 8])
            .style(move |_t, s| {
                let mut st = button::text(_t, s);
                if is_selected {
                    st.background = Some(Background::Color(Color::from_rgb(0.2, 0.4, 0.6)));
                    st.text_color = Color::WHITE;
                } else {
                    st.background = Some(Background::Color(if matches!(s, button::Status::Hovered) {
                        Color::from_rgb(0.15, 0.15, 0.15)
                    } else {
                        Color::TRANSPARENT
                    }));
                    st.text_color = Color::from_rgb(0.7, 0.7, 0.7);
                }
                st
            })
            .on_press(Message::SettingsCategorySelected(idx))
        );
    }
    
    container(
        scrollable(
            column![
                text(settings_header(tab_kind)).size(12).font(Font { weight: Weight::Bold, ..Default::default() }),
                Space::new().height(Length::Fixed(12.0)),
                category_buttons,
            ]
            .spacing(8)
            .padding(10)
        )
    )
    .width(Length::Fixed(180.0))
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgb(0.11, 0.11, 0.11))),
        ..Default::default()
    })
    .into()
}

fn setting_row(label: &'static str, placeholder: &'static str) -> Element<'static, Message> {
    row![
        column![
            text(label).size(13),
            text("Placeholder").size(11).color(Color::from_rgb(0.58, 0.58, 0.58)),
        ]
        .spacing(2)
        .width(Length::Fill),
        container(text_input("", placeholder).padding([6, 10]).width(Length::Fixed(240.0)))
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.10, 0.10, 0.10))),
                border: iced::Border {
                    width: 1.0,
                    color: Color::from_rgb(0.24, 0.24, 0.24),
                    radius: 6.0.into(),
                },
                ..Default::default()
            })
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

fn checkbox_placeholder_row(label: &'static str, key: SettingsToggleKey, checked: bool) -> Element<'static, Message> {
    let check_label = if checked { "[x]" } else { "[ ]" };
    let status_label = if checked { "ON" } else { "OFF" };

    row![
        column![
            text(label).size(13),
            text("Checkbox Placeholder").size(11).color(Color::from_rgb(0.58, 0.58, 0.58)),
        ]
        .spacing(2)
        .width(Length::Fill),
        button(
            row![
                text(check_label).size(12),
                text(status_label).size(12).font(Font { weight: Weight::Bold, ..Default::default() }),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
        )
        .width(Length::Fixed(90.0))
        .padding([6, 8])
        .style(move |_t, s| {
            let mut st = button::secondary(_t, s);
            st.background = Some(Background::Color(if checked {
                Color::from_rgb(0.18, 0.34, 0.22)
            } else {
                Color::from_rgb(0.18, 0.18, 0.18)
            }));
            st.text_color = Color::from_rgb(0.92, 0.92, 0.92);
            st.border = iced::Border {
                width: 1.0,
                color: if checked { Color::from_rgb(0.34, 0.54, 0.38) } else { Color::from_rgb(0.35, 0.35, 0.35) },
                radius: 6.0.into(),
            };
            st
        })
        .on_press(Message::ToggleSettingsCheckbox(key))
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

fn render_settings_panel(state: &State, tab_kind: SettingsTabKind, selected_index: usize) -> Element<'static, Message> {
    let categories = get_settings_categories(tab_kind);
    let category_name = categories.get(selected_index).copied().unwrap_or("Unknown");

    let title = match tab_kind {
        SettingsTabKind::Preferences => format!("{} Settings", category_name),
        SettingsTabKind::Theme => "Theme Settings".to_string(),
    };

    let content: Element<'_, Message> = match (tab_kind, category_name) {
        (SettingsTabKind::Preferences, "Common") => column![
            text(title).size(18).font(Font { weight: Weight::Bold, ..Default::default() }),
            text("Adjust application-wide behavior.").size(12).color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Default Connection Profile", "placeholder-default-profile"),
            checkbox_placeholder_row("Auto-reconnect", SettingsToggleKey::AutoReconnect, state.settings_checkbox_value(SettingsToggleKey::AutoReconnect)),
            setting_row("Global Timeout", "placeholder-timeout-ms"),
        ].spacing(14).into(),
        (SettingsTabKind::Preferences, "SSH") => column![
            text(title).size(18).font(Font { weight: Weight::Bold, ..Default::default() }),
            text("Configure SSH behavior.").size(12).color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Host Key Policy", "placeholder-strict-checking"),
            setting_row("Keep Alive Interval", "placeholder-interval-sec"),
            checkbox_placeholder_row("Use Agent Forwarding", SettingsToggleKey::UseAgentForwarding, state.settings_checkbox_value(SettingsToggleKey::UseAgentForwarding)),
        ].spacing(14).into(),
        (SettingsTabKind::Preferences, "Telnet") => column![
            text(title).size(18).font(Font { weight: Weight::Bold, ..Default::default() }),
            text("Configure Telnet behavior.").size(12).color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Negotiation Mode", "placeholder-auto"),
            setting_row("Line Ending", "placeholder-CRLF"),
            checkbox_placeholder_row("Echo Locally", SettingsToggleKey::EchoLocally, state.settings_checkbox_value(SettingsToggleKey::EchoLocally)),
        ].spacing(14).into(),
        (SettingsTabKind::Preferences, "Serial") => column![
            text(title).size(18).font(Font { weight: Weight::Bold, ..Default::default() }),
            text("Configure Serial behavior.").size(12).color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Default Baud Rate", "placeholder-115200"),
            setting_row("Parity", "placeholder-none"),
            checkbox_placeholder_row("Hardware Flow Control", SettingsToggleKey::HardwareFlowControl, state.settings_checkbox_value(SettingsToggleKey::HardwareFlowControl)),
        ].spacing(14).into(),
        (SettingsTabKind::Preferences, "Local Shell") => column![
            text(title).size(18).font(Font { weight: Weight::Bold, ..Default::default() }),
            text("Configure local shell launch behavior.").size(12).color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Default Shell", "placeholder-pwsh"),
            setting_row("Startup Args", "placeholder-no-logo"),
            checkbox_placeholder_row("Launch In Login Mode", SettingsToggleKey::LaunchInLoginMode, state.settings_checkbox_value(SettingsToggleKey::LaunchInLoginMode)),
        ].spacing(14).into(),
        (SettingsTabKind::Preferences, "RDP") => column![
            text(title).size(18).font(Font { weight: Weight::Bold, ..Default::default() }),
            text("Configure RDP behavior.").size(12).color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Default Resolution", "placeholder-1280x720"),
            setting_row("Color Depth", "placeholder-32bit"),
            checkbox_placeholder_row("NLA", SettingsToggleKey::RdpNla, state.settings_checkbox_value(SettingsToggleKey::RdpNla)),
        ].spacing(14).into(),
        (SettingsTabKind::Preferences, "VNC") => column![
            text(title).size(18).font(Font { weight: Weight::Bold, ..Default::default() }),
            text("Configure VNC behavior.").size(12).color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Encoding", "placeholder-tight"),
            setting_row("Compression", "placeholder-medium"),
            checkbox_placeholder_row("Remote Cursor", SettingsToggleKey::VncRemoteCursor, state.settings_checkbox_value(SettingsToggleKey::VncRemoteCursor)),
        ].spacing(14).into(),
        (SettingsTabKind::Theme, "Theme") => column![
            text(title).size(18).font(Font { weight: Weight::Bold, ..Default::default() }),
            text("Theme options placeholder.").size(12).color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Color Theme", "placeholder-dark"),
            setting_row("Terminal Font Family", "placeholder-d2coding"),
            checkbox_placeholder_row("Use Compact Tab Style", SettingsToggleKey::CompactTabStyle, state.settings_checkbox_value(SettingsToggleKey::CompactTabStyle)),
        ].spacing(14).into(),
        _ => column![
            text(title).size(18).font(Font { weight: Weight::Bold, ..Default::default() }),
            text("No settings available.").size(12).color(Color::from_rgb(0.65, 0.65, 0.65)),
        ].spacing(14).into(),
    };
    
    container(
        scrollable(content)
            .height(Length::Fill)
    )
    .width(Length::Fill)
    .height(Length::Fill)
    .padding(30)
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgb(0.12, 0.12, 0.12))),
        ..Default::default()
    })
    .into()
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
                    protocol_btn(ProtocolMode::Vnc, "VNC", &state.welcome_protocol),
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
                        {
                            let resolution_labels: Vec<String> = RDP_RESOLUTION_PRESETS.iter().map(|(w, h)| format!("{}x{}", w, h)).collect();
                            let selected_label = Some(format!("{}x{}", RDP_RESOLUTION_PRESETS[state.selected_rdp_resolution_index].0, RDP_RESOLUTION_PRESETS[state.selected_rdp_resolution_index].1));
                            let pick = pick_list(resolution_labels, selected_label, |selected| {
                                RDP_RESOLUTION_PRESETS.iter().position(|(w, h)| format!("{}x{}", w, h) == selected).map(Message::RdpResolutionSelected).unwrap_or(Message::RdpResolutionSelected(1))
                            }).width(150);
                            row![text("Resolution: ").width(100), container(pick).id(state.rdp_id_resolution.clone()).width(150)]
                        },
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Connect")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectRdp)
                    ].spacing(15).into(),
                    ProtocolMode::Vnc => column![
                        text("VNC Connection").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        row![text("Host: ").width(100), text_input("IP Address", &state.vnc_host).id(state.vnc_id_host.clone()).on_input(Message::VncHostChanged).width(300)],
                        row![text("Port: ").width(100), text_input("5900", &state.vnc_port).id(state.vnc_id_port.clone()).on_input(Message::VncPortChanged).width(150)],
                        row![text("Password: ").width(100), text_input("optional", &state.vnc_pass).id(state.vnc_id_pass.clone()).on_input(Message::VncPassChanged).secure(true).width(300).on_submit(Message::ConnectVnc)],
                        text("Password is optional for servers allowing None auth.").size(12).color(Color::from_rgb(0.6, 0.6, 0.6)),
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Connect")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectVnc)
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
                            dirty_rects: display.dirty_rects.clone(),
                            full_upload: display.full_upload,
                            frame_seq: display.frame_seq,
                            source_id: display.source_id,
                        };
                        container(
                            shader(program)
                                .width(Length::Fill)
                                .height(Length::Fill)
                        )
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .style(|_| container::Style {
                            background: Some(Background::Color(Color::BLACK)),
                            ..Default::default()
                        })
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
            SessionKind::Settings => {
                if let Some(tab_kind) = session.settings_tab_kind {
                    row![
                        render_settings_sidebar(tab_kind, state.settings_selected_category),
                        vr(),
                        container(render_settings_panel(state, tab_kind, state.settings_selected_category))
                            .width(Length::Fill)
                            .height(Length::Fill),
                    ]
                    .height(Length::Fill)
                    .into()
                } else {
                    container(text("Settings protocol not selected"))
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
                        button(text("New VNC Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenProtocolTab(ProtocolMode::Vnc)),
                        hr(),
                        button(text("Close Current Tab").size(12)).width(Length::Fill).style(button::text).on_press(Message::CloseTab(state.active_index))
                    ]
                    .spacing(2)
                    .width(Length::Fixed(200.0))
                ),
                "Settings" => container(
                    column![
                        button(text("Preferences").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenSettingsTab(SettingsTabKind::Preferences)),
                        hr(),
                        button(text("Theme").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenSettingsTab(SettingsTabKind::Theme))
                    ]
                    .spacing(2)
                    .width(Length::Fixed(170.0))
                ),
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


