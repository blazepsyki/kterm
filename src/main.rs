use iced::widget::{button, column, container, row, scrollable, text, vertical_slider, Space, text_input, Id};
use iced::{Background, Color, Element, Length, Task, Subscription, event, keyboard, advanced::input_method};
use iced::widget::operation::focus;
mod terminal;
mod ssh;
use terminal::{TerminalEmulator, TerminalView};
use tokio::sync::mpsc;

pub const D2CODING: iced::Font = iced::Font {
    family: iced::font::Family::Name("D2Coding"),
    ..iced::Font::DEFAULT
};

pub fn main() -> iced::Result {
    iced::application(
        || (State::default(), iced::font::load(include_bytes!("../assets/fonts/D2Coding.ttf")).map(Message::FontLoaded)),
        update,
        view,
    )
    .subscription(subscription)
    .title("k_term - Pure Rust Iced Client")
    .run()
}

// ---------- Session ----------

#[derive(Debug)]
enum SessionKind {
    Welcome,
    Terminal,
}

struct Session {
    name: String,
    kind: SessionKind,
    terminal: TerminalEmulator,
    sender: Option<mpsc::UnboundedSender<Vec<u8>>>,
}

impl Session {
    fn welcome() -> Self {
        Self {
            name: "Welcome".to_string(),
            kind: SessionKind::Welcome,
            terminal: TerminalEmulator::new(24, 80),
            sender: None,
        }
    }

    fn new_terminal(name: String, rows: usize, cols: usize) -> Self {
        Self {
            name,
            kind: SessionKind::Terminal,
            terminal: TerminalEmulator::new(rows, cols),
            sender: None,
        }
    }
}

// ---------- State ----------

struct State {
    sessions: Vec<Session>,
    active_index: usize,
    // SSH form fields (shared across Welcome tabs for simplicity)
    ssh_host: String,
    ssh_port: String,
    ssh_user: String,
    ssh_pass: String,
    id_host: Id,
    id_port: Id,
    id_user: Id,
    id_pass: Id,
    focused_field: usize,
}

impl Default for State {
    fn default() -> Self {
        Self {
            sessions: vec![Session::welcome()],
            active_index: 0,
            ssh_host: "".to_string(),
            ssh_port: "22".to_string(),
            ssh_user: "".to_string(),
            ssh_pass: "".to_string(),
            id_host: Id::new("host"),
            id_port: Id::new("port"),
            id_user: Id::new("user"),
            id_pass: Id::new("pass"),
            focused_field: 0,
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
    SshMessage(usize, ssh::SshEvent), // (session index, event)
    TerminalResize(usize, usize),
    TerminalScroll(f32),
    TerminalScrollTo(usize),
    HostChanged(String),
    PortChanged(String),
    UserChanged(String),
    PassChanged(String),
    ConnectSsh,
    TabPressed(bool),
    FieldFocused(usize),
}

// ---------- Update ----------

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::TabSelected(index) => {
            if index < state.sessions.len() {
                state.active_index = index;
            }
            Task::none()
        }

        Message::CloseTab(index) => {
            if state.sessions.len() <= 1 {
                return Task::none(); // always keep at least one tab
            }
            state.sessions.remove(index);
            if state.active_index >= state.sessions.len() {
                state.active_index = state.sessions.len() - 1;
            }
            Task::none()
        }

        Message::NewSshTab => {
            // Add a fresh Welcome tab so the user can enter SSH details
            state.sessions.push(Session::welcome());
            state.active_index = state.sessions.len() - 1;
            Task::none()
        }

        Message::TerminalInput(bytes) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if let Some(ref sender) = session.sender {
                    let _ = sender.send(bytes);
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
            let bytes = text_str.into_bytes();
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if let Some(ref sender) = session.sender {
                    let _ = sender.send(bytes);
                } else {
                    session.terminal.process_bytes(&bytes);
                }
                session.terminal.clear_preedit();
            }
            Task::none()
        }

        Message::FontLoaded(_) => Task::none(),

        Message::SshMessage(target_index, event) => {
            if let Some(session) = state.sessions.get_mut(target_index) {
                match event {
                    ssh::SshEvent::Connected(sender) => {
                        session.sender = Some(sender);
                    }
                    ssh::SshEvent::Data(data) => {
                        session.terminal.process_bytes(&data);
                    }
                    ssh::SshEvent::Disconnected => {
                        session.sender = None;
                    }
                    ssh::SshEvent::Error(e) => {
                        println!("SSH Error on tab {}: {}", target_index, e);
                        session.sender = None;
                    }
                }
            }
            Task::none()
        }

        Message::TerminalResize(new_rows, new_cols) => {
            for session in state.sessions.iter_mut() {
                session.terminal.resize(new_rows, new_cols);
            }
            Task::none()
        }

