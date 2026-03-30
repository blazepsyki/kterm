// SPDX-License-Identifier: MIT OR Apache-2.0

use std::sync::Arc;
use std::time::Duration;
use russh::client;
use russh::keys::PublicKey;
use tokio::sync::mpsc;
use crate::connection::{ConnectionEvent, ConnectionInput};
use iced::futures::{self, StreamExt};

pub struct ClientHandler {
    pub sender: mpsc::UnboundedSender<Vec<u8>>,
}

impl client::Handler for ClientHandler {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        _server_public_key: &PublicKey,
    ) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn data(
        &mut self,
        _channel: russh::ChannelId,
        data: &[u8],
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let _ = self.sender.send(data.to_vec());
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        _channel: russh::ChannelId,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        Ok(())
    }

    async fn channel_close(
        &mut self,
        _channel: russh::ChannelId,
        _session: &mut client::Session,
    ) -> Result<(), Self::Error> {
        let _ = self.sender.send(b"\r\nConnection closed by foreign host.\r\n".to_vec());
        Ok(())
    }
}


enum SshState {
    Init { host: String, port: u16, user: String, pass: String, keepalive: u64, terminal_type: String },
    Connecting(client::Handle<ClientHandler>, mpsc::UnboundedReceiver<Vec<u8>>, String),
    Connected {
        session: client::Handle<ClientHandler>,
        channel: russh::Channel<client::Msg>,
        ssh_to_iced_rx: mpsc::UnboundedReceiver<Vec<u8>>,
        iced_to_ssh_rx: mpsc::UnboundedReceiver<ConnectionInput>,
    },
    Finished,
}

pub fn connect_and_subscribe(
    host: String,
    port: u16,
    user: String,
    password: String,
    keepalive: u64,
    terminal_type: String,
) -> futures::stream::BoxStream<'static, ConnectionEvent> {
    let initial_state = SshState::Init { host, port, user, pass: password, keepalive, terminal_type };

    futures::stream::unfold(initial_state, |state| async move {
        match state {
            SshState::Init { host, port, user, pass, keepalive, terminal_type } => {
                let (ssh_to_iced_tx, ssh_to_iced_rx) = mpsc::unbounded_channel::<Vec<u8>>();
                let mut config = client::Config::default();
                if keepalive > 0 {
                    config.keepalive_interval = Some(Duration::from_secs(keepalive));
                }
                let config = Arc::new(config);
                let handler = ClientHandler { sender: ssh_to_iced_tx };

                let mut session = match client::connect(config, (host.as_str(), port), handler).await {
                    Ok(s) => s,
                    Err(e) => return Some((ConnectionEvent::Error(e.to_string()), SshState::Finished)),
                };

                match session.authenticate_password(user, pass).await {
                    Ok(russh::client::AuthResult::Success) => {
                        Some((ConnectionEvent::Data(b"Authenticated...\n".to_vec()), SshState::Connecting(session, ssh_to_iced_rx, terminal_type)))
                    },
                    Ok(_) => Some((ConnectionEvent::Error("Auth failed".into()), SshState::Finished)),
                    Err(e) => Some((ConnectionEvent::Error(e.to_string()), SshState::Finished)),
                }
            }
            SshState::Connecting(session, ssh_to_iced_rx, terminal_type) => {
                let channel = match session.channel_open_session().await {
                    Ok(c) => c,
                    Err(e) => return Some((ConnectionEvent::Error(e.to_string()), SshState::Finished)),
                };

                if let Err(e) = channel.request_pty(true, &terminal_type, 80, 24, 0, 0, &[]).await {
                    return Some((ConnectionEvent::Error(e.to_string()), SshState::Finished));
                }

                if let Err(e) = channel.request_shell(true).await {
                    return Some((ConnectionEvent::Error(e.to_string()), SshState::Finished));
                }

                let (iced_to_ssh_tx, iced_to_ssh_rx) = mpsc::unbounded_channel::<ConnectionInput>();
                Some((ConnectionEvent::Connected(iced_to_ssh_tx), SshState::Connected {
                    session,
                    channel,
                    ssh_to_iced_rx,
                    iced_to_ssh_rx,
                }))
            }
            SshState::Connected { session, channel, mut ssh_to_iced_rx, mut iced_to_ssh_rx } => {
                tokio::select! {
                    res = ssh_to_iced_rx.recv() => {
                        match res {
                            Some(data) => Some((ConnectionEvent::Data(data), SshState::Connected { session, channel, ssh_to_iced_rx, iced_to_ssh_rx })),
                            None => Some((ConnectionEvent::Disconnected, SshState::Finished)),
                        }
                    }
                    res = iced_to_ssh_rx.recv() => {
                        match res {
                            Some(ConnectionInput::Data(input)) => {
                                if let Err(e) = channel.data(&input[..]).await {
                                    Some((ConnectionEvent::Error(format!("Send Error: {}", e)), SshState::Finished))
                                } else {
                                    Some((ConnectionEvent::Data(vec![]), SshState::Connected { session, channel, ssh_to_iced_rx, iced_to_ssh_rx }))
                                }
                            }
                            Some(ConnectionInput::Resize { cols, rows }) => {
                                if let Err(e) = channel.window_change(cols as u32, rows as u32, 0, 0).await {
                                    Some((ConnectionEvent::Error(format!("Resize Error: {}", e)), SshState::Finished))
                                } else {
                                    Some((ConnectionEvent::Data(vec![]), SshState::Connected { session, channel, ssh_to_iced_rx, iced_to_ssh_rx }))
                                }
                            }
                            Some(ConnectionInput::RemoteInput(_)) => {
                                Some((ConnectionEvent::Data(vec![]), SshState::Connected { session, channel, ssh_to_iced_rx, iced_to_ssh_rx }))
                            }
                            Some(ConnectionInput::SyncKeyboardIndicators(_)) => {
                                Some((ConnectionEvent::Data(vec![]), SshState::Connected { session, channel, ssh_to_iced_rx, iced_to_ssh_rx }))
                            }
                            Some(ConnectionInput::ReleaseAllModifiers) => {
                                Some((ConnectionEvent::Data(vec![]), SshState::Connected { session, channel, ssh_to_iced_rx, iced_to_ssh_rx }))
                            }
                            None => Some((ConnectionEvent::Disconnected, SshState::Finished)),
                        }
                    }
                }
            }
            SshState::Finished => None,
        }
    }).boxed()
}

