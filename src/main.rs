use iced::widget::{button, column, container, row, scrollable, text, Space};
use iced::{Background, Color, Element, Length, Task, Subscription};
use iced::advanced::input_method;
use iced::event;
use iced::keyboard;
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

struct State {
    active_tab: usize,
    terminal: TerminalEmulator,
    ssh_sender: Option<mpsc::UnboundedSender<Vec<u8>>>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            active_tab: 0,
            terminal: TerminalEmulator::new(24, 80),
            ssh_sender: None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    TabSelected(usize),
    TerminalInput(Vec<u8>),
    ImePreedit(String),
    ImeCommit(String),
    FontLoaded(Result<(), iced::font::Error>),
    SshMessage(ssh::SshEvent),
}

fn update(state: &mut State, message: Message) -> Task<Message> {
    match message {
        Message::TabSelected(index) => {
            state.active_tab = index;
            Task::none()
        }
        Message::TerminalInput(bytes) => {
            if let Some(ref sender) = state.ssh_sender {
                let _ = sender.send(bytes);
            } else {
                state.terminal.process_bytes(&bytes);
            }
            Task::none()
        }
        Message::ImePreedit(preedit) => {
            state.terminal.ime_preedit = preedit;
            state.terminal.cache.clear();
            Task::none()
        }
        Message::ImeCommit(text) => {
            let bytes = text.into_bytes();
            if let Some(ref sender) = state.ssh_sender {
                let _ = sender.send(bytes);
            } else {
                state.terminal.process_bytes(&bytes);
            }
            state.terminal.clear_preedit();
            Task::none()
        }
        Message::FontLoaded(_) => Task::none(),
        Message::SshMessage(event) => {
            match event {
                ssh::SshEvent::Connected(sender) => {
                    state.ssh_sender = Some(sender);
                }
                ssh::SshEvent::Data(data) => {
                    state.terminal.process_bytes(&data);
                }
                ssh::SshEvent::Disconnected => {
                    state.ssh_sender = None;
                }
                ssh::SshEvent::Error(e) => {
                    println!("SSH Error: {}", e);
                    state.ssh_sender = None;
                }
            }
            Task::none()
        }
    }
}

fn subscription(state: &State) -> Subscription<Message> {
    let mut subs = vec![
        event::listen_with(|event, _status, _window| {
            match event {
                iced::Event::InputMethod(ime) => {
                    match ime {
                        input_method::Event::Preedit(text, _) => Some(Message::ImePreedit(text)),
                        input_method::Event::Commit(text) => Some(Message::ImeCommit(text)),
                        _ => None,
                    }
                }
                iced::Event::Keyboard(keyboard::Event::KeyPressed { key, text: key_text, .. }) => {
                    let mut bytes = Vec::new();
                    match key {
                        keyboard::Key::Named(keyboard::key::Named::Enter) => bytes.extend_from_slice(b"\r\n"),
                        keyboard::Key::Named(keyboard::key::Named::Backspace) => bytes.push(b'\x08'),
                        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => bytes.extend_from_slice(b"\x1b[A"),
                        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => bytes.extend_from_slice(b"\x1b[B"),
                        keyboard::Key::Named(keyboard::key::Named::ArrowRight) => bytes.extend_from_slice(b"\x1b[C"),
                        keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => bytes.extend_from_slice(b"\x1b[D"),
                        keyboard::Key::Named(keyboard::key::Named::Home) => bytes.extend_from_slice(b"\x1b[H"),
                        keyboard::Key::Named(keyboard::key::Named::End) => bytes.extend_from_slice(b"\x1b[F"),
                        keyboard::Key::Named(keyboard::key::Named::Delete) => bytes.extend_from_slice(b"\x1b[3~"),
                        keyboard::Key::Named(keyboard::key::Named::PageUp) => bytes.extend_from_slice(b"\x1b[5~"),
                        keyboard::Key::Named(keyboard::key::Named::PageDown) => bytes.extend_from_slice(b"\x1b[6~"),
                        _ => {}
                    }
                    if bytes.is_empty() {
                        if let Some(t) = key_text {
                            if t.as_str().is_ascii() {
                                bytes.extend_from_slice(t.as_str().as_bytes());
                            }
                        }
                    }
                    if !bytes.is_empty() { Some(Message::TerminalInput(bytes)) } else { None }
                }
                _ => None
            }
        })
    ];

    if state.active_tab == 1 {
        // Iced 0.14 Subscription::run expects a closure that returns a Stream
        subs.push(Subscription::run(|| {
            ssh::connect_and_subscribe(
                "192.168.1.1".to_string(), 
                22, 
                "gth1919".to_string(), 
                "&208Psirns".to_string()
            )
        }).map(Message::SshMessage));
    }

    Subscription::batch(subs)
}

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

fn view(state: &State) -> Element<'_, Message> {
    let menu_bar = row![
        button(text("Session").size(14)).padding(6),
        button(text("Settings").size(14)).padding(6),
        button(text("View").size(14)).padding(6),
        button(text("Help").size(14)).padding(6),
    ].spacing(5).padding(5);

    let sidebar = container(
        column![
            text("Sessions").size(18),
            hr(),
            scrollable(
                column![
                    button("Local Shell").width(Length::Fill).on_press(Message::TabSelected(1)),
                    button("Server A (SSH)").width(Length::Fill).on_press(Message::TabSelected(1)),
                    button("Router (Telnet)").width(Length::Fill).on_press(Message::TabSelected(1)),
                ].spacing(10)
            )
            .height(Length::Fill)
        ]
        .spacing(10)
    )
    .padding(10)
    .width(Length::Fixed(250.0))
    .height(Length::Fill);

    let main_content = column![
        row![
            button("Welcome").padding(8).on_press(Message::TabSelected(0)),
            button("Terminal #1").padding(8).on_press(Message::TabSelected(1)),
        ].spacing(5),
        hr(),
        {
            let tab_content: Element<'_, Message> = if state.active_tab == 0 {
                container(
                    column![
                        text("MobaXterm Clone - Powered by Iced").size(30),
                        text("가장 기초적인 상단 메뉴바와 레이아웃 분할선이 적용되었습니다.").size(16),
                        text("디자인 고도화는 6단계에서 진행됩니다.").size(12),
                    ].spacing(20)
                ).center(Length::Fill).into()
            } else {
                container(TerminalView::new(&state.terminal))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .into()
            };
            container(tab_content).width(Length::Fill).height(Length::Fill)
        }
    ]
    .padding(10)
    .width(Length::Fill)
    .height(Length::Fill);

    let top_level = column![
        menu_bar,
        hr(),
        row![sidebar, vr(), main_content].height(Length::Fill)
    ];

    container(top_level).width(Length::Fill).height(Length::Fill).into()
}
