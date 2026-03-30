// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::advanced::input_method;
use iced::{event, keyboard, mouse, window, Subscription};

use crate::connection;
use crate::connection::remote_input_policy::{
    is_remote_secure_attention_key, is_remote_secure_attention_shortcut, route_key_pressed,
    route_key_released, RoutedKeyEvent,
};

use super::{Message, SessionKind, State};

pub fn subscription(state: &State) -> Subscription<Message> {
    let active_kind = state.sessions.get(state.active_index).map(|s| &s.kind);
    let is_welcome = matches!(active_kind, Some(SessionKind::Welcome));
    let is_terminal = matches!(active_kind, Some(SessionKind::Terminal));

    let mouse_sub = event::listen_with(|event, _status, _window| match event {
        iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            Some(Message::ResizeFinished)
        }
        iced::Event::Window(window::Event::Resized(size)) => {
            Some(Message::WindowSizeChanged(size.width, size.height))
        }
        iced::Event::Window(window::Event::Focused) => {
            Some(Message::SyncRemoteKeyboardIndicators)
        }
        iced::Event::Window(window::Event::Unfocused) => Some(Message::ReleaseRemoteModifiers),
        _ => None,
    });

    let tab_sub = event::listen_with(|event, _status, _window| match event {
        iced::Event::Keyboard(keyboard::Event::KeyPressed { key, modifiers, .. }) => {
            if key == keyboard::Key::Named(keyboard::key::Named::Tab) {
                Some(Message::TabPressed(modifiers.shift()))
            } else if key == keyboard::Key::Named(keyboard::key::Named::Escape) {
                Some(Message::TabPressed(false))
            } else {
                None
            }
        }
        _ => None,
    });

    let menu_close_sub = if matches!(state.dummy_menu_open, Some(menu) if !menu.is_empty()) {
        event::listen_with(|event, status, _window| match event {
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if matches!(status, event::Status::Ignored) =>
            {
                Some(Message::CloseMenuDeferred)
            }
            _ => None,
        })
    } else {
        Subscription::none()
    };

    if is_welcome {
        let mut subs = vec![tab_sub, mouse_sub, menu_close_sub];
        if state.window_id.is_none() {
            subs.push(window::open_events().map(Message::WindowIdCaptured));
        }
        Subscription::batch(subs)
    } else if is_terminal {
        let term_sub = event::listen_with(|event, _status, _window| match event {
            iced::Event::InputMethod(ime) => match ime {
                input_method::Event::Preedit(text, _) => Some(Message::ImePreedit(text)),
                input_method::Event::Commit(text) => Some(Message::ImeCommit(text)),
                _ => None,
            },
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                text: key_text,
                location,
                modifiers,
                ..
            }) => {
                let ctrl = modifiers.control();

                if ctrl
                    && matches!(key, keyboard::Key::Character(ref c) if c == "c" || c == "C" || c == "v" || c == "V")
                {
                    return Some(Message::TryHandleKey(key.clone(), modifiers));
                }
                if matches!(key, keyboard::Key::Named(keyboard::key::Named::Escape)) {
                    return Some(Message::TryHandleKey(key.clone(), modifiers));
                }

                let mut bytes = Vec::new();
                let is_numpad = matches!(location, keyboard::Location::Numpad);
                let numpad_text = if is_numpad {
                    key_text.as_deref().filter(|s| {
                        !s.is_empty() && s.chars().all(|c| c.is_ascii_digit() || ".-+*/".contains(c))
                    })
                } else {
                    None
                };

                if let Some(s) = numpad_text {
                    bytes.extend_from_slice(s.as_bytes());
                } else {
                    match &key {
                        keyboard::Key::Named(keyboard::key::Named::Enter) => {
                            bytes.extend_from_slice(b"\r")
                        }
                        keyboard::Key::Named(keyboard::key::Named::Backspace) => bytes.push(b'\x7f'),
                        keyboard::Key::Named(keyboard::key::Named::Tab) => bytes.extend_from_slice(b"\t"),
                        keyboard::Key::Named(keyboard::key::Named::ArrowUp) => {
                            bytes.extend_from_slice(b"\x1b[A")
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowDown) => {
                            bytes.extend_from_slice(b"\x1b[B")
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowRight) => {
                            bytes.extend_from_slice(b"\x1b[C")
                        }
                        keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => {
                            bytes.extend_from_slice(b"\x1b[D")
                        }
                        keyboard::Key::Named(keyboard::key::Named::Home) => {
                            bytes.extend_from_slice(b"\x1b[H")
                        }
                        keyboard::Key::Named(keyboard::key::Named::End) => {
                            bytes.extend_from_slice(b"\x1b[F")
                        }
                        keyboard::Key::Named(keyboard::key::Named::Delete) => {
                            bytes.extend_from_slice(b"\x1b[3~")
                        }
                        keyboard::Key::Named(keyboard::key::Named::PageUp) => {
                            bytes.extend_from_slice(b"\x1b[5~")
                        }
                        keyboard::Key::Named(keyboard::key::Named::PageDown) => {
                            bytes.extend_from_slice(b"\x1b[6~")
                        }
                        keyboard::Key::Named(keyboard::key::Named::Escape) => {
                            bytes.extend_from_slice(b"\x1b")
                        }
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

                if !bytes.is_empty() {
                    Some(Message::TerminalInput(bytes))
                } else {
                    None
                }
            }
            _ => None,
        });

        let mut subs = vec![term_sub, mouse_sub, menu_close_sub];
        if state.window_id.is_none() {
            subs.push(window::open_events().map(Message::WindowIdCaptured));
        }
        Subscription::batch(subs)
    } else {
        let remote_sub = event::listen_with(|event, status, _window| match event {
            iced::Event::InputMethod(ime) => match ime {
                input_method::Event::Commit(text) => Some(Message::ImeCommit(text)),
                _ => None,
            },
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                text,
                physical_key,
                modifiers,
                ..
            }) => {
                if is_remote_secure_attention_shortcut(&physical_key, modifiers) {
                    return Some(Message::RemoteSecureAttention(true));
                }

                match route_key_pressed(&key, text.as_deref(), &physical_key) {
                    RoutedKeyEvent::Ignore => None,
                    RoutedKeyEvent::SyncIndicators => Some(Message::SyncRemoteKeyboardIndicators),
                    RoutedKeyEvent::Input(input) => Some(Message::RemoteDisplayInput(input)),
                }
            }
            iced::Event::Keyboard(keyboard::Event::KeyReleased {
                key,
                physical_key,
                modifiers,
                ..
            }) => {
                if is_remote_secure_attention_shortcut(&physical_key, modifiers)
                    || (is_remote_secure_attention_key(&physical_key)
                        && modifiers.control()
                        && modifiers.alt())
                {
                    return Some(Message::RemoteSecureAttention(false));
                }

                match route_key_released(&key, &physical_key) {
                    RoutedKeyEvent::Ignore => None,
                    RoutedKeyEvent::SyncIndicators => Some(Message::SyncRemoteKeyboardIndicators),
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
                    mouse::Button::Left => Some(Message::RemoteDisplayInput(
                        connection::RemoteInput::MouseButton {
                            button: connection::RemoteMouseButton::Left,
                            down: true,
                        },
                    )),
                    mouse::Button::Right => Some(Message::RemoteDisplayInput(
                        connection::RemoteInput::MouseButton {
                            button: connection::RemoteMouseButton::Right,
                            down: true,
                        },
                    )),
                    mouse::Button::Middle => Some(Message::RemoteDisplayInput(
                        connection::RemoteInput::MouseButton {
                            button: connection::RemoteMouseButton::Middle,
                            down: true,
                        },
                    )),
                    _ => None,
                }
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(button))
                if status == event::Status::Ignored =>
            {
                match button {
                    mouse::Button::Left => Some(Message::RemoteDisplayInput(
                        connection::RemoteInput::MouseButton {
                            button: connection::RemoteMouseButton::Left,
                            down: false,
                        },
                    )),
                    mouse::Button::Right => Some(Message::RemoteDisplayInput(
                        connection::RemoteInput::MouseButton {
                            button: connection::RemoteMouseButton::Right,
                            down: false,
                        },
                    )),
                    mouse::Button::Middle => Some(Message::RemoteDisplayInput(
                        connection::RemoteInput::MouseButton {
                            button: connection::RemoteMouseButton::Middle,
                            down: false,
                        },
                    )),
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
                    Some(Message::RemoteDisplayInput(
                        connection::RemoteInput::MouseHorizontalWheel {
                            delta: hx_step.max(i16::MIN as f32).min(i16::MAX as f32) as i16,
                        },
                    ))
                } else {
                    None
                }
            }
            _ => None,
        });

        let mut subs = vec![mouse_sub, menu_close_sub, remote_sub];
        if state.window_id.is_none() {
            subs.push(window::open_events().map(Message::WindowIdCaptured));
        }
        Subscription::batch(subs)
    }
}
