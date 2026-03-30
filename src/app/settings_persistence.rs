// SPDX-License-Identifier: MIT OR Apache-2.0

use log::{info, warn};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

use super::state::State;

/// Flat struct holding every persistable setting.
/// Uses `skip_serializing_if` so only values that differ from defaults appear in JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsData {
    // ── Bool toggles ────────────────────────────────────────────────────
    #[serde(skip_serializing_if = "is_false")]
    pub auto_reconnect: bool,

    #[serde(skip_serializing_if = "is_false")]
    pub ssh_use_agent_forwarding: bool,

    #[serde(skip_serializing_if = "is_true")]
    pub telnet_echo_locally: bool,

    #[serde(skip_serializing_if = "is_false")]
    pub serial_hardware_flow_control: bool,

    #[serde(skip_serializing_if = "is_true")]
    pub local_launch_in_login_mode: bool,

    #[serde(skip_serializing_if = "is_true")]
    pub rdp_nla: bool,

    #[serde(skip_serializing_if = "is_true")]
    pub rdp_enable_audio: bool,

    #[serde(skip_serializing_if = "is_true")]
    pub rdp_font_smoothing: bool,

    #[serde(skip_serializing_if = "is_true")]
    pub rdp_desktop_composition: bool,

    #[serde(skip_serializing_if = "is_true")]
    pub vnc_remote_cursor: bool,

    #[serde(skip_serializing_if = "is_true")]
    pub vnc_shared_session: bool,

    #[serde(skip_serializing_if = "is_false")]
    pub vnc_view_only: bool,

    #[serde(skip_serializing_if = "is_false")]
    pub theme_compact_tab_style: bool,

    // ── Text settings ───────────────────────────────────────────────────
    #[serde(skip_serializing_if = "is_default_30")]
    pub common_timeout: String,

    #[serde(skip_serializing_if = "is_default_0")]
    pub ssh_keepalive: String,

    #[serde(skip_serializing_if = "is_default_xterm256color")]
    pub ssh_terminal_type: String,

    #[serde(skip_serializing_if = "is_default_crlf")]
    pub telnet_line_ending: String,

    #[serde(skip_serializing_if = "is_default_8")]
    pub serial_data_bits: String,

    #[serde(skip_serializing_if = "is_default_1")]
    pub serial_stop_bits: String,

    #[serde(skip_serializing_if = "is_default_none_str")]
    pub serial_parity: String,

    #[serde(skip_serializing_if = "String::is_empty")]
    pub local_default_shell: String,

    #[serde(skip_serializing_if = "String::is_empty")]
    pub local_startup_args: String,

    #[serde(skip_serializing_if = "is_default_32")]
    pub rdp_color_depth: String,

    #[serde(skip_serializing_if = "is_default_10")]
    pub vnc_timeout: String,
}

// ── skip_serializing_if helpers ──────────────────────────────────────────

fn is_false(v: &bool) -> bool { !v }
fn is_true(v: &bool) -> bool { *v }

fn is_default_30(v: &str) -> bool { v == "30" }
fn is_default_0(v: &str) -> bool { v == "0" }
fn is_default_xterm256color(v: &str) -> bool { v == "xterm-256color" }
fn is_default_crlf(v: &str) -> bool { v == "CRLF" }
fn is_default_8(v: &str) -> bool { v == "8" }
fn is_default_1(v: &str) -> bool { v == "1" }
fn is_default_none_str(v: &str) -> bool { v == "None" }
fn is_default_32(v: &str) -> bool { v == "32" }
fn is_default_10(v: &str) -> bool { v == "10" }

// ── Default ──────────────────────────────────────────────────────────────

