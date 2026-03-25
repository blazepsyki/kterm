// SPDX-License-Identifier: MIT OR Apache-2.0

use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;
use iced::futures::{self, StreamExt};
use nectar::{event::TelnetEvent, TelnetCodec};
use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

use crate::connection::{ConnectionEvent, ConnectionInput};

struct TelnetState {
    stream: TcpStream,
    codec: TelnetCodec,
    read_buf: BytesMut,
    write_buf: BytesMut,
}

pub fn connect_and_subscribe(
    host: String,
    port: u16,
) -> futures::stream::BoxStream<'static, ConnectionEvent> {
    let (tx_to_iced, rx_from_ssh) = mpsc::unbounded_channel::<Vec<u8>>();
    let (tx_to_ssh, rx_from_iced) = mpsc::unbounded_channel::<ConnectionInput>();

    let initial_state = (None, host, port, tx_to_iced.clone(), tx_to_ssh.clone(), rx_from_ssh, rx_from_iced);

    futures::stream::unfold(
        initial_state,
        |(mut state_opt, host, port, tx_to_iced, tx_to_ssh, rx_from_ssh, mut rx_from_iced)| async move {
            if state_opt.is_none() {
                match TcpStream::connect((host.as_str(), port)).await {
                    Ok(stream) => {
                        let mut codec = TelnetCodec::new(8192);
                        codec.message_mode = false;
                        
                        // Send WILL TERMINAL TYPE natively if needed, but standard VT works directly mostly.
                        
                        state_opt = Some(TelnetState {
                            stream,
                            codec,
                            read_buf: BytesMut::with_capacity(8192),
                            write_buf: BytesMut::with_capacity(8192),
                        });
                        return Some((
                            ConnectionEvent::Connected(tx_to_ssh.clone()),
                            (state_opt, host, port, tx_to_iced, tx_to_ssh, rx_from_ssh, rx_from_iced)
                        ));
                    }
                    Err(e) => {
                        return Some((
                            ConnectionEvent::Error(format!("Telnet Connect Error: {}", e)),
                            (None, host, port, tx_to_iced, tx_to_ssh, rx_from_ssh, rx_from_iced)
                        ));
                    }
                }
            }

            let mut state = state_opt.take().unwrap();

            loop {
                let mut tcp_buf = [0u8; 8192];
                tokio::select! {
                    res = state.stream.read(&mut tcp_buf) => {
                        match res {
                            Ok(n) if n > 0 => {
                                state.read_buf.extend_from_slice(&tcp_buf[..n]);
                                let mut out_bytes = Vec::new();

                                while let Ok(Some(event)) = state.codec.decode(&mut state.read_buf) {
                                    match event {
                                        TelnetEvent::Character(c) => out_bytes.push(c),
                                        TelnetEvent::Message(msg) => out_bytes.extend_from_slice(msg.as_bytes()),
                                        _ => {} // Ignore options for raw emulation
                                    }
                                }

                                if !out_bytes.is_empty() {
                                    return Some((
                                        ConnectionEvent::Data(out_bytes),
                                        (Some(state), host, port, tx_to_iced, tx_to_ssh, rx_from_ssh, rx_from_iced)
                                    ));
                                }
                            }
                            Ok(_) | Err(_) => {
                                return Some((
                                    ConnectionEvent::Disconnected,
                                    (None, host, port, tx_to_iced, tx_to_ssh, rx_from_ssh, rx_from_iced)
                                ));
                            }
                        }
                    }
                    res = rx_from_iced.recv() => {
                        match res {
                            Some(ConnectionInput::Data(data)) => {
                                let mut escaped = Vec::with_capacity(data.len() + 2);
                                for &b in &data {
                                    if b == 255 { escaped.extend_from_slice(&[255, 255]); }
                                    else { escaped.push(b); }
                                }
                                let _ = state.stream.write_all(&escaped).await;
                                return Some((
                                    ConnectionEvent::Data(vec![]),
                                    (Some(state), host, port, tx_to_iced, tx_to_ssh, rx_from_ssh, rx_from_iced)
                                ));
                            }
                            Some(ConnectionInput::Resize { rows, cols }) => {
                                use nectar::subnegotiation::SubnegotiationType;
                                let _ = state.codec.encode(
                                    TelnetEvent::Subnegotiate(SubnegotiationType::WindowSize(cols, rows)),
                                    &mut state.write_buf
                                );
                                if !state.write_buf.is_empty() {
                                    let _ = state.stream.write_all(&state.write_buf).await;
                                    state.write_buf.clear();
                                }
                                return Some((
                                    ConnectionEvent::Data(vec![]),
                                    (Some(state), host, port, tx_to_iced, tx_to_ssh, rx_from_ssh, rx_from_iced)
                                ));
                            }
                            Some(ConnectionInput::RdpInput(_)) => {
                                return Some((
                                    ConnectionEvent::Data(vec![]),
                                    (Some(state), host, port, tx_to_iced, tx_to_ssh, rx_from_ssh, rx_from_iced)
                                ));
                            }
                            Some(ConnectionInput::SyncKeyboardIndicators(_)) => {
                                return Some((
                                    ConnectionEvent::Data(vec![]),
                                    (Some(state), host, port, tx_to_iced, tx_to_ssh, rx_from_ssh, rx_from_iced)
                                ));
                            }
                            Some(ConnectionInput::ReleaseAllModifiers) => {
                                return Some((
                                    ConnectionEvent::Data(vec![]),
                                    (Some(state), host, port, tx_to_iced, tx_to_ssh, rx_from_ssh, rx_from_iced)
                                ));
                            }
                            None => return None,
                        }
                    }
                }
            }
        }
    ).boxed()
}
