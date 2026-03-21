pub mod ssh;
pub mod telnet;

use tokio::sync::mpsc;

pub enum ConnectionEvent {
    Connected(mpsc::UnboundedSender<ConnectionInput>),
    Data(Vec<u8>),
    Disconnected,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum ConnectionInput {
    Data(Vec<u8>),
    Resize { cols: u16, rows: u16 },
}

impl std::fmt::Debug for ConnectionEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connected(_) => write!(f, "Connected"),
            Self::Data(d) => write!(f, "Data({} bytes)", d.len()),
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
            Self::Disconnected => Self::Disconnected,
            Self::Error(e) => Self::Error(e.clone()),
        }
    }
}