impl Default for SettingsData {
    fn default() -> Self {
        Self {
            auto_reconnect: false,
            ssh_use_agent_forwarding: false,
            telnet_echo_locally: true,
            serial_hardware_flow_control: false,
            local_launch_in_login_mode: true,
            rdp_nla: true,
            rdp_enable_audio: true,
            rdp_font_smoothing: true,
            rdp_desktop_composition: true,
            vnc_remote_cursor: true,
            vnc_shared_session: true,
            vnc_view_only: false,
            theme_compact_tab_style: false,
            common_timeout: "30".to_string(),
            ssh_keepalive: "0".to_string(),
            ssh_terminal_type: "xterm-256color".to_string(),
            telnet_line_ending: "CRLF".to_string(),
            serial_data_bits: "8".to_string(),
            serial_stop_bits: "1".to_string(),
            serial_parity: "None".to_string(),
            local_default_shell: String::new(),
            local_startup_args: String::new(),
            rdp_color_depth: "32".to_string(),
            vnc_timeout: "10".to_string(),
        }
    }
}

// ── Conversion State <-> SettingsData ────────────────────────────────────

impl SettingsData {
    /// Extract persistable settings from application state.
    pub fn from_state(state: &State) -> Self {
        Self {
            auto_reconnect: state.settings_auto_reconnect,
            ssh_use_agent_forwarding: state.settings_ssh_use_agent_forwarding,
            telnet_echo_locally: state.settings_telnet_echo_locally,
            serial_hardware_flow_control: state.settings_serial_hardware_flow_control,
            local_launch_in_login_mode: state.settings_local_launch_in_login_mode,
            rdp_nla: state.settings_rdp_nla,
            rdp_enable_audio: state.settings_rdp_enable_audio,
            rdp_font_smoothing: state.settings_rdp_font_smoothing,
            rdp_desktop_composition: state.settings_rdp_desktop_composition,
            vnc_remote_cursor: state.settings_vnc_remote_cursor,
            vnc_shared_session: state.settings_vnc_shared_session,
            vnc_view_only: state.settings_vnc_view_only,
            theme_compact_tab_style: state.settings_theme_compact_tab_style,
            common_timeout: state.settings_common_timeout.clone(),
            ssh_keepalive: state.settings_ssh_keepalive.clone(),
            ssh_terminal_type: state.settings_ssh_terminal_type.clone(),
            telnet_line_ending: state.settings_telnet_line_ending.clone(),
            serial_data_bits: state.settings_serial_data_bits.clone(),
            serial_stop_bits: state.settings_serial_stop_bits.clone(),
            serial_parity: state.settings_serial_parity.clone(),
            local_default_shell: state.settings_local_default_shell.clone(),
            local_startup_args: state.settings_local_startup_args.clone(),
            rdp_color_depth: state.settings_rdp_color_depth.clone(),
            vnc_timeout: state.settings_vnc_timeout.clone(),
        }
    }

    /// Apply loaded settings onto application state.
    pub fn apply_to(&self, state: &mut State) {
        state.settings_auto_reconnect = self.auto_reconnect;
        state.settings_ssh_use_agent_forwarding = self.ssh_use_agent_forwarding;
        state.settings_telnet_echo_locally = self.telnet_echo_locally;
        state.settings_serial_hardware_flow_control = self.serial_hardware_flow_control;
        state.settings_local_launch_in_login_mode = self.local_launch_in_login_mode;
        state.settings_rdp_nla = self.rdp_nla;
        state.settings_rdp_enable_audio = self.rdp_enable_audio;
        state.settings_rdp_font_smoothing = self.rdp_font_smoothing;
        state.settings_rdp_desktop_composition = self.rdp_desktop_composition;
        state.settings_vnc_remote_cursor = self.vnc_remote_cursor;
        state.settings_vnc_shared_session = self.vnc_shared_session;
        state.settings_vnc_view_only = self.vnc_view_only;
        state.settings_theme_compact_tab_style = self.theme_compact_tab_style;
        state.settings_common_timeout = self.common_timeout.clone();
        state.settings_ssh_keepalive = self.ssh_keepalive.clone();
        state.settings_ssh_terminal_type = self.ssh_terminal_type.clone();
        state.settings_telnet_line_ending = self.telnet_line_ending.clone();
        state.settings_serial_data_bits = self.serial_data_bits.clone();
        state.settings_serial_stop_bits = self.serial_stop_bits.clone();
        state.settings_serial_parity = self.serial_parity.clone();
        state.settings_local_default_shell = self.local_default_shell.clone();
        state.settings_local_startup_args = self.local_startup_args.clone();
        state.settings_rdp_color_depth = self.rdp_color_depth.clone();
        state.settings_vnc_timeout = self.vnc_timeout.clone();
    }
}

