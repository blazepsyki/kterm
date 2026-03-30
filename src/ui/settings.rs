// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Color, Element, Font, Length, font::Weight};

use crate::app::{Message, SettingsTabKind, SettingsTextKey, SettingsToggleKey, State};

fn get_settings_categories(tab_kind: SettingsTabKind) -> Vec<&'static str> {
    match tab_kind {
        SettingsTabKind::Preferences => {
            vec!["Common", "SSH", "Telnet", "Serial", "Local Shell", "RDP", "VNC"]
        }
        SettingsTabKind::Theme => vec!["Theme"],
    }
}

fn settings_header(tab_kind: SettingsTabKind) -> &'static str {
    match tab_kind {
        SettingsTabKind::Preferences => "Preferences",
        SettingsTabKind::Theme => "Theme Settings",
    }
}

pub fn render_settings_sidebar(
    tab_kind: SettingsTabKind,
    selected_index: usize,
) -> Element<'static, Message> {
    let categories = get_settings_categories(tab_kind);

    let mut category_buttons = column![].spacing(2);

    for (idx, category) in categories.iter().enumerate() {
        let is_selected = idx == selected_index;

        category_buttons = category_buttons.push(
            button(container(text(*category).size(13)).width(Length::Fill).padding([10, 0]))
                .width(Length::Fill)
                .padding([6, 8])
                .style(move |_t, s| {
                    let mut st = button::text(_t, s);
                    if is_selected {
                        st.background = Some(Background::Color(Color::from_rgb(0.2, 0.4, 0.6)));
                        st.text_color = Color::WHITE;
                    } else {
                        st.background = Some(Background::Color(if matches!(s, button::Status::Hovered) {
                            Color::from_rgb(0.15, 0.15, 0.15)
                        } else {
                            Color::TRANSPARENT
                        }));
                        st.text_color = Color::from_rgb(0.7, 0.7, 0.7);
                    }
                    st
                })
                .on_press(Message::SettingsCategorySelected(idx)),
        );
    }

    container(scrollable(
        column![
            text(settings_header(tab_kind))
                .size(12)
                .font(Font {
                    weight: Weight::Bold,
                    ..Default::default()
                }),
            Space::new().height(Length::Fixed(12.0)),
            category_buttons,
        ]
        .spacing(8)
        .padding(10),
    ))
    .width(Length::Fixed(180.0))
    .height(Length::Fill)
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgb(0.11, 0.11, 0.11))),
        ..Default::default()
    })
    .into()
}

fn setting_text_row(
    label: &'static str,
    description: &'static str,
    value: String,
    key: SettingsTextKey,
) -> Element<'static, Message> {
    row![
        column![
            text(label).size(13),
            text(description)
                .size(11)
                .color(Color::from_rgb(0.58, 0.58, 0.58)),
        ]
        .spacing(2)
        .width(Length::Fill),
        container(
            text_input("", &value)
                .on_input(move |v| Message::SettingsTextChanged(key, v))
                .padding([6, 10])
                .width(Length::Fixed(240.0))
        )
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.10, 0.10, 0.10))),
            border: iced::Border {
                width: 1.0,
                color: Color::from_rgb(0.24, 0.24, 0.24),
                radius: 6.0.into(),
            },
            ..Default::default()
        })
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

fn setting_row_readonly(label: &'static str, placeholder: &'static str) -> Element<'static, Message> {
    row![
        column![
            text(label).size(13),
            text("Not yet configurable")
                .size(11)
                .color(Color::from_rgb(0.58, 0.58, 0.58)),
        ]
        .spacing(2)
        .width(Length::Fill),
        container(
            text_input("", placeholder)
                .padding([6, 10])
                .width(Length::Fixed(240.0))
        )
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.10, 0.10, 0.10))),
            border: iced::Border {
                width: 1.0,
                color: Color::from_rgb(0.24, 0.24, 0.24),
                radius: 6.0.into(),
            },
            ..Default::default()
        })
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

fn checkbox_placeholder_row(
    label: &'static str,
    description: &'static str,
    key: SettingsToggleKey,
    checked: bool,
) -> Element<'static, Message> {
    let status_label = if checked { "ON" } else { "OFF" };

    row![
        column![
            text(label).size(13),
            text(description)
                .size(11)
                .color(Color::from_rgb(0.58, 0.58, 0.58)),
        ]
        .spacing(2)
        .width(Length::Fill),
        button(
            container(
                text(status_label).size(12).font(Font {
                    weight: Weight::Bold,
                    ..Default::default()
                })
            )
            .width(Length::Fill)
            .center_x(Length::Fill)
            .center_y(Length::Fill)
        )
        .width(Length::Fixed(90.0))
        .padding([6, 8])
        .style(move |_t, s| {
            let mut st = button::secondary(_t, s);
            st.background = Some(Background::Color(if checked {
                Color::from_rgb(0.18, 0.34, 0.22)
            } else {
                Color::from_rgb(0.18, 0.18, 0.18)
            }));
            st.text_color = Color::from_rgb(0.92, 0.92, 0.92);
            st.border = iced::Border {
                width: 1.0,
                color: if checked {
                    Color::from_rgb(0.34, 0.54, 0.38)
                } else {
                    Color::from_rgb(0.35, 0.35, 0.35)
                },
                radius: 6.0.into(),
            };
            st
        })
        .on_press(Message::ToggleSettingsCheckbox(key))
    ]
    .align_y(iced::Alignment::Center)
    .into()
}

