// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::widget::{
    button, column, container, mouse_area, pick_list, row, scrollable, shader, stack, text,
    text_input, vertical_slider, Space,
};
use iced::{Background, Color, Element, Font, Length, font::Weight, mouse, window};

use crate::app::{Message, ProtocolMode, SessionKind, SettingsTabKind, State};
use crate::remote_display;
use crate::terminal::TerminalView;

fn hr() -> Element<'static, Message> {
    container(
        Space::new()
            .width(Length::Fill)
            .height(Length::Fixed(1.0)),
    )
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(Color::from_rgb(0.5, 0.5, 0.5))),
        ..Default::default()
    })
    .into()
}

fn vr() -> Element<'static, Message> {
    container(
        Space::new()
            .width(Length::Fixed(1.0))
            .height(Length::Fill),
    )
    .style(|_| iced::widget::container::Style {
        background: Some(Background::Color(Color::from_rgb(0.5, 0.5, 0.5))),
        ..Default::default()
    })
    .into()
}

pub fn view(state: &State) -> Element<'_, Message> {
    let active_session_name = state
        .sessions
        .get(state.active_index)
        .map(|s| s.name.clone())
        .unwrap_or_else(|| "kterm".to_string());
    let menu_bar = row![
        button(text("Session ▾").size(12))
            .padding([4, 8])
            .style(button::text)
            .on_press(Message::ToggleMenu("Session")),
        button(text("Settings ▾").size(12))
            .padding([4, 8])
            .style(button::text)
            .on_press(Message::ToggleMenu("Settings")),
        button(text("View ▾").size(12))
            .padding([4, 8])
            .style(button::text)
            .on_press(Message::ToggleMenu("View")),
        button(text("Help ▾").size(12))
            .padding([4, 8])
            .style(button::text)
            .on_press(Message::ToggleMenu("Help")),
    ]
    .spacing(2)
    .align_y(iced::Alignment::Center);

    let title_bar = container(
        row![
            container(
                text(" ◈ kterm").size(14).font(Font {
                    weight: Weight::Bold,
                    ..Default::default()
                })
            )
            .padding([0, 15])
            .center_y(Length::Fill),
            menu_bar,
            mouse_area(
                container(text(active_session_name).size(12))
                    .width(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill)
            )
            .on_press(Message::WindowDrag)
            .on_release(Message::CloseMenu),
            row![
                button(
                    container(text("—").size(12))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                )
                .width(Length::Fixed(46.0))
                .height(Length::Fill)
                .style(button::text)
                .on_press(Message::MinimizeWindow),
                button(
                    container(text("▢").size(14))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                )
                .width(Length::Fixed(46.0))
                .height(Length::Fill)
                .style(button::text)
                .on_press(Message::MaximizeWindow),
                button(
                    container(text("✕").size(14))
                        .center_x(Length::Fill)
                        .center_y(Length::Fill)
                )
                .width(Length::Fixed(46.0))
                .height(Length::Fill)
                .style(|t, s| {
                    let mut style = button::text(t, s);
                    if matches!(s, button::Status::Hovered) {
                        style.background = Some(Background::Color(Color::from_rgb(0.7, 0.15, 0.15)));
                    }
                    style
                })
                .on_press(Message::CloseWindow),
            ]
            .height(Length::Fill)
        ]
        .height(Length::Fixed(35.0))
        .align_y(iced::Alignment::Center),
    )
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgb(0.12, 0.12, 0.12))),
        ..Default::default()
    });

    let mut tab_bar = row![].spacing(0).padding(0);
    for (i, session) in state.sessions.iter().enumerate() {
        let is_active = i == state.active_index;
        let tab_height = 30.0;
        let tab_bg = if is_active {
            Color::from_rgb(0.08, 0.08, 0.08)
        } else {
            Color::from_rgb(0.12, 0.12, 0.12)
        };
        let border_color = if is_active {
            Color::from_rgb(0.35, 0.35, 0.35)
        } else {
            Color::from_rgb(0.22, 0.22, 0.22)
        };

        let label_btn = button(
            container(text(session.name.clone()).size(12))
                .height(Length::Fill)
                .center_y(Length::Fill),
        )
        .height(Length::Fixed(tab_height))
        .padding([0, 12])
        .style(move |_t, _s| button::Style {
            background: Some(Background::Color(Color::TRANSPARENT)),
            text_color: if is_active {
                Color::WHITE
            } else {
                Color::from_rgb(0.6, 0.6, 0.6)
            },
            ..Default::default()
        })
        .on_press(Message::TabSelected(i));

        let tab_item: Element<'_, Message> = if state.sessions.len() > 1 {
            let close_btn = button(
                container(text("×").size(13))
                    .width(Length::Fill)
                    .height(Length::Fill)
                    .center_x(Length::Fill)
                    .center_y(Length::Fill),
            )
            .width(Length::Fixed(24.0))
            .height(Length::Fixed(tab_height))
            .padding(0)
            .style(move |_t, s| button::Style {
                background: Some(Background::Color(Color::TRANSPARENT)),
                text_color: if matches!(s, button::Status::Hovered) {
                    Color::from_rgb(0.9, 0.4, 0.4)
                } else {
                    Color::from_rgb(0.45, 0.45, 0.45)
                },
                ..Default::default()
            })
            .on_press(Message::CloseTab(i));

            container(
                row![label_btn, close_btn]
                    .height(Length::Fixed(tab_height))
                    .align_y(iced::Alignment::Center),
            )
            .style(move |_| container::Style {
                background: Some(Background::Color(tab_bg)),
                border: iced::Border {
                    radius: iced::border::Radius {
                        top_left: 6.0,
                        top_right: 6.0,
                        ..Default::default()
                    },
                    width: 1.0,
                    color: border_color,
                },
                ..Default::default()
            })
            .into()
        } else {
            container(label_btn)
                .style(move |_| container::Style {
                    background: Some(Background::Color(tab_bg)),
                    border: iced::Border {
                        radius: iced::border::Radius {
                            top_left: 6.0,
                            top_right: 6.0,
                            ..Default::default()
                        },
                        width: 1.0,
                        color: border_color,
                    },
                    ..Default::default()
                })
                .into()
        };

        tab_bar = tab_bar.push(tab_item);
    }
    tab_bar = tab_bar.push(
        button(text("+").size(14))
            .padding([4, 8])
            .style(|_t, s| {
                let mut style = button::text(_t, s);
                if matches!(s, button::Status::Hovered) {
                    style.background = Some(Background::Color(Color::from_rgb(0.2, 0.2, 0.2)));
                }
                style.text_color = Color::from_rgb(0.6, 0.6, 0.6);
                style.border.radius = 4.0.into();
                style
            })
            .on_press(Message::NewSshTab),
    );

    let sidebar = container(
        column![
            text("SESSIONS").size(12).font(Font {
                weight: Weight::Bold,
                ..Default::default()
            }),
            hr(),
            scrollable(
                column![button(text("+ New SSH").size(13))
                    .width(Length::Fill)
                    .style(button::secondary)
                    .on_press(Message::NewSshTab)]
                .spacing(8)
            )
            .height(Length::Fill)
        ]
        .spacing(10),
    )
    .padding(10)
    .width(Length::Fixed(180.0))
    .style(|_| container::Style {
        background: Some(Background::Color(Color::from_rgb(0.1, 0.1, 0.1))),
        ..Default::default()
    });

    let tab_content: Element<'_, Message> = if let Some(session) = state.sessions.get(state.active_index) {
        match session.kind {
            SessionKind::Welcome => {
                let protocol_btn = |mode: ProtocolMode, label: &str, current: &ProtocolMode| {
                    let is_active = mode == *current;
                    button(container(text(label.to_string()).size(15)).center_x(Length::Fill))
                        .width(Length::Fixed(110.0))
                        .padding([8, 0])
                        .style(move |_t, s| {
                            let mut st = button::secondary(_t, s);
                            if is_active {
                                st.background = Some(Background::Color(Color::from_rgb(0.2, 0.4, 0.6)));
                                st.text_color = Color::WHITE;
                            } else {
                                st.background = Some(Background::Color(if matches!(s, button::Status::Hovered) {
                                    Color::from_rgb(0.18, 0.18, 0.18)
                                } else {
                                    Color::from_rgb(0.12, 0.12, 0.12)
                                }));
                                st.text_color = Color::from_rgb(0.7, 0.7, 0.7);
                            }
                            st.border.radius = 4.0.into();
                            st
                        })
                        .on_press(Message::SelectProtocol(mode.clone()))
                };

                let protocol_tabs = row![
                    protocol_btn(ProtocolMode::Ssh, "SSH", &state.welcome_protocol),
                    protocol_btn(ProtocolMode::Telnet, "Telnet", &state.welcome_protocol),
                    protocol_btn(ProtocolMode::Serial, "Serial", &state.welcome_protocol),
                    protocol_btn(ProtocolMode::Local, "Local Shell", &state.welcome_protocol),
                    protocol_btn(ProtocolMode::Rdp, "RDP", &state.welcome_protocol),
                    protocol_btn(ProtocolMode::Vnc, "VNC", &state.welcome_protocol),
                ]
                .spacing(10);

                let form_content: Element<'_, Message> = match state.welcome_protocol {
                    ProtocolMode::Ssh => column![
                        text("SSH Connection").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        row![text("Host: ").width(100), text_input("IP Address", &state.ssh_host).id(state.ssh_id_host.clone()).on_input(Message::HostChanged).width(300)],
                        row![text("Port: ").width(100), text_input("22", &state.ssh_port).id(state.ssh_id_port.clone()).on_input(Message::PortChanged).width(150)],
                        row![text("Username: ").width(100), text_input("user", &state.ssh_user).id(state.ssh_id_user.clone()).on_input(Message::UserChanged).width(300)],
                        row![text("Password: ").width(100), text_input("pass", &state.ssh_pass).id(state.ssh_id_pass.clone()).on_input(Message::PassChanged).secure(true).width(300).on_submit(Message::ConnectSsh)],
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Connect")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectSsh)
                    ].spacing(15).into(),
                    ProtocolMode::Telnet => column![
                        text("Telnet Connection").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        row![text("Host: ").width(100), text_input("IP Address", &state.ssh_host).id(state.telnet_id_host.clone()).on_input(Message::HostChanged).width(300)],
                        row![text("Port: ").width(100), text_input("23", &state.ssh_port).id(state.telnet_id_port.clone()).on_input(Message::PortChanged).width(150).on_submit(Message::ConnectTelnet)],
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Connect")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectTelnet)
                    ].spacing(15).into(),
                    ProtocolMode::Serial => column![
                        text("Serial Connection").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        row![text("COM Port: ").width(100), text_input("COM1", &state.serial_port).id(state.serial_id_port.clone()).on_input(Message::SerialPortChanged).width(300)],
                        row![text("Baud Rate: ").width(100), text_input("115200", &state.serial_baud).id(state.serial_id_baud.clone()).on_input(Message::SerialBaudChanged).width(150).on_submit(Message::ConnectSerial)],
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Connect")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectSerial)
                    ].spacing(15).into(),
                    ProtocolMode::Local => column![
                        text("Local System").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        text("Detected shells on this system").size(14),
                        {
                            let options: Vec<String> = state.local_shells.iter().map(|s| s.name.clone()).collect();
                            let selected = state.local_shells.get(state.selected_local_shell).map(|s| s.name.clone());

                            pick_list(options, selected, |name| {
                                let index = state
                                    .local_shells
                                    .iter()
                                    .position(|s| s.name == name)
                                    .unwrap_or(0);
                                Message::SelectLocalShell(index)
                            })
                            .width(Length::Fill)
                            .padding([8, 12])
                        },
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Launch Selected Shell")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectLocal)
                    ].spacing(15).into(),
                    ProtocolMode::Rdp => column![
                        text("RDP Connection").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        row![text("Host: ").width(100), text_input("IP Address", &state.rdp_host).id(state.rdp_id_host.clone()).on_input(Message::RdpHostChanged).width(300)],
                        row![text("Port: ").width(100), text_input("3389", &state.rdp_port).id(state.rdp_id_port.clone()).on_input(Message::RdpPortChanged).width(150)],
                        row![text("Username: ").width(100), text_input("user", &state.rdp_user).id(state.rdp_id_user.clone()).on_input(Message::RdpUserChanged).width(300)],
                        row![text("Password: ").width(100), text_input("pass", &state.rdp_pass).id(state.rdp_id_pass.clone()).on_input(Message::RdpPassChanged).secure(true).width(300).on_submit(Message::ConnectRdp)],
                        {
                            let resolution_labels: Vec<String> = crate::RDP_RESOLUTION_PRESETS.iter().map(|(w, h)| format!("{}x{}", w, h)).collect();
                            let selected_label = Some(format!("{}x{}", crate::RDP_RESOLUTION_PRESETS[state.selected_rdp_resolution_index].0, crate::RDP_RESOLUTION_PRESETS[state.selected_rdp_resolution_index].1));
                            let pick = pick_list(resolution_labels, selected_label, |selected| {
                                crate::RDP_RESOLUTION_PRESETS.iter().position(|(w, h)| format!("{}x{}", w, h) == selected).map(Message::RdpResolutionSelected).unwrap_or(Message::RdpResolutionSelected(1))
                            }).width(150);
                            row![text("Resolution: ").width(100), container(pick).id(state.rdp_id_resolution.clone()).width(150)]
                        },
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Connect")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectRdp)
                    ].spacing(15).into(),
                    ProtocolMode::Vnc => column![
                        text("VNC Connection").size(20).font(Font { weight: Weight::Bold, ..Default::default() }),
                        hr(),
                        row![text("Host: ").width(100), text_input("IP Address", &state.vnc_host).id(state.vnc_id_host.clone()).on_input(Message::VncHostChanged).width(300)],
                        row![text("Port: ").width(100), text_input("5900", &state.vnc_port).id(state.vnc_id_port.clone()).on_input(Message::VncPortChanged).width(150)],
                        row![text("Password: ").width(100), text_input("optional", &state.vnc_pass).id(state.vnc_id_pass.clone()).on_input(Message::VncPassChanged).secure(true).width(300).on_submit(Message::ConnectVnc)],
                        text("Password is optional for servers allowing None auth.").size(12).color(Color::from_rgb(0.6, 0.6, 0.6)),
                        Space::new().height(Length::Fixed(10.0)),
                        button(container(text("Connect")).center_x(Length::Fill).center_y(Length::Fill)).padding(12).width(Length::Fill).style(button::primary).on_press(Message::ConnectVnc)
                    ].spacing(15).into(),
                };

                let card_container = container(form_content)
                    .width(Length::Fixed(500.0))
                    .padding(30)
                    .style(|_| container::Style {
                        background: Some(Background::Color(Color::from_rgb(0.14, 0.14, 0.14))),
                        border: iced::Border { radius: 8.0.into(), width: 1.0, color: Color::from_rgb(0.25, 0.25, 0.25) },
                        ..Default::default()
                    });

                container(scrollable(
                    column![
                        text("Start a new session").size(28).font(Font { weight: Weight::Bold, ..Default::default() }),
                        Space::new().height(Length::Fixed(30.0)),
                        protocol_tabs,
                        Space::new().height(Length::Fixed(15.0)),
                        card_container,
                    ].align_x(iced::alignment::Horizontal::Center)
                )).center(Length::Fill).into()
            }
            SessionKind::Terminal => {
                let hist_len = session.terminal.history.len();
                let offset = session.terminal.display_offset;
                row![
                    container(TerminalView::new(
                        &session.terminal,
                        Message::TerminalScroll,
                        Message::TerminalResize,
                        Message::SelectionStart,
                        Message::SelectionUpdate,
                        || Message::CopyCurrentSelection,
                    ))
                    .width(Length::Fill)
                    .height(Length::Fill),
                    container(vertical_slider(0.0..=(hist_len as f32).max(1.0), offset as f32, |v| Message::TerminalScrollTo(v as usize)).step(1.0).style(|_, _| iced::widget::slider::Style { rail: iced::widget::slider::Rail { backgrounds: (iced::Background::Color(Color::from_rgb(0.1, 0.1, 0.1)), iced::Background::Color(Color::from_rgb(0.1, 0.1, 0.1))), width: 4.0, border: Default::default() }, handle: iced::widget::slider::Handle { shape: iced::widget::slider::HandleShape::Rectangle { width: 10, border_radius: 2.0f32.into() }, background: iced::Background::Color(Color::from_rgb(0.4, 0.4, 0.4)), border_width: 0.0, border_color: Color::TRANSPARENT } })).width(Length::Fixed(12.0)).height(Length::Fill).padding(2)
                ].into()
            }
            SessionKind::RemoteDisplay => {
                if let Some(display) = &session.remote_display {
                    if let Some(ref msg) = display.status_message {
                        scrollable(
                            container(
                                text(msg.clone()).size(12).color(Color::from_rgb(0.0, 1.0, 0.4))
                            )
                            .padding(10)
                            .width(Length::Fill)
                        )
                        .width(Length::Fill)
                        .height(Length::Fill)
                        .into()
                    } else if display.width > 0 {
                        let program = remote_display::renderer::RemoteDisplayProgram {
                            frame: std::sync::Arc::clone(&display.rgba),
                            tex_width: display.width as u32,
                            tex_height: display.height as u32,
                            dirty_rects: display.dirty_rects.clone(),
                            full_upload: display.full_upload,
                            frame_seq: display.frame_seq,
                            source_id: display.source_id,
                        };
                        container(shader(program).width(Length::Fill).height(Length::Fill))
                            .width(Length::Fill)
                            .height(Length::Fill)
                            .style(|_| container::Style {
                                background: Some(Background::Color(Color::BLACK)),
                                ..Default::default()
                            })
                            .into()
                    } else {
                        container(
                            column![
                                text("Remote display session connected"),
                                text("Waiting for first frame...").size(14),
                            ]
                            .spacing(10),
                        )
                        .center(Length::Fill)
                        .into()
                    }
                } else {
                    container(text("Remote display state is not initialized"))
                        .center(Length::Fill)
                        .into()
                }
            }
            SessionKind::Settings => {
                if let Some(tab_kind) = session.settings_tab_kind {
                    row![
                        super::settings::render_settings_sidebar(tab_kind, state.settings_selected_category),
                        vr(),
                        container(super::settings::render_settings_panel(state, tab_kind, state.settings_selected_category))
                            .width(Length::Fill)
                            .height(Length::Fill),
                    ]
                    .height(Length::Fill)
                    .into()
                } else {
                    container(text("Settings protocol not selected"))
                        .center(Length::Fill)
                        .into()
                }
            }
        }
    } else {
        text("No active tab").into()
    };

    let main_content = column![
        row![tab_bar, Space::new().width(Length::Fill)].align_y(iced::Alignment::Center),
        hr(),
        container(tab_content).width(Length::Fill).height(Length::Fill)
    ]
    .width(Length::Fill)
    .height(Length::Fill);
    let body = row![sidebar, vr(), main_content].height(Length::Fill);
    let base_layout: Element<'_, Message> = column![title_bar, body].into();

    let resize_handle = |dir: window::Direction, w: Length, h: Length| {
        let interaction = match dir {
            window::Direction::North | window::Direction::South => {
                mouse::Interaction::ResizingVertically
            }
            window::Direction::West | window::Direction::East => {
                mouse::Interaction::ResizingHorizontally
            }
            window::Direction::NorthWest | window::Direction::SouthEast => {
                mouse::Interaction::ResizingDiagonallyDown
            }
            window::Direction::NorthEast | window::Direction::SouthWest => {
                mouse::Interaction::ResizingDiagonallyUp
            }
        };
        mouse_area(
            container(Space::new())
                .width(w)
                .height(h)
                .style(|_| container::Style {
                    background: Some(Background::Color(Color::TRANSPARENT)),
                    ..Default::default()
                }),
        )
        .on_press(Message::WindowResize(dir))
        .interaction(interaction)
    };

    let content_with_resize = stack![
        container(base_layout).width(Length::Fill).height(Length::Fill),
        container(resize_handle(window::Direction::North, Length::Fill, Length::Fixed(8.0))).width(Length::Fill).height(Length::Fill).padding([0, 20]).align_y(iced::alignment::Vertical::Top),
        container(resize_handle(window::Direction::South, Length::Fill, Length::Fixed(8.0))).width(Length::Fill).height(Length::Fill).padding([0, 20]).align_y(iced::alignment::Vertical::Bottom),
        container(resize_handle(window::Direction::West, Length::Fixed(8.0), Length::Fill)).width(Length::Fill).height(Length::Fill).padding([20, 0]).align_x(iced::alignment::Horizontal::Left),
        container(resize_handle(window::Direction::East, Length::Fixed(8.0), Length::Fill)).width(Length::Fill).height(Length::Fill).padding([20, 0]).align_x(iced::alignment::Horizontal::Right),
        container(resize_handle(window::Direction::NorthWest, Length::Fixed(15.0), Length::Fixed(15.0))).width(Length::Fill).height(Length::Fill).align_x(iced::alignment::Horizontal::Left).align_y(iced::alignment::Vertical::Top),
        container(resize_handle(window::Direction::NorthEast, Length::Fixed(15.0), Length::Fixed(15.0))).width(Length::Fill).height(Length::Fill).align_x(iced::alignment::Horizontal::Right).align_y(iced::alignment::Vertical::Top),
        container(resize_handle(window::Direction::SouthWest, Length::Fixed(15.0), Length::Fixed(15.0))).width(Length::Fill).height(Length::Fill).align_x(iced::alignment::Horizontal::Left).align_y(iced::alignment::Vertical::Bottom),
        container(resize_handle(window::Direction::SouthEast, Length::Fixed(15.0), Length::Fixed(15.0))).width(Length::Fill).height(Length::Fill).align_x(iced::alignment::Horizontal::Right).align_y(iced::alignment::Vertical::Bottom),
    ];

    let final_content: Element<'_, Message> = if let Some(dir) = state.resizing_direction {
        let interaction = match dir {
            window::Direction::North | window::Direction::South => {
                mouse::Interaction::ResizingVertically
            }
            window::Direction::West | window::Direction::East => {
                mouse::Interaction::ResizingHorizontally
            }
            window::Direction::NorthWest | window::Direction::SouthEast => {
                mouse::Interaction::ResizingDiagonallyDown
            }
            window::Direction::NorthEast | window::Direction::SouthWest => {
                mouse::Interaction::ResizingDiagonallyUp
            }
        };
        stack![
            content_with_resize,
            mouse_area(container(Space::new()).width(Length::Fill).height(Length::Fill)).interaction(interaction)
        ]
        .into()
    } else {
        content_with_resize.into()
    };

    let overlay_layer: Element<'_, Message> = if let Some(menu) = state.dummy_menu_open {
        if !menu.is_empty() {
            let dropdown = match menu {
                "Session" => container(
                    column![
                        button(text("New SSH Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenProtocolTab(ProtocolMode::Ssh)),
                        button(text("New Telnet Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenProtocolTab(ProtocolMode::Telnet)),
                        button(text("New Serial Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenProtocolTab(ProtocolMode::Serial)),
                        button(text("New Local Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenProtocolTab(ProtocolMode::Local)),
                        button(text("New RDP Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenProtocolTab(ProtocolMode::Rdp)),
                        button(text("New VNC Session").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenProtocolTab(ProtocolMode::Vnc)),
                        hr(),
                        button(text("Close Current Tab").size(12)).width(Length::Fill).style(button::text).on_press(Message::CloseTab(state.active_index))
                    ]
                    .spacing(2)
                    .width(Length::Fixed(200.0))
                ),
                "Settings" => container(
                    column![
                        button(text("Preferences").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenSettingsTab(SettingsTabKind::Preferences)),
                        hr(),
                        button(text("Theme").size(12)).width(Length::Fill).style(button::text).on_press(Message::OpenSettingsTab(SettingsTabKind::Theme))
                    ]
                    .spacing(2)
                    .width(Length::Fixed(170.0))
                ),
                _ => container(
                    column![
                        button(text(format!("{} Option 1", menu)).size(12)).width(Length::Fill).style(button::text),
                        button(text(format!("{} Option 2", menu)).size(12)).width(Length::Fill).style(button::text)
                    ]
                    .spacing(2)
                    .width(Length::Fixed(160.0))
                ),
            }
            .padding(4)
            .style(|_| container::Style {
                background: Some(Background::Color(Color::from_rgb(0.18, 0.18, 0.18))),
                border: iced::Border {
                    width: 1.0,
                    color: Color::from_rgb(0.3, 0.3, 0.3),
                    radius: 4.0f32.into(),
                },
                ..Default::default()
            });
            let h_offset: f32 = match menu {
                "Session" => 95.0,
                "Settings" => 95.0 + 72.0,
                "View" => 95.0 + 72.0 * 2.0,
                "Help" => 95.0 + 72.0 * 3.0,
                _ => 95.0,
            };
            column![
                Space::new().height(Length::Fixed(35.0)),
                row![Space::new().width(Length::Fixed(h_offset)), dropdown]
            ]
            .into()
        } else {
            container(Space::new().width(Length::Shrink).height(Length::Shrink)).into()
        }
    } else {
        container(Space::new().width(Length::Shrink).height(Length::Shrink)).into()
    };

    let final_layout: Element<'_, Message> = stack![final_content, overlay_layer].into();

    container(final_layout)
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|_| container::Style {
            background: Some(Background::Color(Color::from_rgb(0.08, 0.08, 0.08))),
            text_color: Some(Color::WHITE),
            ..Default::default()
        })
        .into()
}