// ── File I/O ─────────────────────────────────────────────────────────────

/// Return the path to the settings JSON file (next to the executable).
pub fn settings_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            return dir.join("kterm_settings.json");
        }
    }
    PathBuf::from("kterm_settings.json")
}

/// Load settings from disk. Returns `SettingsData::default()` if the file
/// does not exist or cannot be parsed (logs a warning).
pub fn load_settings() -> SettingsData {
    let path = settings_path();
    load_settings_from(&path)
}

fn load_settings_from(path: &Path) -> SettingsData {
    if !path.exists() {
        info!("[SETTINGS] no settings file found at {}, using defaults", path.display());
        return SettingsData::default();
    }
    match std::fs::read_to_string(path) {
        Ok(contents) => match serde_json::from_str::<SettingsData>(&contents) {
            Ok(data) => {
                info!("[SETTINGS] loaded settings from {}", path.display());
                data
            }
            Err(e) => {
                warn!("[SETTINGS] failed to parse {}: {}; using defaults", path.display(), e);
                SettingsData::default()
            }
        },
        Err(e) => {
            warn!("[SETTINGS] failed to read {}: {}; using defaults", path.display(), e);
            SettingsData::default()
        }
    }
}

/// Save settings to disk. Only fields that differ from defaults will appear in
/// the JSON output.
pub fn save_settings(state: &State) {
    let data = SettingsData::from_state(state);
    let path = settings_path();
    save_settings_to(&data, &path);
}

fn save_settings_to(data: &SettingsData, path: &Path) {
    match serde_json::to_string_pretty(data) {
        Ok(json) => {
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            match std::fs::write(path, &json) {
                Ok(()) => info!("[SETTINGS] saved settings to {}", path.display()),
                Err(e) => warn!("[SETTINGS] failed to write {}: {}", path.display(), e),
            }
        }
        Err(e) => warn!("[SETTINGS] failed to serialize settings: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_serializes_to_empty_json() {
        let data = SettingsData::default();
        let json = serde_json::to_string(&data).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn only_changed_fields_appear() {
        let mut data = SettingsData::default();
        data.rdp_nla = false;
        data.ssh_terminal_type = "vt100".to_string();
        let json = serde_json::to_string_pretty(&data).unwrap();
        assert!(json.contains("rdp_nla"));
        assert!(json.contains("ssh_terminal_type"));
        assert!(!json.contains("auto_reconnect"));
        assert!(!json.contains("common_timeout"));
    }

    #[test]
    fn roundtrip() {
        let mut data = SettingsData::default();
        data.vnc_view_only = true;
        data.serial_parity = "Even".to_string();
        let json = serde_json::to_string(&data).unwrap();
        let loaded: SettingsData = serde_json::from_str(&json).unwrap();
        assert!(loaded.vnc_view_only);
        assert_eq!(loaded.serial_parity, "Even");
        // Non-changed fields should still be defaults
        assert!(loaded.rdp_nla);
        assert_eq!(loaded.common_timeout, "30");
    }

    #[test]
    fn load_nonexistent_returns_default() {
        let data = load_settings_from(Path::new("__nonexistent_test_file__.json"));
        assert_eq!(data.common_timeout, "30");
    }
}