fn setting_text_value_row(
    state: &State,
    label: &'static str,
    description: &'static str,
    key: SettingsTextKey,
) -> Element<'static, Message> {
    setting_text_row(
        label,
        description,
        state.settings_text_value(key).to_owned(),
        key,
    )
}

fn setting_toggle_row(
    state: &State,
    label: &'static str,
    description: &'static str,
    key: SettingsToggleKey,
) -> Element<'static, Message> {
    checkbox_placeholder_row(label, description, key, state.settings_checkbox_value(key))
}

fn settings_section(
    title: String,
    subtitle: &'static str,
    rows: Vec<Element<'static, Message>>,
) -> Element<'static, Message> {
    let mut section = column![
        text(title).size(18).font(Font {
            weight: Weight::Bold,
            ..Default::default()
        }),
        text(subtitle)
            .size(12)
            .color(Color::from_rgb(0.65, 0.65, 0.65)),
        Space::new().height(Length::Fixed(10.0)),
    ]
    .spacing(14);

    for row in rows {
        section = section.push(row);
    }

    section.into()
}

fn render_common_settings(state: &State, title: String) -> Element<'static, Message> {
    settings_section(
        title,
        "Adjust application-wide behavior.",
        vec![
            setting_text_value_row(
                state,
                "Connection Timeout (sec)",
                "TCP connect timeout in seconds",
                SettingsTextKey::CommonTimeout,
            ),
            setting_toggle_row(
                state,
                "Auto-reconnect",
                "Reconnect automatically when the session disconnects",
                SettingsToggleKey::AutoReconnect,
            ),
        ],
    )
}

fn render_ssh_settings(state: &State, title: String) -> Element<'static, Message> {
    settings_section(
        title,
        "Configure SSH connection options.",
        vec![
            setting_text_value_row(
                state,
                "Keep Alive Interval (sec)",
                "Seconds between keepalive packets (0 = disabled)",
                SettingsTextKey::SshKeepAliveInterval,
            ),
            setting_text_value_row(
                state,
                "Terminal Type",
                "PTY terminal type string for remote shell",
                SettingsTextKey::SshTerminalType,
            ),
            setting_toggle_row(
                state,
                "Use Agent Forwarding",
                "Forward your local SSH agent to the remote host",
                SettingsToggleKey::UseAgentForwarding,
            ),
        ],
    )
}

fn render_telnet_settings(state: &State, title: String) -> Element<'static, Message> {
    settings_section(
        title,
        "Configure Telnet connection options.",
        vec![
            setting_text_value_row(
                state,
                "Line Ending",
                "Outgoing line ending: CRLF, CR, or LF",
                SettingsTextKey::TelnetLineEnding,
            ),
            setting_toggle_row(
                state,
                "Echo Locally",
                "Show typed characters locally before server echo",
                SettingsToggleKey::EchoLocally,
            ),
        ],
    )
}

fn render_serial_settings(state: &State, title: String) -> Element<'static, Message> {
    settings_section(
        title,
        "Configure serial port parameters.",
        vec![
            setting_text_value_row(
                state,
                "Data Bits",
                "Number of data bits: 5, 6, 7, 8",
                SettingsTextKey::SerialDataBits,
            ),
            setting_text_value_row(
                state,
                "Stop Bits",
                "Number of stop bits: 1, 2",
                SettingsTextKey::SerialStopBits,
            ),
            setting_text_value_row(
                state,
                "Parity",
                "Parity check: None, Odd, Even",
                SettingsTextKey::SerialParity,
            ),
            setting_toggle_row(
                state,
                "Hardware Flow Control",
                "Use RTS/CTS hardware flow control",
                SettingsToggleKey::HardwareFlowControl,
            ),
        ],
    )
}

