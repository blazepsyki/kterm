// SPDX-License-Identifier: MIT OR Apache-2.0

use tokio::sync::mpsc;

use crate::connection;
use crate::remote_display::RemoteDisplayState;
use crate::terminal::TerminalEmulator;

#[derive(Debug)]
pub enum SessionKind {
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
pub enum RemoteDisplayProtocol {
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
pub struct LocalShellOption {
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
}

pub struct Session {
    pub id: u64,
    pub name: String,
    pub kind: SessionKind,
    pub terminal: TerminalEmulator,
    pub remote_display: Option<RemoteDisplayState>,
    pub remote_display_protocol: Option<RemoteDisplayProtocol>,
    pub sender: Option<mpsc::UnboundedSender<connection::ConnectionInput>>,
    pub settings_tab_kind: Option<SettingsTabKind>,
}

impl Session {
    pub fn welcome(id: u64) -> Self {
        Self {
            id,
            name: "Welcome".to_string(),
            kind: SessionKind::Welcome,
            terminal: TerminalEmulator::new(24, 80),
            remote_display: None,
            remote_display_protocol: None,
            sender: None,
            settings_tab_kind: None,
        }
    }

    pub fn new_terminal(id: u64, name: String, rows: usize, cols: usize) -> Self {
        Self {
            id,
            name,
            kind: SessionKind::Terminal,
            terminal: TerminalEmulator::new(rows, cols),
            remote_display: None,
            remote_display_protocol: None,
            sender: None,
            settings_tab_kind: None,
        }
    }

    pub fn new_remote_display(
        id: u64,
        name: String,
        width: u16,
        height: u16,
        remote_display_protocol: RemoteDisplayProtocol,
    ) -> Self {
        Self {
            id,
            name,
            kind: SessionKind::RemoteDisplay,
            terminal: TerminalEmulator::new(24, 80),
            remote_display: Some(RemoteDisplayState::new(width, height)),
            remote_display_protocol: Some(remote_display_protocol),
            sender: None,
            settings_tab_kind: None,
        }
    }

    pub fn is_rdp_display(&self) -> bool {
        matches!(
            self.remote_display_protocol.as_ref(),
            Some(RemoteDisplayProtocol::Rdp)
        )
    }

    pub fn new_settings(id: u64, tab_kind: SettingsTabKind) -> Self {
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
            remote_display_protocol: None,
            sender: None,
            settings_tab_kind: Some(tab_kind),
        }
    }
}
