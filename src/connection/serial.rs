use iced::futures::{self, StreamExt};
use tokio::sync::mpsc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_serial::SerialPortBuilderExt;

use crate::connection::{ConnectionEvent, ConnectionInput};

pub fn connect_and_subscribe(
    port_name: String,
    baud_rate: u32,
) -> futures::stream::BoxStream<'static, ConnectionEvent> {
    let (tx_to_serial, rx_from_iced) = mpsc::unbounded_channel::<ConnectionInput>();

    // State: (port_halves, port_name, baud_rate, tx_to_serial, rx_from_iced)
    type PortHalves = (
        tokio::io::ReadHalf<tokio_serial::SerialStream>,
        tokio::io::WriteHalf<tokio_serial::SerialStream>,
    );
    let initial_state: (Option<PortHalves>, String, u32, mpsc::UnboundedSender<ConnectionInput>, mpsc::UnboundedReceiver<ConnectionInput>) =
        (None, port_name, baud_rate, tx_to_serial, rx_from_iced);

    futures::stream::unfold(
        initial_state,
        |(state_opt, port_name, baud_rate, tx_to_serial, mut rx_from_iced)| async move {
            if state_opt.is_none() {
                let port_result = tokio_serial::new(port_name.clone(), baud_rate)
                    .open_native_async();

                match port_result {
                    Ok(port) => {
                        let (reader, writer) = tokio::io::split(port);
                        return Some((
                            ConnectionEvent::Connected(tx_to_serial.clone()),
                            (Some((reader, writer)), port_name, baud_rate, tx_to_serial, rx_from_iced),
                        ));
                    }
                    Err(e) => {
                        return Some((
                            ConnectionEvent::Error(format!("Serial Port Error: {}", e)),
                            (None, port_name, baud_rate, tx_to_serial, rx_from_iced),
                        ));
                    }
                }
            }

            let (mut reader, mut writer) = state_opt.unwrap();
            let mut buf = [0u8; 8192];

            tokio::select! {
                res = reader.read(&mut buf) => {
                    match res {
                        Ok(0) => {
                            // EOF — port disconnected
                            Some((
                                ConnectionEvent::Disconnected,
                                (None, port_name, baud_rate, tx_to_serial, rx_from_iced),
                            ))
                        }
                        Ok(n) => {
                            Some((
                                ConnectionEvent::Data(buf[..n].to_vec()),
                                (Some((reader, writer)), port_name, baud_rate, tx_to_serial, rx_from_iced),
                            ))
                        }
                        Err(e) => {
                            Some((
                                ConnectionEvent::Error(format!("Serial read error: {}", e)),
                                (None, port_name, baud_rate, tx_to_serial, rx_from_iced),
                            ))
                        }
                    }
                }
                res = rx_from_iced.recv() => {
                    match res {
                        Some(ConnectionInput::Data(data)) => {
                            let _ = writer.write_all(&data).await;
                            Some((
                                ConnectionEvent::Data(vec![]),
                                (Some((reader, writer)), port_name, baud_rate, tx_to_serial, rx_from_iced),
                            ))
                        }
                        Some(ConnectionInput::Resize { .. }) => {
                            // Serial ports do not support NAWS; ignore.
                            Some((
                                ConnectionEvent::Data(vec![]),
                                (Some((reader, writer)), port_name, baud_rate, tx_to_serial, rx_from_iced),
                            ))
                        }
                        Some(ConnectionInput::RemoteInput(_)) => {
                            Some((
                                ConnectionEvent::Data(vec![]),
                                (Some((reader, writer)), port_name, baud_rate, tx_to_serial, rx_from_iced),
                            ))
                        }
                        Some(ConnectionInput::SyncKeyboardIndicators(_)) => {
                            Some((
                                ConnectionEvent::Data(vec![]),
                                (Some((reader, writer)), port_name, baud_rate, tx_to_serial, rx_from_iced),
                            ))
                        }
                        Some(ConnectionInput::ReleaseAllModifiers) => {
                            Some((
                                ConnectionEvent::Data(vec![]),
                                (Some((reader, writer)), port_name, baud_rate, tx_to_serial, rx_from_iced),
                            ))
                        }
                        None => {
                            // Sender dropped — session closed
                            None
                        }
                    }
                }
            }
        },
    ).boxed()
}