        Message::TerminalScroll(delta) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                let max_scroll = session.terminal.history.len();
                if delta > 0.0 {
                    session.terminal.display_offset = std::cmp::min(session.terminal.display_offset + 3, max_scroll);
                } else {
                    session.terminal.display_offset = session.terminal.display_offset.saturating_sub(3);
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

        Message::HostChanged(s) => { state.ssh_host = s; Task::none() }
        Message::PortChanged(s) => { state.ssh_port = s; Task::none() }
        Message::UserChanged(s) => { state.ssh_user = s; Task::none() }
        Message::PassChanged(s) => { state.ssh_pass = s; Task::none() }

        Message::ConnectSsh => {
            let host = state.ssh_host.clone();
            let port: u16 = state.ssh_port.parse().unwrap_or(22);
            let user = state.ssh_user.clone();
            let pass = state.ssh_pass.clone();
            let name = format!("SSH: {}@{}", user, host);

            // Find the current active Welcome session and convert it, or add a new one
            let target_index = state.active_index;
            if let Some(session) = state.sessions.get_mut(target_index) {
                *session = Session::new_terminal(name, session.terminal.rows, session.terminal.cols);
            }

            Task::run(
                ssh::connect_and_subscribe(host, port, user, pass),
                move |event| Message::SshMessage(target_index, event)
            )
        }

        Message::TabPressed(shift) => {
            if shift {
                state.focused_field = if state.focused_field == 0 { 3 } else { state.focused_field - 1 };
            } else {
                state.focused_field = (state.focused_field + 1) % 4;
            }
            let target_id = match state.focused_field {
                0 => state.id_host.clone(),
                1 => state.id_port.clone(),
                2 => state.id_user.clone(),
                _ => state.id_pass.clone(),
            };
            focus(target_id)
        }

        Message::FieldFocused(index) => {
            state.focused_field = index;
            Task::none()
        }
    }
}

// ---------- Subscription ----------

fn subscription(state: &State) -> Subscription<Message> {
    let is_welcome = matches!(
        state.sessions.get(state.active_index).map(|s| &s.kind),
        Some(SessionKind::Welcome)
    );

    // Tab 키 구독 (Welcome 탭에서만 의미 있음, 하지만 항상 등록)
    let tab_sub = event::listen_with(|event, _status, _window| {
        match event {
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                if key == keyboard::Key::Named(keyboard::key::Named::Tab) {
                    Some(Message::TabPressed(modifiers.shift()))
                } else {
                    None
                }
            }
            _ => None
        }
    });

    if is_welcome {
        // Welcome 탭: Tab 포커스 이동만 필요
        Subscription::batch(vec![tab_sub])
    } else {
        // Terminal 탭: IME + 모든 키보드 입력
        let term_sub = event::listen_with(|event, _status, _window| {
            match event {
                iced::Event::InputMethod(ime) => {
                    match ime {
                        input_method::Event::Preedit(text, _) => Some(Message::ImePreedit(text)),
                        input_method::Event::Commit(text) => Some(Message::ImeCommit(text)),
                        _ => None,
                    }
                }
                iced::Event::Keyboard(keyboard::Event::KeyPressed { key, text: key_text, location, .. }) => {
                    let mut bytes = Vec::new();
                    
                    // Numpad 우선 처리: NumLock ON일 때 Iced는 Named(ArrowUp)과 text("8")을 같이 보낼 수 있음
                    let is_numpad = matches!(location, keyboard::Location::Numpad);
                    let numpad_text = if is_numpad {
                        key_text.as_deref().filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || ".-+*/".contains(c)))
                    } else {
                        None
                    };

                    if let Some(s) = numpad_text {
                        bytes.extend_from_slice(s.as_bytes());
                    } else {
                        match &key {
                            keyboard::Key::Named(keyboard::key::Named::Enter) => bytes.extend_from_slice(b"\r"),
                            keyboard::Key::Named(keyboard::key::Named::Backspace) => bytes.push(b'\x08'),
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
                        if bytes.is_empty() {
                            if let Some(t) = key_text {
                                let s = t.as_str();
                                if s.is_ascii() {
                                    bytes.extend_from_slice(s.as_bytes());
                                }
                            }
                        }
                    }
                    if !bytes.is_empty() { Some(Message::TerminalInput(bytes)) } else { None }
                }
                _ => None
            }
        });
        Subscription::batch(vec![term_sub])
    }
}

// ---------- View Helpers ----------

fn hr() -> Element<'static, Message> {
    container(Space::new().width(Length::Fill).height(Length::Fixed(1.0)))
        .style(|_t| iced::widget::container::Style {
            background: Some(Background::Color(Color::from_rgb(0.5, 0.5, 0.5))),
            ..Default::default()
        })
        .into()
}

fn vr() -> Element<'static, Message> {
    container(Space::new().width(Length::Fixed(1.0)).height(Length::Fill))
        .style(|_t| iced::widget::container::Style {
            background: Some(Background::Color(Color::from_rgb(0.5, 0.5, 0.5))),
            ..Default::default()
        })
        .into()
}

