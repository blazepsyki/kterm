use iced::futures::{self, StreamExt};
use tokio::sync::mpsc;
use std::time::Duration;
use serialport;
use std::io::{Read, Write};

use crate::connection::{ConnectionEvent, ConnectionInput};

pub fn connect_and_subscribe(
    port_name: String,
    baud_rate: u32,
) -> futures::stream::BoxStream<'static, ConnectionEvent> {
    let (tx_to_iced, rx_from_serial) = mpsc::unbounded_channel::<Vec<u8>>();
    let (tx_to_serial, rx_from_iced) = mpsc::unbounded_channel::<ConnectionInput>();

    let initial_state = (None, port_name, baud_rate, tx_to_iced.clone(), tx_to_serial.clone(), rx_from_serial, rx_from_iced);

    futures::stream::unfold(
        initial_state,
        |(mut state_opt, port_name, baud_rate, tx_to_iced, tx_to_serial, mut rx_from_serial, mut rx_from_iced)| async move {
            if state_opt.is_none() {
                // Open Serial Port
                let port_result = serialport::new(port_name.clone(), baud_rate)
                    .timeout(Duration::from_millis(10))
                    .open();
                
                match port_result {
                    Ok(port) => {
                        // Clone the port for writing
                        let mut read_port = port.try_clone().expect("Failed to clone serial port");
                        
                        // Spawn background blocking thread for reading
                        let tx_clone = tx_to_iced.clone();
                        std::thread::spawn(move || {
                            let mut buf = [0u8; 8192];
                            loop {
                                match read_port.read(&mut buf) {
                                    Ok(0) => break, // EOF typically means disconnected
                                    Ok(n) => {
                                        if tx_clone.send(buf[..n].to_vec()).is_err() {
                                            break; // Receiver dropped, stop thread
                                        }
                                    }
                                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                                        // Timeout is expected, just loop and read again
                                        continue;
                                    }
                                    Err(_) => {
                                        // Real error (e.g. disconnected)
                                        break;
                                    }
                                }
                            }
                        });

                        state_opt = Some(port);
                        return Some((
                            ConnectionEvent::Connected(tx_to_serial.clone()),
                            (state_opt, port_name, baud_rate, tx_to_iced, tx_to_serial, rx_from_serial, rx_from_iced)
                        ));
                    }
                    Err(e) => {
                        return Some((
                            ConnectionEvent::Error(format!("Serial Port Error: {}", e)),
                            (None, port_name, baud_rate, tx_to_iced, tx_to_serial, rx_from_serial, rx_from_iced)
                        ));
                    }
                }
            }

            let mut write_port = state_opt.take().unwrap();

            // We use tokio::select! to await incoming bytes from the reading thread OR user inputs
            tokio::select! {
                res = rx_from_serial.recv() => {
                    match res {
                        Some(data) => {
                            return Some((
                                ConnectionEvent::Data(data),
                                (Some(write_port), port_name, baud_rate, tx_to_iced, tx_to_serial, rx_from_serial, rx_from_iced)
                            ));
                        }
                        None => {
                            // Channel closed (Reader thread exited)
                            return Some((
                                ConnectionEvent::Disconnected,
                                (None, port_name, baud_rate, tx_to_iced, tx_to_serial, rx_from_serial, rx_from_iced)
                            ));
                        }
                    }
                }
                res = rx_from_iced.recv() => {
                    match res {
                        Some(ConnectionInput::Data(data)) => {
                            let _ = write_port.write_all(&data);
                            return Some((
                                ConnectionEvent::Data(vec![]),
                                (Some(write_port), port_name, baud_rate, tx_to_iced, tx_to_serial, rx_from_serial, rx_from_iced)
                            ));
                        }
                        Some(ConnectionInput::Resize { .. }) => {
                            // Serial terminals ignore NAWS (Negotiate About Window Size) mostly, or handle it via out-of-band VT escape codes.
                            // We ignore it for now.
                            return Some((
                                ConnectionEvent::Data(vec![]),
                                (Some(write_port), port_name, baud_rate, tx_to_iced, tx_to_serial, rx_from_serial, rx_from_iced)
                            ));
                        }
                        Some(ConnectionInput::RdpInput(_)) => {
                            return Some((
                                ConnectionEvent::Data(vec![]),
                                (Some(write_port), port_name, baud_rate, tx_to_iced, tx_to_serial, rx_from_serial, rx_from_iced)
                            ));
                        }
                        None => {
                            // Session closed
                            return None;
                        }
                    }
                }
            }
        }
    ).boxed()
}
