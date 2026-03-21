// SPDX-License-Identifier: MIT OR Apache-2.0

use tokio::sync::mpsc;
use iced::futures::StreamExt;
use portable_pty::{native_pty_system, CommandBuilder, PtySize, MasterPty, Child};
use std::io::Write;
use crate::connection::{ConnectionEvent, ConnectionInput};

/// 로컬 셸의 상태를 관리하기 위한 구조체
struct LocalShellState {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    _child: Box<dyn Child + Send + Sync>, // 프로세스가 종료되지 않도록 보관
}

pub fn spawn_local_shell(program: String, args: Vec<String>) -> iced::futures::stream::BoxStream<'static, ConnectionEvent> {
    let (ssh_to_iced_tx, ssh_to_iced_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (iced_to_ssh_tx, iced_to_ssh_rx) = mpsc::unbounded_channel::<ConnectionInput>();

    // unfold의 초기 상태 정의
    let initial_state = (None, program, args, ssh_to_iced_tx, iced_to_ssh_tx, ssh_to_iced_rx, iced_to_ssh_rx);

    futures::stream::unfold(
        initial_state,
        |(mut shell_state, program, args, ssh_tx, iced_tx, mut ssh_rx, mut iced_rx)| async move {
            if shell_state.is_none() {
                let pty_system = native_pty_system();
                let pair = match pty_system.openpty(PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                }) {
                    Ok(p) => p,
                    Err(e) => return Some((ConnectionEvent::Error(format!("PTY Open Error: {}", e)), (None, program, args, ssh_tx, iced_tx, ssh_rx, iced_rx))),
                };

                let mut cmd = CommandBuilder::new(&program);
                for arg in &args {
                    cmd.arg(arg);
                }
                let child = match pair.slave.spawn_command(cmd) {
                    Ok(c) => c,
                    Err(e) => return Some((ConnectionEvent::Error(format!("Process Spawn Error: {}", e)), (None, program, args, ssh_tx, iced_tx, ssh_rx, iced_rx))),
                };

                // PTY -> Iced 출력 루프 (별도 스레드에서 블로킹 읽기 수행)
                let mut reader = pair.master.try_clone_reader().expect("Failed to clone PTY reader");
                let tx_clone = ssh_tx.clone();
                std::thread::spawn(move || {
                    let mut buf = [0u8; 8192];
                    use std::io::Read;
                    while let Ok(n) = reader.read(&mut buf) {
                        if n == 0 { break; }
                        if let Err(_) = tx_clone.send(buf[..n].to_vec()) {
                            break;
                        }
                    }
                });

                let writer = pair.master.take_writer().expect("Failed to take PTY writer");
                shell_state = Some(LocalShellState {
                    master: pair.master,
                    writer,
                    _child: child,
                });
                
                return Some((ConnectionEvent::Connected(iced_tx.clone()), (shell_state, program, args, ssh_tx, iced_tx, ssh_rx, iced_rx)));
            }

            let mut current = shell_state.take().unwrap();
            
            tokio::select! {
                // PTY 출력 수신
                res = ssh_rx.recv() => {
                    match res {
                        Some(data) => {
                            // println!("[LocalShell] PTY Data: {:?}", String::from_utf8_lossy(&data));
                            let next_state = (Some(current), program, args, ssh_tx, iced_tx, ssh_rx, iced_rx);
                            Some((ConnectionEvent::Data(data), next_state))
                        }
                        None => {
                            Some((ConnectionEvent::Disconnected, (None, program, args, ssh_tx, iced_tx, ssh_rx, iced_rx)))
                        }
                    }
                }
                // Iced 입력 수신
                res = iced_rx.recv() => {
                    match res {
                        Some(ConnectionInput::Data(data)) => {
                            let _ = current.writer.write_all(&data);
                            let _ = current.writer.flush();
                            let next_state = (Some(current), program, args, ssh_tx, iced_tx, ssh_rx, iced_rx);
                            Some((ConnectionEvent::Data(vec![]), next_state))
                        }
                        Some(ConnectionInput::Resize { cols, rows }) => {
                            let _ = current.master.resize(PtySize {
                                rows,
                                cols,
                                pixel_width: 0,
                                pixel_height: 0,
                            });
                            let next_state = (Some(current), program, args, ssh_tx, iced_tx, ssh_rx, iced_rx);
                            Some((ConnectionEvent::Data(vec![]), next_state))
                        }
                        None => None,
                    }
                }
            }
        }
    ).boxed()
}

