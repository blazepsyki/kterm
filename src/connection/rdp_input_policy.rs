// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::keyboard;

use crate::connection::{KeyboardIndicators, RdpInput};

pub enum RoutedKeyEvent {
    Ignore,
    SyncIndicators,
    Input(RdpInput),
}

pub fn route_key_pressed(
    key: &keyboard::Key,
    text: Option<&str>,
    physical_key: &keyboard::key::Physical,
) -> RoutedKeyEvent {
    if is_lock_key_event(physical_key, key) {
        return RoutedKeyEvent::Ignore;
    }

    if let Some((code, extended)) = map_key_to_rdp_scancode(physical_key) {
        return RoutedKeyEvent::Input(RdpInput::KeyboardScancode {
            code,
            extended,
            down: true,
        });
    }

    if let Some(ch) = text.and_then(|value| value.chars().next()) {
        let codepoint = ch as u32;
        if codepoint <= 0xFFFF {
            return RoutedKeyEvent::Input(RdpInput::KeyboardUnicode {
                codepoint: codepoint as u16,
                down: true,
            });
        }
    }

    RoutedKeyEvent::Ignore
}

pub fn route_key_released(
    key: &keyboard::Key,
    physical_key: &keyboard::key::Physical,
) -> RoutedKeyEvent {
    if is_lock_key_event(physical_key, key) {
        return RoutedKeyEvent::SyncIndicators;
    }

    if let Some((code, extended)) = map_key_to_rdp_scancode(physical_key) {
        return RoutedKeyEvent::Input(RdpInput::KeyboardScancode {
            code,
            extended,
            down: false,
        });
    }

    if let keyboard::Key::Character(value) = key {
        if let Some(ch) = value.chars().next() {
            let codepoint = ch as u32;
            if codepoint <= 0xFFFF {
                return RoutedKeyEvent::Input(RdpInput::KeyboardUnicode {
                    codepoint: codepoint as u16,
                    down: false,
                });
            }
        }
    }

    RoutedKeyEvent::Ignore
}

pub fn current_keyboard_indicators() -> KeyboardIndicators {
    #[cfg(windows)]
    {
        unsafe extern "system" {
            fn GetKeyState(nVirtKey: i32) -> i16;
        }

        KeyboardIndicators {
            num_lock: (unsafe { GetKeyState(0x90) } & 1) != 0,
            caps_lock: (unsafe { GetKeyState(0x14) } & 1) != 0,
            scroll_lock: (unsafe { GetKeyState(0x91) } & 1) != 0,
        }
    }

    #[cfg(not(windows))]
    {
        KeyboardIndicators::default()
    }
}

pub fn is_numlock_conflict_scancode(code: u8) -> bool {
    matches!(code, 0x47 | 0x48 | 0x49 | 0x4B | 0x4C | 0x4D | 0x4F | 0x50 | 0x51 | 0x52 | 0x53)
}

pub fn is_remote_secure_attention_shortcut(
    physical_key: &keyboard::key::Physical,
    modifiers: keyboard::Modifiers,
) -> bool {
    modifiers.control() && modifiers.alt() && is_remote_secure_attention_key(physical_key)
}

pub fn is_remote_secure_attention_key(physical_key: &keyboard::key::Physical) -> bool {
    matches!(physical_key, keyboard::key::Physical::Code(keyboard::key::Code::End))
}

pub fn remote_secure_attention_inputs(down: bool) -> Vec<RdpInput> {
    vec![RdpInput::KeyboardScancode {
        code: 0x53,
        extended: true,
        down,
    }]
}

pub fn unicode_inputs_for_text(text: &str) -> Vec<RdpInput> {
    let mut inputs = Vec::new();

    for ch in text.chars() {
        let codepoint = ch as u32;
        if codepoint > 0xFFFF {
            continue;
        }

        let codepoint = codepoint as u16;
        inputs.push(RdpInput::KeyboardUnicode {
            codepoint,
            down: true,
        });
        inputs.push(RdpInput::KeyboardUnicode {
            codepoint,
            down: false,
        });
    }

    inputs
}

fn is_lock_key_event(physical_key: &keyboard::key::Physical, key: &keyboard::Key) -> bool {
    use keyboard::key::{Code, Named, Physical};

    match physical_key {
        Physical::Code(Code::CapsLock | Code::NumLock | Code::ScrollLock) => true,
        _ => matches!(
            key,
            keyboard::Key::Named(Named::CapsLock)
                | keyboard::Key::Named(Named::NumLock)
                | keyboard::Key::Named(Named::ScrollLock)
        ),
    }
}

