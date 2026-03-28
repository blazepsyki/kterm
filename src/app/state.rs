// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::widget::Id;
use iced::window;

use super::local_shell::detect_local_shells;

use super::model::{
    LocalShellOption, ProtocolMode, Session, SettingsToggleKey,
};

pub struct State {
    pub sessions: Vec<Session>,
    pub next_session_id: u64,
    pub active_index: usize,
    pub welcome_protocol: ProtocolMode,
    pub ssh_host: String,
    pub ssh_port: String,
    pub ssh_user: String,
    pub ssh_pass: String,
    pub serial_port: String,
    pub serial_baud: String,
    pub rdp_host: String,
    pub rdp_port: String,
    pub rdp_user: String,
    pub rdp_pass: String,
    pub selected_rdp_resolution_index: usize,
    pub vnc_host: String,
    pub vnc_port: String,
    pub vnc_pass: String,
    pub local_shells: Vec<LocalShellOption>,
    pub selected_local_shell: usize,
    pub ssh_id_host: Id,
    pub ssh_id_port: Id,
    pub ssh_id_user: Id,
    pub ssh_id_pass: Id,
    pub telnet_id_host: Id,
    pub telnet_id_port: Id,
    pub serial_id_port: Id,
    pub serial_id_baud: Id,
    pub rdp_id_host: Id,
    pub rdp_id_port: Id,
    pub rdp_id_user: Id,
    pub rdp_id_pass: Id,
    pub rdp_id_resolution: Id,
    pub vnc_id_host: Id,
    pub vnc_id_port: Id,
    pub vnc_id_pass: Id,
    pub focused_field: usize,
    pub window_id: Option<window::Id>,
    pub dummy_menu_open: Option<&'static str>,
    pub resizing_direction: Option<window::Direction>,
    pub window_size: (f32, f32),
    pub settings_selected_category: usize,
    pub settings_auto_reconnect: bool,
    pub settings_ssh_use_agent_forwarding: bool,
    pub settings_telnet_echo_locally: bool,
    pub settings_serial_hardware_flow_control: bool,
    pub settings_local_launch_in_login_mode: bool,
    pub settings_rdp_nla: bool,
    pub settings_vnc_remote_cursor: bool,
    pub settings_theme_compact_tab_style: bool,
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
            selected_rdp_resolution_index: 1,
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
    pub fn current_field_ids(&self) -> Vec<Id> {
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

    pub fn settings_checkbox_value(&self, key: SettingsToggleKey) -> bool {
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

    pub fn toggle_settings_checkbox(&mut self, key: SettingsToggleKey) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_toggle_flips_value() {
        let mut state = State::default();
        assert!(!state.settings_checkbox_value(SettingsToggleKey::AutoReconnect));
        state.toggle_settings_checkbox(SettingsToggleKey::AutoReconnect);
        assert!(state.settings_checkbox_value(SettingsToggleKey::AutoReconnect));
    }

    #[test]
    fn rdp_fields_include_resolution_selector() {
        let mut state = State::default();
        state.welcome_protocol = ProtocolMode::Rdp;
        let fields = state.current_field_ids();
        assert_eq!(fields.len(), 5);
    }
}