fn render_local_shell_settings(state: &State, title: String) -> Element<'static, Message> {
    settings_section(
        title,
        "Configure local shell launch behavior.",
        vec![
            setting_text_value_row(
                state,
                "Default Shell",
                "Path to shell executable (empty = auto-detect)",
                SettingsTextKey::LocalDefaultShell,
            ),
            setting_text_value_row(
                state,
                "Startup Arguments",
                "Extra arguments passed to shell",
                SettingsTextKey::LocalStartupArgs,
            ),
            setting_toggle_row(
                state,
                "Launch In Login Mode",
                "Start the shell as a login shell when supported",
                SettingsToggleKey::LaunchInLoginMode,
            ),
        ],
    )
}

fn render_rdp_settings(state: &State, title: String) -> Element<'static, Message> {
    settings_section(
        title,
        "Configure Remote Desktop Protocol options.",
        vec![
            setting_text_value_row(
                state,
                "Color Depth (bpp)",
                "Bits per pixel: 16, 24, 32",
                SettingsTextKey::RdpColorDepth,
            ),
            setting_toggle_row(
                state,
                "NLA (Network Level Auth)",
                "Use CredSSP / NLA during authentication",
                SettingsToggleKey::RdpNla,
            ),
            setting_toggle_row(
                state,
                "Enable Audio Playback",
                "Play remote session audio on this machine",
                SettingsToggleKey::RdpEnableAudio,
            ),
            setting_toggle_row(
                state,
                "Font Smoothing",
                "Request ClearType/font smoothing in the remote session",
                SettingsToggleKey::RdpFontSmoothing,
            ),
            setting_toggle_row(
                state,
                "Desktop Composition",
                "Enable desktop composition effects when available",
                SettingsToggleKey::RdpDesktopComposition,
            ),
        ],
    )
}

fn render_vnc_settings(state: &State, title: String) -> Element<'static, Message> {
    settings_section(
        title,
        "Configure VNC connection options.",
        vec![
            setting_text_value_row(
                state,
                "Connection Timeout (sec)",
                "TCP connect timeout in seconds",
                SettingsTextKey::VncTimeout,
            ),
            setting_toggle_row(
                state,
                "Remote Cursor",
                "Use server-provided cursor shape updates",
                SettingsToggleKey::VncRemoteCursor,
            ),
            setting_toggle_row(
                state,
                "Shared Session",
                "Allow sharing the same VNC desktop with other clients",
                SettingsToggleKey::VncSharedSession,
            ),
            setting_toggle_row(
                state,
                "View Only",
                "Disable keyboard and mouse input to the remote host",
                SettingsToggleKey::VncViewOnly,
            ),
        ],
    )
}

fn render_theme_settings(state: &State, title: String) -> Element<'static, Message> {
    settings_section(
        title,
        "Theme options placeholder.",
        vec![
            setting_row_readonly("Color Theme", "dark"),
            setting_row_readonly("Terminal Font Family", "D2Coding"),
            setting_toggle_row(
                state,
                "Use Compact Tab Style",
                "Use denser spacing for the tab strip",
                SettingsToggleKey::CompactTabStyle,
            ),
        ],
    )
}

fn render_unknown_settings(title: String) -> Element<'static, Message> {
    column![
        text(title).size(18).font(Font {
            weight: Weight::Bold,
            ..Default::default()
        }),
        text("No settings available.")
            .size(12)
            .color(Color::from_rgb(0.65, 0.65, 0.65)),
    ]
    .spacing(14)
    .into()
}

pub fn render_settings_panel(
    state: &State,
    tab_kind: SettingsTabKind,
    selected_index: usize,
) -> Element<'static, Message> {
    let categories = get_settings_categories(tab_kind);
    let category_name = categories.get(selected_index).copied().unwrap_or("Unknown");

    let title = match tab_kind {
        SettingsTabKind::Preferences => format!("{} Settings", category_name),
        SettingsTabKind::Theme => "Theme Settings".to_string(),
    };

    let content: Element<'static, Message> = match (tab_kind, category_name) {
        (SettingsTabKind::Preferences, "Common") => render_common_settings(state, title),
        (SettingsTabKind::Preferences, "SSH") => render_ssh_settings(state, title),
        (SettingsTabKind::Preferences, "Telnet") => render_telnet_settings(state, title),
        (SettingsTabKind::Preferences, "Serial") => render_serial_settings(state, title),
        (SettingsTabKind::Preferences, "Local Shell") => render_local_shell_settings(state, title),
        (SettingsTabKind::Preferences, "RDP") => render_rdp_settings(state, title),
        (SettingsTabKind::Preferences, "VNC") => render_vnc_settings(state, title),
        (SettingsTabKind::Theme, "Theme") => render_theme_settings(state, title),
        _ => render_unknown_settings(title),
    };

    container(scrollable(content).height(Length::Fill))
        .width(Length::Fill)
        .height(Length::Fill)
        .padding(30)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.12, 0.12, 0.12))),
            ..Default::default()
        })
        .into()
}
