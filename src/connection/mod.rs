
// SPDX-License-Identifier: MIT OR Apache-2.0

pub mod ssh;
pub mod telnet;
pub mod serial;
pub mod rdp;
pub mod vnc;
pub mod remote_input_policy;

use crate::remote_display::FrameUpdate;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum RemoteInput {
    KeyboardScancode {
        code: u8,
        extended: bool,
        down: bool,
    },
    KeyboardUnicode {
        codepoint: u16,
        down: bool,
    },
    MouseMove {
        x: u16,
        y: u16,
    },
    MouseButton {
        button: RemoteMouseButton,
        down: bool,
    },
    MouseWheel {
        delta: i16,
    },
    MouseHorizontalWheel {
        delta: i16,
    },
}

#[derive(Debug, Clone)]
pub enum RemoteMouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct KeyboardIndicators {
    pub scroll_lock: bool,
    pub num_lock: bool,
    pub caps_lock: bool,
}

pub enum ConnectionEvent {
    Connected(mpsc::UnboundedSender<ConnectionInput>),
    Data(Vec<u8>),
    Frames(Vec<FrameUpdate>),
    Disconnected,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum ConnectionInput {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    SyncKeyboardIndicators(KeyboardIndicators),
    ReleaseAllModifiers,
    RemoteInput(RemoteInput),
}

impl std::fmt::Debug for ConnectionEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connected(_) => write!(f, "Connected"),
            Self::Data(d) => write!(f, "Data({} bytes)", d.len()),
            Self::Frames(frames) => write!(f, "Frames({})", frames.len()),
            Self::Disconnected => write!(f, "Disconnected"),
            Self::Error(e) => write!(f, "Error({})", e),
        }
    }
}

impl Clone for ConnectionEvent {
    fn clone(&self) -> Self {
        match self {
            Self::Connected(s) => Self::Connected(s.clone()),
            Self::Data(d) => Self::Data(d.clone()),
            Self::Frames(frames) => Self::Frames(frames.clone()),
            Self::Disconnected => Self::Disconnected,
            Self::Error(e) => Self::Error(e.clone()),
        }
    }
}