fn map_key_to_rdp_scancode(physical_key: &keyboard::key::Physical) -> Option<(u8, bool)> {
    use keyboard::key::{Code, Physical};

    let Physical::Code(code) = physical_key else {
        return None;
    };

    match code {
        Code::Backquote => Some((0x29, false)),
        Code::Digit1 => Some((0x02, false)),
        Code::Digit2 => Some((0x03, false)),
        Code::Digit3 => Some((0x04, false)),
        Code::Digit4 => Some((0x05, false)),
        Code::Digit5 => Some((0x06, false)),
        Code::Digit6 => Some((0x07, false)),
        Code::Digit7 => Some((0x08, false)),
        Code::Digit8 => Some((0x09, false)),
        Code::Digit9 => Some((0x0A, false)),
        Code::Digit0 => Some((0x0B, false)),
        Code::Minus => Some((0x0C, false)),
        Code::Equal => Some((0x0D, false)),
        Code::KeyQ => Some((0x10, false)),
        Code::KeyW => Some((0x11, false)),
        Code::KeyE => Some((0x12, false)),
        Code::KeyR => Some((0x13, false)),
        Code::KeyT => Some((0x14, false)),
        Code::KeyY => Some((0x15, false)),
        Code::KeyU => Some((0x16, false)),
        Code::KeyI => Some((0x17, false)),
        Code::KeyO => Some((0x18, false)),
        Code::KeyP => Some((0x19, false)),
        Code::BracketLeft => Some((0x1A, false)),
        Code::BracketRight => Some((0x1B, false)),
        Code::KeyA => Some((0x1E, false)),
        Code::KeyS => Some((0x1F, false)),
        Code::KeyD => Some((0x20, false)),
        Code::KeyF => Some((0x21, false)),
        Code::KeyG => Some((0x22, false)),
        Code::KeyH => Some((0x23, false)),
        Code::KeyJ => Some((0x24, false)),
        Code::KeyK => Some((0x25, false)),
        Code::KeyL => Some((0x26, false)),
        Code::Semicolon => Some((0x27, false)),
        Code::Quote => Some((0x28, false)),
        Code::Backslash => Some((0x2B, false)),
        Code::KeyZ => Some((0x2C, false)),
        Code::KeyX => Some((0x2D, false)),
        Code::KeyC => Some((0x2E, false)),
        Code::KeyV => Some((0x2F, false)),
        Code::KeyB => Some((0x30, false)),
        Code::KeyN => Some((0x31, false)),
        Code::KeyM => Some((0x32, false)),
        Code::Comma => Some((0x33, false)),
        Code::Period => Some((0x34, false)),
        Code::Slash => Some((0x35, false)),
        Code::Space => Some((0x39, false)),
        Code::Numpad0 => Some((0x52, false)),
        Code::Numpad1 => Some((0x4F, false)),
        Code::Numpad2 => Some((0x50, false)),
        Code::Numpad3 => Some((0x51, false)),
        Code::Numpad4 => Some((0x4B, false)),
        Code::Numpad5 => Some((0x4C, false)),
        Code::Numpad6 => Some((0x4D, false)),
        Code::Numpad7 => Some((0x47, false)),
        Code::Numpad8 => Some((0x48, false)),
        Code::Numpad9 => Some((0x49, false)),
        Code::NumpadDecimal | Code::NumpadComma => Some((0x53, false)),
        Code::NumpadAdd => Some((0x4E, false)),
        Code::NumpadSubtract => Some((0x4A, false)),
        Code::NumpadMultiply => Some((0x37, false)),
        Code::NumpadDivide => Some((0x35, true)),
        Code::NumpadEnter => Some((0x1C, true)),
        Code::ShiftLeft => Some((0x2A, false)),
        Code::ShiftRight => Some((0x36, false)),
        Code::ControlLeft => Some((0x1D, false)),
        Code::ControlRight => Some((0x1D, true)),
        Code::AltLeft => Some((0x38, false)),
        Code::AltRight => Some((0x38, true)),
        Code::SuperLeft => Some((0x5B, true)),
        Code::SuperRight => Some((0x5C, true)),
        Code::Enter => Some((0x1C, false)),
        Code::Backspace => Some((0x0E, false)),
        Code::Tab => Some((0x0F, false)),
        Code::Escape => Some((0x01, false)),
        Code::ArrowUp => Some((0x48, true)),
        Code::ArrowDown => Some((0x50, true)),
        Code::ArrowLeft => Some((0x4B, true)),
        Code::ArrowRight => Some((0x4D, true)),
        Code::Home => Some((0x47, true)),
        Code::End => Some((0x4F, true)),
        Code::PageUp => Some((0x49, true)),
        Code::PageDown => Some((0x51, true)),
        Code::Insert => Some((0x52, true)),
        Code::Delete => Some((0x53, true)),
        Code::F1 => Some((0x3B, false)),
        Code::F2 => Some((0x3C, false)),
        Code::F3 => Some((0x3D, false)),
        Code::F4 => Some((0x3E, false)),
        Code::F5 => Some((0x3F, false)),
        Code::F6 => Some((0x40, false)),
        Code::F7 => Some((0x41, false)),
        Code::F8 => Some((0x42, false)),
        Code::F9 => Some((0x43, false)),
        Code::F10 => Some((0x44, false)),
        Code::F11 => Some((0x57, false)),
        Code::F12 => Some((0x58, false)),
        _ => None,
    }
}