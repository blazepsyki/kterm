
// SPDX-License-Identifier: MIT OR Apache-2.0

pub mod ssh;
pub mod telnet;
pub mod serial;
pub mod rdp;

use crate::remote_display::FrameUpdate;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum RdpInput {
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
        button: RdpMouseButton,
        down: bool,
    },
    MouseWheel {
        delta: i16,
    },
}

#[derive(Debug, Clone)]
pub enum RdpMouseButton {
    Left,
    Right,
    Middle,
}

pub enum ConnectionEvent {
    Connected(mpsc::UnboundedSender<ConnectionInput>),
    Data(Vec<u8>),
    Frame(FrameUpdate),
    Disconnected,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum ConnectionInput {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
    RdpInput(RdpInput),
}

impl std::fmt::Debug for ConnectionEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connected(_) => write!(f, "Connected"),
            Self::Data(d) => write!(f, "Data({} bytes)", d.len()),
            Self::Frame(_) => write!(f, "Frame"),
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
            Self::Frame(frame) => Self::Frame(frame.clone()),
            Self::Disconnected => Self::Disconnected,
            Self::Error(e) => Self::Error(e.clone()),
        }
    }
}