// ---------- View ----------

fn view(state: &State) -> Element<'_, Message> {
    let menu_bar = row![
        button(text("Session").size(14)).padding(6),
        button(text("Settings").size(14)).padding(6),
        button(text("View").size(14)).padding(6),
        button(text("Help").size(14)).padding(6),
    ].spacing(5).padding(5);

    // --- Tab bar ---
    let mut tab_bar = row![].spacing(2).padding([4, 8]);
    for (i, session) in state.sessions.iter().enumerate() {
        let is_active = i == state.active_index;
        let label = text(session.name.clone()).size(13);
        let tab_btn = if is_active {
            button(label).padding([4, 10])
        } else {
            button(label).padding([4, 10]).on_press(Message::TabSelected(i))
        };
        tab_bar = tab_bar.push(tab_btn);

        // Close button (X), always shown except for the last remaining tab
        if state.sessions.len() > 1 {
            let close_idx = i;
            tab_bar = tab_bar.push(
                button(text("✕").size(11)).padding([4, 6]).on_press(Message::CloseTab(close_idx))
            );
        }
    }

    // + New Tab button
    tab_bar = tab_bar.push(
        button(text("+").size(16)).padding([4, 8]).on_press(Message::NewSshTab)
    );

    // --- Sidebar ---
    let sidebar = container(
        column![
            text("Sessions").size(18),
            hr(),
            scrollable(
                column![
                    button("New SSH").width(Length::Fill).on_press(Message::NewSshTab),
                ].spacing(10)
            )
            .height(Length::Fill)
        ]
        .spacing(10)
    )
    .padding(10)
    .width(Length::Fixed(200.0))
    .height(Length::Fill);

    // --- Active tab content ---
    let tab_content: Element<'_, Message> = if let Some(session) = state.sessions.get(state.active_index) {
        match session.kind {
            SessionKind::Welcome => {
                container(
                    column![
                        text("SSH Connection Settings").size(24),
                        row![text("Host: "), text_input("IP Address", &state.ssh_host)
                            .id(state.id_host.clone()).on_input(Message::HostChanged)
                            .on_submit(Message::TabPressed(false)).width(200)],
                        row![text("Port: "), text_input("22", &state.ssh_port)
                            .id(state.id_port.clone()).on_input(Message::PortChanged)
                            .on_submit(Message::TabPressed(false)).width(100)],
                        row![text("Username: "), text_input("user", &state.ssh_user)
                            .id(state.id_user.clone()).on_input(Message::UserChanged)
                            .on_submit(Message::TabPressed(false)).width(200)],
                        row![text("Password: "), text_input("pass", &state.ssh_pass)
                            .id(state.id_pass.clone()).on_input(Message::PassChanged)
                            .secure(true).width(200).on_submit(Message::ConnectSsh)],
                        button(text("Connect Now")).padding(10).on_press(Message::ConnectSsh),
                    ].spacing(15)
                ).center(Length::Fill).into()
            }
            SessionKind::Terminal => {
                let hist_len = session.terminal.history.len();
                let offset = session.terminal.display_offset;
                row![
                    container(TerminalView::new(&session.terminal, Message::TerminalScroll, Message::TerminalResize))
                        .width(Length::Fill)
                        .height(Length::Fill),
                    container(
                        vertical_slider(
                            0.0..=(hist_len as f32).max(1.0),
                            offset as f32,
                            |v| Message::TerminalScrollTo(v as usize)
                        )
                        .step(1.0)
                        .style(|_theme, _status| iced::widget::slider::Style {
                            rail: iced::widget::slider::Rail {
                                backgrounds: (
                                    iced::Background::Color(Color::from_rgb(0.25, 0.25, 0.25)),
                                    iced::Background::Color(Color::from_rgb(0.25, 0.25, 0.25)),
                                ),
                                width: 4.0,
                                border: Default::default(),
                            },
                            handle: iced::widget::slider::Handle {
                                shape: iced::widget::slider::HandleShape::Rectangle {
                                    width: 10,
                                    border_radius: 2.0f32.into(),
                                },
                                background: iced::Background::Color(Color::from_rgb(0.6, 0.6, 0.6)),
                                border_width: 0.0,
                                border_color: Color::TRANSPARENT,
                            },
                        })
                    )
                    .width(Length::Fixed(15.0))
                    .height(Length::Fill)
                    .padding(2)
                ]
                .into()
            }
        }
    } else {
        text("No active tab").into()
    };

    let main_content = column![
        tab_bar,
        hr(),
        container(tab_content).width(Length::Fill).height(Length::Fill)
    ]
    .width(Length::Fill)
    .height(Length::Fill);

    let top_level = column![
        menu_bar,
        hr(),
        row![sidebar, vr(), main_content].height(Length::Fill)
    ];

    container(top_level).width(Length::Fill).height(Length::Fill).into()
}
