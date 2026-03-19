use std::sync::Arc;
use russh::client;
use russh::keys::PublicKey;
use tokio::sync::mpsc;
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
}

pub enum SshEvent {
    Connected(mpsc::UnboundedSender<Vec<u8>>),
    Data(Vec<u8>),
    Disconnected,
    Error(String),
}

impl std::fmt::Debug for SshEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Connected(_) => write!(f, "Connected"),
            Self::Data(d) => write!(f, "Data({} bytes)", d.len()),
            Self::Disconnected => write!(f, "Disconnected"),
            Self::Error(e) => write!(f, "Error({})", e),
        }
    }
}

impl Clone for SshEvent {
    fn clone(&self) -> Self {
        match self {
            Self::Connected(s) => Self::Connected(s.clone()),
            Self::Data(d) => Self::Data(d.clone()),
            Self::Disconnected => Self::Disconnected,
            Self::Error(e) => Self::Error(e.clone()),
        }
    }
}

enum SshState {
    Init { host: String, port: u16, user: String, pass: String },
    Connecting(client::Handle<ClientHandler>, mpsc::UnboundedReceiver<Vec<u8>>),
    Connected {
        session: client::Handle<ClientHandler>,
        channel: russh::Channel<client::Msg>,
        ssh_to_iced_rx: mpsc::UnboundedReceiver<Vec<u8>>,
        iced_to_ssh_rx: mpsc::UnboundedReceiver<Vec<u8>>,
    },
    Finished,
}

pub fn connect_and_subscribe(
    host: String,
    port: u16,
    user: String,
    password: String,
) -> futures::stream::BoxStream<'static, SshEvent> {
    let initial_state = SshState::Init { host, port, user, pass: password };

    futures::stream::unfold(initial_state, |state| async move {
        match state {
            SshState::Init { host, port, user, pass } => {
                let (ssh_to_iced_tx, ssh_to_iced_rx) = mpsc::unbounded_channel::<Vec<u8>>();
                let config = Arc::new(client::Config::default());
                let handler = ClientHandler { sender: ssh_to_iced_tx };

                let mut session = match client::connect(config, (host.as_str(), port), handler).await {
                    Ok(s) => s,
                    Err(e) => return Some((SshEvent::Error(e.to_string()), SshState::Finished)),
                };

                match session.authenticate_password(user, pass).await {
                    Ok(russh::client::AuthResult::Success) => {
                        Some((SshEvent::Data(b"Authenticated...\n".to_vec()), SshState::Connecting(session, ssh_to_iced_rx)))
                    },
                    Ok(_) => Some((SshEvent::Error("Auth failed".into()), SshState::Finished)),
                    Err(e) => Some((SshEvent::Error(e.to_string()), SshState::Finished)),
                }
            }
            SshState::Connecting(session, ssh_to_iced_rx) => {
                let channel = match session.channel_open_session().await {
                    Ok(c) => c,
                    Err(e) => return Some((SshEvent::Error(e.to_string()), SshState::Finished)),
                };

                // PTY 요청 (인터랙티브 쉘을 위해 필수)
                if let Err(e) = channel.request_pty(true, "xterm-256color", 80, 24, 0, 0, &[]).await {
                    return Some((SshEvent::Error(e.to_string()), SshState::Finished));
                }

                if let Err(e) = channel.request_shell(true).await {
                    return Some((SshEvent::Error(e.to_string()), SshState::Finished));
                }

                let (iced_to_ssh_tx, iced_to_ssh_rx) = mpsc::unbounded_channel::<Vec<u8>>();
                Some((SshEvent::Connected(iced_to_ssh_tx), SshState::Connected {
                    session,
                    channel,
                    ssh_to_iced_rx,
                    iced_to_ssh_rx,
                }))
            }
            SshState::Connected { session, mut channel, mut ssh_to_iced_rx, mut iced_to_ssh_rx } => {
                tokio::select! {
                    Some(data) = ssh_to_iced_rx.recv() => {
                        Some((SshEvent::Data(data), SshState::Connected { session, channel, ssh_to_iced_rx, iced_to_ssh_rx }))
                    }
                    Some(input) = iced_to_ssh_rx.recv() => {
                        if let Err(e) = channel.data(&input[..]).await {
                            Some((SshEvent::Error(e.to_string()), SshState::Finished))
                        } else {
                            // No event to return, but we need to continue unfold.
                            // We can use a recursive call or a dummy event.
                            // I'll return an empty Data event which main.rs will ignore or process as empty.
                            Some((SshEvent::Data(vec![]), SshState::Connected { session, channel, ssh_to_iced_rx, iced_to_ssh_rx }))
                        }
                    }
                    _res = channel.wait() => {
                         Some((SshEvent::Data(vec![]), SshState::Connected { session, channel, ssh_to_iced_rx, iced_to_ssh_rx }))
                    }
                }
            }
            SshState::Finished => None,
        }
    }).boxed()
}
