use iced::widget::{button, column, container, row, scrollable, text, vertical_slider, Space, text_input, Id, mouse_area, stack};
use iced::{Background, Color, Element, Length, Task, Subscription, event, keyboard, advanced::input_method, Font, font::Weight, mouse};
use iced::widget::operation::focus;
use iced::window;
mod terminal;
mod ssh;
mod platform;
use terminal::{TerminalEmulator, TerminalView};
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
}

struct Session {
    name: String,
    kind: SessionKind,
    terminal: TerminalEmulator,
    sender: Option<mpsc::UnboundedSender<ssh::SshInput>>,
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
    ssh_host: String,
    ssh_port: String,
    ssh_user: String,
    ssh_pass: String,
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
    SshMessage(usize, ssh::SshEvent),
    TerminalResize(usize, usize),
    TerminalScroll(f32),
    TerminalScrollTo(usize),
    HostChanged(String),
    PortChanged(String),
    UserChanged(String),
    PassChanged(String),
    ConnectSsh,
    ConnectLocal,
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
            state.sessions.push(Session::welcome());
            state.active_index = state.sessions.len() - 1;
            Task::none()
        }
        Message::TerminalInput(bytes) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if let Some(ref sender) = session.sender { let _ = sender.send(ssh::SshInput::Data(bytes)); }
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
                if let Some(ref sender) = session.sender { let _ = sender.send(ssh::SshInput::Data(bytes)); }
                else { session.terminal.process_bytes(&bytes); }
                session.terminal.clear_preedit();
            }
            Task::none()
        }
        Message::FontLoaded(_) => Task::none(),
        Message::SshMessage(target_index, event) => {
            if let Some(session) = state.sessions.get_mut(target_index) {
                match event {
                    ssh::SshEvent::Connected(sender) => {
                        session.sender = Some(sender.clone());
                        let _ = sender.send(ssh::SshInput::Resize { cols: session.terminal.cols as u16, rows: session.terminal.rows as u16 });
                    }
                    ssh::SshEvent::Data(data) => {
                        if !data.is_empty() {
                            session.terminal.process_bytes(&data);
                            let responses: Vec<Vec<u8>> = session.terminal.pending_responses.drain(..).collect();
                            for resp in responses {
                                if let Some(ref sender) = session.sender { let _ = sender.send(ssh::SshInput::Data(resp)); }
                            }
                        }
                    }
                    ssh::SshEvent::Disconnected => { session.sender = None; }
                    ssh::SshEvent::Error(_e) => { session.sender = None; }
                }
            }
            Task::none()
        }
        Message::TerminalResize(new_rows, new_cols) => {
            if let Some(session) = state.sessions.get_mut(state.active_index) {
                if session.terminal.rows != new_rows || session.terminal.cols != new_cols {
                    session.terminal.resize(new_rows, new_cols);
                    if let Some(ref sender) = session.sender { let _ = sender.send(ssh::SshInput::Resize { cols: new_cols as u16, rows: new_rows as u16 }); }
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
        Message::ConnectSsh => {
            let host = state.ssh_host.clone();
            let port: u16 = state.ssh_port.parse().unwrap_or(22);
            let user = state.ssh_user.clone();
            let pass = state.ssh_pass.clone();
            let name = format!("SSH: {}@{}", user, host);
            let target_index = state.active_index;
            if let Some(session) = state.sessions.get_mut(target_index) {
                *session = Session::new_terminal(name, session.terminal.rows, session.terminal.cols);
            }
            Task::run(ssh::connect_and_subscribe(host, port, user, pass), move |event| Message::SshMessage(target_index, event))
        }
        Message::ConnectLocal => {
            let name = "Local: PowerShell".to_string();
            let target_index = state.active_index;
            if let Some(session) = state.sessions.get_mut(target_index) {
                *session = Session::new_terminal(name, session.terminal.rows, session.terminal.cols);
            }
            Task::run(platform::windows::spawn_local_shell(), move |event| Message::SshMessage(target_index, event))
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
        Message::ToggleMenu(menu) => { if state.dummy_menu_open == Some(menu) { state.dummy_menu_open = None; } else { state.dummy_menu_open = Some(menu); } Task::none() }
    }
}

fn subscription(state: &State) -> Subscription<Message> {
    let is_welcome = matches!(state.sessions.get(state.active_index).map(|s| &s.kind), Some(SessionKind::Welcome));
    
    let mouse_sub = event::listen_with(|event, _status, _window| {
        match event {
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => Some(Message::ResizeFinished),
            _ => None
        }
    });

    let tab_sub = event::listen_with(|event, _status, _window| {
        match event {
            iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
                if key == keyboard::Key::Named(keyboard::key::Named::Tab) { Some(Message::TabPressed(modifiers.shift())) } else { None }
            }
            _ => None
        }
    });

    if is_welcome {
        let mut subs = vec![tab_sub, mouse_sub];
        if state.window_id.is_none() { subs.push(window::open_events().map(Message::WindowIdCaptured)); }
        Subscription::batch(subs)
    } else {
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
                    let is_numpad = matches!(location, keyboard::Location::Numpad);
                    let numpad_text = if is_numpad { key_text.as_deref().filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || ".-+*/".contains(c))) } else { None };
                    if let Some(s) = numpad_text { bytes.extend_from_slice(s.as_bytes()); }
                    else {
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
                        if bytes.is_empty() { if let Some(t) = key_text { let s = t.as_str(); if s.is_ascii() { bytes.extend_from_slice(s.as_bytes()); } } }
                    }
                    if !bytes.is_empty() { Some(Message::TerminalInput(bytes)) } else { None }
                }
                _ => None
            }
        });
        let mut subs = vec![term_sub, mouse_sub];
        if state.window_id.is_none() { subs.push(window::open_events().map(Message::WindowIdCaptured)); }
        Subscription::batch(subs)
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
                .on_press(Message::WindowDrag).on_release(Message::ToggleMenu("")),
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

    let mut tab_bar = row![].spacing(2).padding([2, 10]);
    for (i, session) in state.sessions.iter().enumerate() {
        let is_active = i == state.active_index;
        let label = text(session.name.clone()).size(12);
        let tab_btn = if is_active {
            button(label).padding([5, 12]).style(|_, _| button::Style {
                background: Some(Background::Color(Color::from_rgb(0.2, 0.4, 0.6))), text_color: Color::WHITE,
                border: iced::Border { radius: iced::border::Radius { top_left: 4.0, top_right: 4.0, ..Default::default() }, ..Default::default() }, ..button::Style::default()
            })
        } else {
            button(label).padding([5, 12]).style(|_t, s| {
                let mut style = button::secondary(_t, s);
                style.background = Some(Background::Color(if matches!(s, button::Status::Hovered) { Color::from_rgb(0.25, 0.25, 0.25) } else { Color::from_rgb(0.15, 0.15, 0.15) }));
                style.border.radius = iced::border::Radius { top_left: 4.0, top_right: 4.0, ..Default::default() };
                style
            }).on_press(Message::TabSelected(i))
        };
        let tab_item = if state.sessions.len() > 1 { row![tab_btn, button(text("✕").size(10)).padding([5, 8]).style(button::secondary).on_press(Message::CloseTab(i))].spacing(0) } else { row![tab_btn] };
        tab_bar = tab_bar.push(tab_item);
    }
    tab_bar = tab_bar.push(button(text("+").size(14)).padding([4, 10]).style(button::secondary).on_press(Message::NewSshTab));

    let sidebar = container(column![text("SESSIONS").size(12).font(Font { weight: Weight::Bold, ..Default::default() }), hr(), scrollable(column![button(text("+ New SSH").size(13)).width(Length::Fill).style(button::secondary).on_press(Message::NewSshTab)].spacing(8)).height(Length::Fill)].spacing(10)).padding(10).width(Length::Fixed(180.0)).style(|_| container::Style { background: Some(Background::Color(Color::from_rgb(0.1, 0.1, 0.1))), ..Default::default() });

    let tab_content: Element<'_, Message> = if let Some(session) = state.sessions.get(state.active_index) {
        match session.kind {
            SessionKind::Welcome => container(column![text("Connection Launcher").size(24).font(Font { weight: Weight::Bold, ..Default::default() }), column![text("SSH Connection").size(18).font(Font { weight: Weight::Bold, ..Default::default() }), row![text("Host: "), text_input("IP Address", &state.ssh_host).id(state.id_host.clone()).on_input(Message::HostChanged).on_submit(Message::TabPressed(false)).width(200)], row![text("Port: "), text_input("22", &state.ssh_port).id(state.id_port.clone()).on_input(Message::PortChanged).on_submit(Message::TabPressed(false)).width(100)], row![text("Username: "), text_input("user", &state.ssh_user).id(state.id_user.clone()).on_input(Message::UserChanged).on_submit(Message::TabPressed(false)).width(200)], row![text("Password: "), text_input("pass", &state.ssh_pass).id(state.id_pass.clone()).on_input(Message::PassChanged).secure(true).width(200).on_submit(Message::ConnectSsh)], button(text("Connect SSH")).padding(10).on_press(Message::ConnectSsh)].spacing(10), hr(), column![text("Local System").size(18).font(Font { weight: Weight::Bold, ..Default::default() }), button(text("Launch PowerShell")).padding(10).on_press(Message::ConnectLocal)].spacing(10)].spacing(20).width(Length::Fixed(400.0))).center(Length::Fill).into(),
            SessionKind::Terminal => {
                let hist_len = session.terminal.history.len();
                let offset = session.terminal.display_offset;
                row![container(TerminalView::new(&session.terminal, Message::TerminalScroll, Message::TerminalResize)).width(Length::Fill).height(Length::Fill), container(vertical_slider(0.0..=(hist_len as f32).max(1.0), offset as f32, |v| Message::TerminalScrollTo(v as usize)).step(1.0).style(|_, _| iced::widget::slider::Style { rail: iced::widget::slider::Rail { backgrounds: (iced::Background::Color(Color::from_rgb(0.1, 0.1, 0.1)), iced::Background::Color(Color::from_rgb(0.1, 0.1, 0.1))), width: 4.0, border: Default::default() }, handle: iced::widget::slider::Handle { shape: iced::widget::slider::HandleShape::Rectangle { width: 10, border_radius: 2.0f32.into() }, background: iced::Background::Color(Color::from_rgb(0.4, 0.4, 0.4)), border_width: 0.0, border_color: Color::TRANSPARENT } })).width(Length::Fixed(12.0)).height(Length::Fill).padding(2)].into()
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

    let final_layout: Element<'_, Message> = if let Some(menu) = state.dummy_menu_open {
        if !menu.is_empty() {
            let dropdown = match menu {
                "Session" => container(column![button(text("New SSH Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::NewSshTab), button(text("New Local Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::ConnectLocal), hr(), button(text("Close Current Tab").size(12)).width(Length::Fill).style(button::text).on_press(Message::CloseTab(state.active_index))].spacing(2).width(Length::Fixed(160.0))),
                "Settings" => container(column![button(text("App Settings").size(12)).width(Length::Fill).style(button::text), button(text("Terminal Theme").size(12)).width(Length::Fill).style(button::text), button(text("Font Settings").size(12)).width(Length::Fill).style(button::text)].spacing(2).width(Length::Fixed(160.0))),
                _ => container(column![button(text(format!("{} Option 1", menu)).size(12)).width(Length::Fill).style(button::text), button(text(format!("{} Option 2", menu)).size(12)).width(Length::Fill).style(button::text)].spacing(2).width(Length::Fixed(160.0))),
            }.padding(4).style(|_| container::Style { background: Some(Background::Color(Color::from_rgb(0.18, 0.18, 0.18))), border: iced::Border { width: 1.0, color: Color::from_rgb(0.3, 0.3, 0.3), radius: 4.0f32.into() }, ..Default::default() });
            let h_offset: f32 = match menu { "Session" => 95.0, "Settings" => 95.0 + 72.0, "View" => 95.0 + 72.0 * 2.0, "Help" => 95.0 + 72.0 * 3.0, _ => 95.0 };
            let overlay_layer: Element<'_, Message> = column![Space::new().height(Length::Fixed(35.0)), row![Space::new().width(Length::Fixed(h_offset)), dropdown]].into();
            stack![final_content, overlay_layer].into()
        } else { final_content }
    } else { final_content };

    container(final_layout).width(Length::Fill).height(Length::Fill).style(|_| container::Style { background: Some(Background::Color(Color::from_rgb(0.08, 0.08, 0.08))), text_color: Some(Color::WHITE), ..Default::default() }).into()
}
