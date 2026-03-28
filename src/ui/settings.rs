// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::widget::{button, column, container, row, scrollable, text, text_input, Space};
use iced::{Background, Color, Element, Font, Length, font::Weight};

use crate::app::{Message, SettingsTabKind, SettingsToggleKey, State};

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

fn setting_row(label: &'static str, placeholder: &'static str) -> Element<'static, Message> {
    row![
        column![
            text(label).size(13),
            text("Placeholder")
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
    key: SettingsToggleKey,
    checked: bool,
) -> Element<'static, Message> {
    let check_label = if checked { "[x]" } else { "[ ]" };
    let status_label = if checked { "ON" } else { "OFF" };

    row![
        column![
            text(label).size(13),
            text("Checkbox Placeholder")
                .size(11)
                .color(Color::from_rgb(0.58, 0.58, 0.58)),
        ]
        .spacing(2)
        .width(Length::Fill),
        button(
            row![
                text(check_label).size(12),
                text(status_label).size(12).font(Font {
                    weight: Weight::Bold,
                    ..Default::default()
                }),
            ]
            .spacing(8)
            .align_y(iced::Alignment::Center)
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

    let content: Element<'_, Message> = match (tab_kind, category_name) {
        (SettingsTabKind::Preferences, "Common") => column![
            text(title).size(18).font(Font {
                weight: Weight::Bold,
                ..Default::default()
            }),
            text("Adjust application-wide behavior.")
                .size(12)
                .color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Default Connection Profile", "placeholder-default-profile"),
            checkbox_placeholder_row(
                "Auto-reconnect",
                SettingsToggleKey::AutoReconnect,
                state.settings_checkbox_value(SettingsToggleKey::AutoReconnect)
            ),
            setting_row("Global Timeout", "placeholder-timeout-ms"),
        ]
        .spacing(14)
        .into(),
        (SettingsTabKind::Preferences, "SSH") => column![
            text(title).size(18).font(Font {
                weight: Weight::Bold,
                ..Default::default()
            }),
            text("Configure SSH behavior.")
                .size(12)
                .color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Host Key Policy", "placeholder-strict-checking"),
            setting_row("Keep Alive Interval", "placeholder-interval-sec"),
            checkbox_placeholder_row(
                "Use Agent Forwarding",
                SettingsToggleKey::UseAgentForwarding,
                state.settings_checkbox_value(SettingsToggleKey::UseAgentForwarding)
            ),
        ]
        .spacing(14)
        .into(),
        (SettingsTabKind::Preferences, "Telnet") => column![
            text(title).size(18).font(Font {
                weight: Weight::Bold,
                ..Default::default()
            }),
            text("Configure Telnet behavior.")
                .size(12)
                .color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Negotiation Mode", "placeholder-auto"),
            setting_row("Line Ending", "placeholder-CRLF"),
            checkbox_placeholder_row(
                "Echo Locally",
                SettingsToggleKey::EchoLocally,
                state.settings_checkbox_value(SettingsToggleKey::EchoLocally)
            ),
        ]
        .spacing(14)
        .into(),
        (SettingsTabKind::Preferences, "Serial") => column![
            text(title).size(18).font(Font {
                weight: Weight::Bold,
                ..Default::default()
            }),
            text("Configure Serial behavior.")
                .size(12)
                .color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Default Baud Rate", "placeholder-115200"),
            setting_row("Parity", "placeholder-none"),
            checkbox_placeholder_row(
                "Hardware Flow Control",
                SettingsToggleKey::HardwareFlowControl,
                state.settings_checkbox_value(SettingsToggleKey::HardwareFlowControl)
            ),
        ]
        .spacing(14)
        .into(),
        (SettingsTabKind::Preferences, "Local Shell") => column![
            text(title).size(18).font(Font {
                weight: Weight::Bold,
                ..Default::default()
            }),
            text("Configure local shell launch behavior.")
                .size(12)
                .color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Default Shell", "placeholder-pwsh"),
            setting_row("Startup Args", "placeholder-no-logo"),
            checkbox_placeholder_row(
                "Launch In Login Mode",
                SettingsToggleKey::LaunchInLoginMode,
                state.settings_checkbox_value(SettingsToggleKey::LaunchInLoginMode)
            ),
        ]
        .spacing(14)
        .into(),
        (SettingsTabKind::Preferences, "RDP") => column![
            text(title).size(18).font(Font {
                weight: Weight::Bold,
                ..Default::default()
            }),
            text("Configure RDP behavior.")
                .size(12)
                .color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Default Resolution", "placeholder-1280x720"),
            setting_row("Color Depth", "placeholder-32bit"),
            checkbox_placeholder_row(
                "NLA",
                SettingsToggleKey::RdpNla,
                state.settings_checkbox_value(SettingsToggleKey::RdpNla)
            ),
        ]
        .spacing(14)
        .into(),
        (SettingsTabKind::Preferences, "VNC") => column![
            text(title).size(18).font(Font {
                weight: Weight::Bold,
                ..Default::default()
            }),
            text("Configure VNC behavior.")
                .size(12)
                .color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Encoding", "placeholder-tight"),
            setting_row("Compression", "placeholder-medium"),
            checkbox_placeholder_row(
                "Remote Cursor",
                SettingsToggleKey::VncRemoteCursor,
                state.settings_checkbox_value(SettingsToggleKey::VncRemoteCursor)
            ),
        ]
        .spacing(14)
        .into(),
        (SettingsTabKind::Theme, "Theme") => column![
            text(title).size(18).font(Font {
                weight: Weight::Bold,
                ..Default::default()
            }),
            text("Theme options placeholder.")
                .size(12)
                .color(Color::from_rgb(0.65, 0.65, 0.65)),
            Space::new().height(Length::Fixed(10.0)),
            setting_row("Color Theme", "placeholder-dark"),
            setting_row("Terminal Font Family", "placeholder-d2coding"),
            checkbox_placeholder_row(
                "Use Compact Tab Style",
                SettingsToggleKey::CompactTabStyle,
                state.settings_checkbox_value(SettingsToggleKey::CompactTabStyle)
            ),
        ]
        .spacing(14)
        .into(),
        _ => column![
            text(title).size(18).font(Font {
                weight: Weight::Bold,
                ..Default::default()
            }),
            text("No settings available.")
                .size(12)
                .color(Color::from_rgb(0.65, 0.65, 0.65)),
        ]
        .spacing(14)
        .into(),
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
