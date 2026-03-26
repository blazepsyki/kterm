// SPDX-License-Identifier: MIT OR Apache-2.0

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write as _;
use std::time::{Duration, Instant};

use iced::futures::{self, StreamExt};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;

use ironrdp::connector::{self, BitmapConfig, ConnectionResult, Credentials};
use ironrdp::connector::connection_activation::ConnectionActivationState;
use ironrdp::graphics::image_processing::PixelFormat;
use ironrdp::graphics::zgfx::Decompressor;
use ironrdp::pdu::Action;
use ironrdp::pdu::bitmap::{BitmapUpdateData, Compression};
use ironrdp::pdu::gcc::KeyboardType;
use ironrdp::pdu::geometry::{InclusiveRectangle, Rectangle};
use ironrdp::pdu::rdp::capability_sets::{
    BitmapCodecs, CaptureFlags, Codec, CodecProperty, EntropyBits, MajorPlatformType,
    RemoteFxContainer, RfxCaps, RfxCapset, RfxClientCapsContainer, RfxICap, RfxICapFlags,
};
use ironrdp::pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp::pdu::rdp::headers::ShareDataPdu;
use ironrdp::pdu::rdp::vc::dvc::gfx::{
    CapabilitiesAdvertisePdu, CapabilitiesV10Flags, CapabilitiesV103Flags, CapabilitiesV104Flags,
    CapabilitiesV107Flags, CapabilitiesV8Flags, CapabilitiesV81Flags, CapabilitySet, ClientPdu,
    Codec1Type, FrameAcknowledgePdu, PixelFormat as GfxPixelFormat, QueueDepth, ServerPdu,
};
use ironrdp::session::image::DecodedImage;
use ironrdp::session::{ActiveStage, ActiveStageOutput};

use ironrdp_cliprdr::backend::{ClipboardMessage, CliprdrBackendFactory};
use ironrdp_cliprdr::{Cliprdr, CliprdrClient};
use ironrdp_core::{
    impl_as_any, Decode as _, Encode as IronEncode, EncodeResult, ReadCursor, WriteBuf, WriteCursor,
};
use ironrdp_dvc::ironrdp_pdu as dvc_pdu;
use ironrdp_dvc::{DrdynvcClient, DvcClientProcessor, DvcEncode, DvcMessage, DvcProcessor};
use ironrdp_input::{
    synchronize_event, Database, MouseButton as IrdpMouseButton, MousePosition, Operation, Scancode,
    WheelRotations,
};
use ironrdp_rdpsnd::client::Rdpsnd;
use ironrdp_rdpsnd_native::cpal::RdpsndBackend;
use ironrdp_tokio::{
    connect_begin, connect_finalize, mark_as_upgraded, single_sequence_step,
    Framed, FramedWrite, MovableTokioStream,
};

use crate::connection::rdp_input_policy::is_numlock_conflict_scancode;
use crate::connection::{ConnectionEvent, ConnectionInput, KeyboardIndicators, RdpInput, RdpMouseButton};
use crate::remote_display::FrameUpdate;

pub fn connect_and_subscribe(
    host: String,
    port: u16,
    username: String,
    password: String,
    cliprdr_factory: Option<Box<dyn CliprdrBackendFactory + Send>>,
    clipboard_rx: Option<mpsc::UnboundedReceiver<ClipboardMessage>>,
) -> futures::stream::BoxStream<'static, ConnectionEvent> {
    let (tx_to_rdp, rx_from_iced) = mpsc::unbounded_channel::<ConnectionInput>();
    let (tx_from_worker, rx_from_worker) = mpsc::unbounded_channel::<ConnectionEvent>();

    tokio::spawn(async move {
        run_rdp_worker(host, port, username, password, rx_from_iced, tx_from_worker, tx_to_rdp, cliprdr_factory, clipboard_rx).await;
    });

    // Batch consecutive Frames events to reduce Handle rebuilds on the UI side.
    struct RdpStream {
        rx: mpsc::UnboundedReceiver<ConnectionEvent>,
        pending: Option<ConnectionEvent>,
    }

    futures::stream::unfold(
        RdpStream { rx: rx_from_worker, pending: None },
        |mut s| async move {
            // Return any buffered non-Frame event first.
            if let Some(ev) = s.pending.take() {
                return Some((ev, s));
            }

            let first = s.rx.recv().await?;

            // For non-Frame events, pass through immediately.
            let ConnectionEvent::Frames(mut merged) = first else {
                return Some((first, s));
            };

            // Greedily drain and merge consecutive Frames.
            loop {
                match s.rx.try_recv() {
                    Ok(ConnectionEvent::Frames(more)) => merged.extend(more),
                    Ok(other) => {
                        s.pending = Some(other);
                        break;
                    }
                    Err(_) => break,
                }
            }

            Some((ConnectionEvent::Frames(merged), s))
        },
    )
    .boxed()
}

async fn run_rdp_worker(
    host: String,
    port: u16,
    username: String,
    password: String,
    rx_from_iced: mpsc::UnboundedReceiver<ConnectionInput>,
    tx_from_worker: mpsc::UnboundedSender<ConnectionEvent>,
    tx_to_rdp: mpsc::UnboundedSender<ConnectionInput>,
    cliprdr_factory: Option<Box<dyn CliprdrBackendFactory + Send>>,
    clipboard_rx: Option<mpsc::UnboundedReceiver<ClipboardMessage>>,
) {
    let tx_err = tx_from_worker.clone();
    if let Err(err) = run_rdp_worker_inner(host, port, username, password, rx_from_iced, tx_from_worker, tx_to_rdp, cliprdr_factory, clipboard_rx).await {
        let _ = tx_err.send(ConnectionEvent::Error(err));
    }
}

async fn run_rdp_worker_inner(
    host: String,
    port: u16,
    username: String,
    password: String,
    mut rx_from_iced: mpsc::UnboundedReceiver<ConnectionInput>,
    tx_from_worker: mpsc::UnboundedSender<ConnectionEvent>,
    tx_to_rdp: mpsc::UnboundedSender<ConnectionInput>,
    cliprdr_factory: Option<Box<dyn CliprdrBackendFactory + Send>>,
    mut clipboard_rx: Option<mpsc::UnboundedReceiver<ClipboardMessage>>,
) -> Result<(), String> {
    // --- PDU trace log -------------------------------------------------------
    // Log handshake sequencing plus every runtime Action so protocol behavior
    // can be inspected end-to-end.
    let pdu_log_path = "rdp_pdu_trace.log";
    let mut pdu_log = OpenOptions::new()
        .create(true).append(true).open(pdu_log_path)
        .map_err(|e| format!("cannot open PDU log: {}", e))?;
    writeln!(pdu_log, "\n=== RDP session start  host={host} ===")
        .map_err(|e| format!("pdu_log write: {}", e))?;

    let config = build_config(username, password, None);
    let (gfx_frame_tx, mut gfx_frame_rx) = mpsc::unbounded_channel::<Vec<crate::remote_display::FrameUpdate>>();
    let (connection_result, mut framed) = connect(config, host.clone(), port, gfx_frame_tx, cliprdr_factory).await?;

    let width = connection_result.desktop_size.width;
    let height = connection_result.desktop_size.height;

    let mut image = DecodedImage::new(PixelFormat::RgbA32, width, height);
    let mut active_stage = ActiveStage::new(connection_result);

    // Emit Connected only AFTER handshake succeeds
    let _ = tx_from_worker.send(ConnectionEvent::Connected(tx_to_rdp));

    let mut graphics_updates = 0usize;
    let mut response_frames = 0usize;
    let mut processed_pdus = 0usize;
    let mut last_frame_emit = Instant::now();
    let mut pending_rect: Option<InclusiveRectangle> = None;
    let mut input_db = Database::new();
    let mut last_indicators: Option<KeyboardIndicators> = None;

    let summary = format!(
        "\r\n[RDP] IronRDP handshake completed: server={} port={} desktop={}x{}\r\n",
        host, port, width, height
    );
    let _ = tx_from_worker.send(ConnectionEvent::Data(summary.into_bytes()));

    loop {
        tokio::select! {
            maybe_clipboard_msg = async {
                if let Some(ref mut rx) = clipboard_rx {
                    rx.recv().await
                } else {
                    futures::future::pending().await
                }
            } => {
                if let Some(msg) = maybe_clipboard_msg {
                    // Obtain a reference to the registered CliprdrClient SVC processor
                    // and dispatch the OS clipboard event to the appropriate Cliprdr method.
                    let svc_result = match msg {
                        ClipboardMessage::SendInitiateCopy(formats) => {
                            if let Some(cliprdr) = active_stage.get_svc_processor::<CliprdrClient>() {
                                cliprdr.initiate_copy(&formats).ok()
                            } else { None }
                        }
                        ClipboardMessage::SendFormatData(response) => {
                            if let Some(cliprdr) = active_stage.get_svc_processor::<CliprdrClient>() {
                                cliprdr.submit_format_data(response).ok()
                            } else { None }
                        }
                        ClipboardMessage::SendInitiatePaste(format_id) => {
                            if let Some(cliprdr) = active_stage.get_svc_processor::<CliprdrClient>() {
                                cliprdr.initiate_paste(format_id).ok()
                            } else { None }
                        }
                        ClipboardMessage::Error(e) => {
                            eprintln!("[CLIPRDR] OS clipboard error: {}", e);
                            None
                        }
                    };

                    // Encode the SVC messages into a network frame and send it.
                    if let Some(messages) = svc_result {
                        match active_stage.process_svc_processor_messages(messages) {
                            Ok(frame) => {
                                if !frame.is_empty() {
                                    framed
                                        .write_all(&frame)
                                        .await
                                        .map_err(|e| format!("clipboard frame write failed: {}", e))?;
                                }
                            }
                            Err(e) => {
                                eprintln!("[CLIPRDR] process_svc_processor_messages error: {:?}", e);
                            }
                        }
                    }
                }
            }

            maybe_input = rx_from_iced.recv() => {
                let Some(input) = maybe_input else {
                    let _ = tx_from_worker.send(ConnectionEvent::Data(
                        b"\r\n[RDP] input channel closed; stopping RDP worker.\r\n".to_vec(),
                    ));
                    let _ = tx_from_worker.send(ConnectionEvent::Disconnected);
                    return Ok(());
                };
                // Track last known lock-key state for pre-keydown sync.
                if let ConnectionInput::SyncKeyboardIndicators(ind) = &input {
                    last_indicators = Some(*ind);
                }
                presync_conflict_key_if_needed(
                    &input,
                    last_indicators,
                    &mut framed,
                    &mut active_stage,
                    &mut image,
                    &mut pending_rect,
                ).await;
                handle_rdp_input(input, &mut framed, &mut active_stage, &mut image,
                    &mut input_db, &mut pending_rect).await?;
                // Drain any additional inputs that arrived concurrently.
                loop {
                    match rx_from_iced.try_recv() {
                        Ok(inp) => {
                            if let ConnectionInput::SyncKeyboardIndicators(ind) = &inp {
                                last_indicators = Some(*ind);
                            }
                            presync_conflict_key_if_needed(
                                &inp,
                                last_indicators,
                                &mut framed,
                                &mut active_stage,
                                &mut image,
                                &mut pending_rect,
                            ).await;
                            handle_rdp_input(inp, &mut framed, &mut active_stage, &mut image,
                                &mut input_db, &mut pending_rect).await?;
                        }
                        Err(TryRecvError::Empty) => break,
                        Err(TryRecvError::Disconnected) => {
                            let _ = tx_from_worker.send(ConnectionEvent::Disconnected);
                            return Ok(());
                        }
                    }
                }
            }

            pdu_result = framed.read_pdu() => {
                let (action, payload) = pdu_result
                    .map_err(|e| format!("active stage read failed: {}", e))?;

                processed_pdus += 1;

                // --- PDU trace -----------------------------------------------
                match action {
                    Action::X224 => log_x224_pdu(&mut pdu_log, processed_pdus, &payload),
                    Action::FastPath => log_fastpath_pdu(&mut pdu_log, processed_pdus, &payload),
                }

                let outputs = match active_stage.process(&mut image, action, &payload) {
                    Ok(outputs) => outputs,
                    Err(e) => {
                        // IronRDP does not handle slow-path Update PDUs natively.
                        // Try to decode as slow-path bitmap update and send directly.
                        if action == Action::X224 {
                            let frame_updates = try_handle_slowpath_bitmap(&payload);
                            if !frame_updates.is_empty() {
                                let _ = tx_from_worker.send(ConnectionEvent::Frames(frame_updates));
                            }
                        } else {
                            eprintln!("[RDP] unhandled PDU #{}: {:?} ({})", processed_pdus, action, e);
                        }
                        continue;
                    }
                };

                for out in outputs {
                    match out {
                        ActiveStageOutput::ResponseFrame(frame) => {
                            framed
                                .write_all(&frame)
                                .await
                                .map_err(|e| format!("active stage write failed: {}", e))?;
                            response_frames += 1;
                        }
                        ActiveStageOutput::GraphicsUpdate(rect) => {
                            graphics_updates += 1;
                            pending_rect = Some(match pending_rect {
                                Some(prev) => merge_rect(prev, rect),
                                None => rect,
                            });

                            if last_frame_emit.elapsed() >= Duration::from_millis(16) {
                                if let Some(r) = pending_rect.take() {
                                    if let Some(update) = rect_update_from_image(&image, r) {
                                        let _ = tx_from_worker.send(ConnectionEvent::Frames(vec![update]));
                                    }
                                }
                                last_frame_emit = Instant::now();
                            }
                        }
                        ActiveStageOutput::Terminate(reason) => {
                            let summary = format!(
                                "\r\n[RDP] terminated: {} (pdus={}, updates={}, responses={})\r\n",
                                reason.description(),
                                processed_pdus,
                                graphics_updates,
                                response_frames,
                            );
                            let _ = tx_from_worker.send(ConnectionEvent::Data(summary.into_bytes()));
                            let _ = tx_from_worker.send(ConnectionEvent::Disconnected);
                            return Ok(());
                        }
                        ActiveStageOutput::DeactivateAll(mut cas) => {
                            let mut buf = WriteBuf::new();
                            loop {
                                if matches!(
                                    cas.connection_activation_state(),
                                    ConnectionActivationState::Finalized { .. }
                                ) {
                                    break;
                                }
                                single_sequence_step(&mut framed, &mut *cas, &mut buf)
                                    .await
                                    .map_err(|e| format!("reactivation step failed: {:?}", e))?;
                            }
                            // Session reactivated (e.g. login → desktop transition
                            // on servers that send DeactivateAll, such as Windows RDS).
                            eprintln!("[RDP] session reactivated");
                            if let Some(ind) = last_indicators {
                                sync_keyboard_indicators(
                                    &mut framed, &mut active_stage, &mut image,
                                    &mut pending_rect, ind, "reactivation",
                                ).await;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        // 50ms fallback: flush accumulated dirty rect if 16ms emitter didn't fire recently.
        if graphics_updates > 0 && last_frame_emit.elapsed() >= Duration::from_millis(50) {
            if let Some(rect) = pending_rect.take() {
                if let Some(update) = rect_update_from_image(&image, rect) {
                    let _ = tx_from_worker.send(ConnectionEvent::Frames(vec![update]));
                }
            }
            last_frame_emit = Instant::now();
        }

        // Forward GFX DVC frames (Phase 9-B-1) to the UI without blocking.
        loop {
            match gfx_frame_rx.try_recv() {
                Ok(frames) => {
                    let _ = tx_from_worker.send(ConnectionEvent::Frames(frames));
                }
                Err(_) => break,
            }
        }
    }
}

/// Sends a TS_SYNC_EVENT to synchronize local toggle-key states (NumLock, CapsLock, ScrollLock)
/// to the remote server.
async fn sync_keyboard_indicators(
    framed: &mut UpgradedFramed,
    active_stage: &mut ActiveStage,
    image: &mut DecodedImage,
    pending_rect: &mut Option<InclusiveRectangle>,
    indicators: KeyboardIndicators,
    reason: &str,
) {
    eprintln!(
        "[RDP] sync_keyboard_indicators [{}]: NumLock={} CapsLock={} ScrollLock={}",
        reason, indicators.num_lock, indicators.caps_lock, indicators.scroll_lock,
    );
    let ev = synchronize_event(
        indicators.scroll_lock,
        indicators.num_lock,
        indicators.caps_lock,
        false,
    );
    if let Ok(outputs) = active_stage.process_fastpath_input(image, &[ev]) {
        for out in outputs {
            if let ActiveStageOutput::ResponseFrame(frame) = out {
                let _ = framed.write_all(&frame).await;
            } else if let ActiveStageOutput::GraphicsUpdate(rect) = out {
                *pending_rect = Some(match pending_rect.take() {
                    Some(prev) => merge_rect(prev, rect),
                    None => rect,
                });
            }
        }
    }
}

fn should_presync_keyboard_input(input: &ConnectionInput) -> bool {
    matches!(
        input,
        ConnectionInput::RdpInput(RdpInput::KeyboardScancode {
            code,
            down: true,
            ..
        }) if is_numlock_conflict_scancode(*code)
    )
}

async fn presync_conflict_key_if_needed(
    input: &ConnectionInput,
    last_indicators: Option<KeyboardIndicators>,
    framed: &mut UpgradedFramed,
    active_stage: &mut ActiveStage,
    image: &mut DecodedImage,
    pending_rect: &mut Option<InclusiveRectangle>,
) {
    if !should_presync_keyboard_input(input) {
        return;
    }

    if let Some(indicators) = last_indicators {
        sync_keyboard_indicators(
            framed,
            active_stage,
            image,
            pending_rect,
            indicators,
            "pre-keydown",
        )
        .await;
    }
}

async fn process_fastpath_events(
    framed: &mut UpgradedFramed,
    active_stage: &mut ActiveStage,
    image: &mut DecodedImage,
    pending_rect: &mut Option<InclusiveRectangle>,
    events: &[ironrdp::pdu::input::fast_path::FastPathInputEvent],
) -> Result<(), String> {
    if events.is_empty() {
        return Ok(());
    }

    if let Ok(outputs) = active_stage.process_fastpath_input(image, events) {
        for out in outputs {
            match out {
                ActiveStageOutput::ResponseFrame(frame) => {
                    framed
                        .write_all(&frame)
                        .await
                        .map_err(|e| format!("fastpath write failed: {}", e))?;
                }
                ActiveStageOutput::GraphicsUpdate(rect) => {
                    *pending_rect = Some(match pending_rect.take() {
                        Some(prev) => merge_rect(prev, rect),
                        None => rect,
                    });
                }
                _ => {}
            }
        }
    }

    Ok(())
}

/// Handle a single `ConnectionInput` event: write resize/fastpath PDUs to the RDP framed stream.
async fn handle_rdp_input(
    input: ConnectionInput,
    framed: &mut UpgradedFramed,
    active_stage: &mut ActiveStage,
    image: &mut DecodedImage,
    db: &mut Database,
    pending_rect: &mut Option<InclusiveRectangle>,
) -> Result<(), String> {
    match input {
        ConnectionInput::Resize { cols, rows } => {
            let pixel_w = u32::from(cols).max(200).min(8192);
            let pixel_h = u32::from(rows).max(200).min(8192);
            if let Some(encoded) = active_stage.encode_resize(pixel_w, pixel_h, None, None) {
                if let Ok(buf) = encoded {
                    framed
                        .write_all(&buf)
                        .await
                        .map_err(|e| format!("resize write failed: {}", e))?;
                }
            }
        }
        ConnectionInput::SyncKeyboardIndicators(indicators) => {
            sync_keyboard_indicators(framed, active_stage, image, pending_rect, indicators, "user-sync").await;
        }
        ConnectionInput::ReleaseAllModifiers => {
            for (code, extended) in [
                (0x2A, false), // Left Shift
                (0x36, false), // Right Shift
                (0x1D, false), // Left Ctrl
                (0x1D, true),  // Right Ctrl
                (0x38, false), // Left Alt
                (0x38, true),  // Right Alt
                (0x5B, true),  // Left Super
                (0x5C, true),  // Right Super
            ] {
                let sc = Scancode::from_u8(extended, code);
                let events = db.apply([Operation::KeyReleased(sc)]);
                process_fastpath_events(framed, active_stage, image, pending_rect, &events).await?;
            }
        }
        ConnectionInput::Data(_) => {}
        ConnectionInput::RdpInput(rdp_input) => {
            let op_opt: Option<Operation> = match rdp_input {
                RdpInput::KeyboardScancode { code, extended, down } => {
                    let sc = Scancode::from_u8(extended, code);
                    Some(if down { Operation::KeyPressed(sc) } else { Operation::KeyReleased(sc) })
                }
                RdpInput::KeyboardUnicode { codepoint, down } => {
                    char::from_u32(u32::from(codepoint)).map(|ch| {
                        if down { Operation::UnicodeKeyPressed(ch) } else { Operation::UnicodeKeyReleased(ch) }
                    })
                }
                RdpInput::MouseMove { x, y } => {
                    Some(Operation::MouseMove(MousePosition { x, y }))
                }
                RdpInput::MouseButton { button, down } => {
                    let mb = match button {
                        RdpMouseButton::Left   => IrdpMouseButton::Left,
                        RdpMouseButton::Right  => IrdpMouseButton::Right,
                        RdpMouseButton::Middle => IrdpMouseButton::Middle,
                    };
                    Some(if down { Operation::MouseButtonPressed(mb) } else { Operation::MouseButtonReleased(mb) })
                }
                RdpInput::MouseWheel { delta } => {
                    Some(Operation::WheelRotations(WheelRotations { is_vertical: true, rotation_units: delta }))
                }
                RdpInput::MouseHorizontalWheel { delta } => {
                    Some(Operation::WheelRotations(WheelRotations { is_vertical: false, rotation_units: delta }))
                }
            };
            if let Some(op) = op_opt {
                let events = db.apply([op]);
                process_fastpath_events(framed, active_stage, image, pending_rect, &events).await?;
            }
        }
    }
    Ok(())
}

/// Decode an X224 frame as far as possible and write a human-readable summary
/// line (with timestamp) to `log`.  Never panics — decode failures are logged
/// as raw hex so no PDU is ever silently dropped from the trace.
fn log_x224_pdu(log: &mut std::fs::File, pdu_n: usize, frame: &[u8]) {
    use std::time::SystemTime;
    let ts = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);

    // Try to decode as MCS SendDataIndication
    let Ok(data_ctx) = connector::legacy::decode_send_data_indication(frame) else {
        let _ = writeln!(log, "[{ts}] #{pdu_n} X224 <decode-fail: not SendDataIndication> raw={}", hex_head(frame, 32));
        return;
    };

    let channel_id = data_ctx.channel_id;
    let initiator_id = data_ctx.initiator_id;

    let Ok(io_pdu) = connector::legacy::decode_io_channel(data_ctx) else {
        // Non-IO channel (SVC data: rdpsnd, drdynvc, cliprdr, …)
        // Try to peek the first byte to hint at the PDU type for DRDYNVC.
        let dvc_hint = if frame.len() > 10 {
            let b0 = frame[10]; // first byte of SVC user-data after MCS header (rough estimate)
            format!(" dvc_b0=0x{b0:02X}")
        } else {
            String::new()
        };
        let _ = writeln!(log, "[{ts}] #{pdu_n} SVC ch={channel_id}{dvc_hint} raw={}", hex_head(frame, 32));
        return;
    };

    match io_pdu {
        connector::legacy::IoChannelPdu::DeactivateAll(_) => {
            let _ = writeln!(log, "[{ts}] #{pdu_n} X224 ch={channel_id} ***DeactivateAll***");
        }
        connector::legacy::IoChannelPdu::Data(ctx) => {
            let name = ctx.pdu.as_short_name();
            let detail = match &ctx.pdu {
                ShareDataPdu::SetKeyboardIndicators(raw) => {
                    format!("***SetKeyboardIndicators*** raw={}", hex_head(raw, 8))
                }
                ShareDataPdu::SaveSessionInfo(_) => "SaveSessionInfo".to_string(),
                ShareDataPdu::ServerSetErrorInfo(e) => format!("ServerSetErrorInfo {:?}", e),
                other => other.as_short_name().to_string(),
            };
            let _ = writeln!(log, "[{ts}] #{pdu_n} X224 ch={channel_id} {name} {detail}");
        }
    }
}

fn log_fastpath_pdu(log: &mut std::fs::File, pdu_n: usize, frame: &[u8]) {
    let ts = now_millis();
    let _ = writeln!(
        log,
        "[{ts}] #{pdu_n} FastPath len={} raw={}",
        frame.len(),
        hex_head(frame, 24)
    );
}

fn now_millis() -> u128 {
    use std::time::SystemTime;

    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn hex_head(data: &[u8], n: usize) -> String {
    data.iter().take(n).map(|b| format!("{b:02X}")).collect::<Vec<_>>().join(" ")
}

fn try_handle_slowpath_bitmap(frame: &[u8]) -> Vec<FrameUpdate> {
    let mut updates = Vec::new();

    // Decode X224 → MCS SendDataIndication → ShareControl → ShareData
    let Ok(data_ctx) = connector::legacy::decode_send_data_indication(frame) else {
        return updates;
    };
    let Ok(io_channel) = connector::legacy::decode_io_channel(data_ctx) else {
        return updates;
    };

    let connector::legacy::IoChannelPdu::Data(ctx) = io_channel else {
        return updates;
    };

    let ShareDataPdu::Update(raw_update) = ctx.pdu else {
        return updates;
    };

    // Slow-path Update PDU data: updateType(u16) + pad(u16) + updateData
    if raw_update.len() < 4 {
        return updates;
    }

    let update_type = u16::from_le_bytes([raw_update[0], raw_update[1]]);

    // 0x0001 = UPDATETYPE_BITMAP
    if update_type != 0x0001 {
        return updates;
    }

    // BitmapUpdateData::decode reads updateType + numberRectangles itself, pass full raw_update
    let mut cursor = ReadCursor::new(&raw_update);
    let Ok(bitmap_update) = BitmapUpdateData::decode(&mut cursor) else {
        return updates;
    };

    for bmp in &bitmap_update.rectangles {
        let src_w = bmp.width as usize;
        let src_h = bmp.height as usize;
        let rect_w = bmp.rectangle.width() as usize;
        let rect_h = bmp.rectangle.height() as usize;
        if rect_w == 0 || rect_h == 0 || src_w == 0 || src_h == 0 {
            continue;
        }

        let is_compressed = bmp
            .compression_flags
            .contains(Compression::BITMAP_COMPRESSION);

        let rgba = if is_compressed && bmp.bits_per_pixel == 32 {
            // RDP6 bitmap stream → RGB24 output (true R,G,B order, bottom-up) → RGBA (top-down)
            let mut buf = Vec::new();
            let mut decoder = ironrdp::graphics::rdp6::BitmapStreamDecoder::default();
            if decoder
                .decode_bitmap_stream_to_rgb24(bmp.bitmap_data, &mut buf, src_w, src_h)
                .is_ok()
            {
                rgb24_to_rgba_flip(&buf, src_w, src_h, rect_w, rect_h)
            } else {
                continue;
            }
        } else if is_compressed && bmp.bits_per_pixel == 16 {
            // RLE-compressed 16bpp → raw RGB565 pixels → RGBA (top-down)
            let mut buf = Vec::new();
            match ironrdp::graphics::rle::decompress(
                bmp.bitmap_data,
                &mut buf,
                src_w,
                src_h,
                bmp.bits_per_pixel as usize,
            ) {
                Ok(ironrdp::graphics::rle::RlePixelFormat::Rgb16) => {
                    rgb16_to_rgba_flip(&buf, src_w, src_h, rect_w, rect_h)
                }
                _ => continue,
            }
        } else if !is_compressed && bmp.bits_per_pixel == 32 {
            // Uncompressed 32bpp BGRX (bottom-up) → RGBA (top-down)
            bgrx_to_rgba_flip(bmp.bitmap_data, src_w, src_h, rect_w, rect_h)
        } else if !is_compressed && bmp.bits_per_pixel == 16 {
            // Uncompressed 16bpp RGB565 (bottom-up) → RGBA (top-down)
            rgb16_to_rgba_flip(bmp.bitmap_data, src_w, src_h, rect_w, rect_h)
        } else if is_compressed && bmp.bits_per_pixel == 24 {
            // RLE 24bpp → BGR byte order → RGBA (top-down)
            let mut buf = Vec::new();
            match ironrdp::graphics::rle::decompress(
                bmp.bitmap_data,
                &mut buf,
                src_w,
                src_h,
                bmp.bits_per_pixel as usize,
            ) {
                Ok(ironrdp::graphics::rle::RlePixelFormat::Rgb24) => {
                    bgr24_to_rgba_flip(&buf, src_w, src_h, rect_w, rect_h)
                }
                _ => continue,
            }
        } else {
            continue;
        };

        updates.push(FrameUpdate::Rect {
            x: bmp.rectangle.left,
            y: bmp.rectangle.top,
            width: bmp.rectangle.width(),
            height: bmp.rectangle.height(),
            rgba,
        });
    }

    updates
}

/// Convert RGB24 (R,G,B byte order from RDP6 decoder) to RGBA, flipping bottom-up → top-down.
/// src_w/src_h = source buffer dimensions (may include padding), rect_w/rect_h = output dimensions.
fn rgb24_to_rgba_flip(rgb24: &[u8], src_w: usize, src_h: usize, rect_w: usize, rect_h: usize) -> Vec<u8> {
    let src_stride = src_w * 3;
    let copy_w = rect_w.min(src_w);
    let copy_h = rect_h.min(src_h);
    let mut rgba = vec![0u8; rect_w * rect_h * 4];
    for y in 0..copy_h {
        let src_y = src_h - 1 - y; // bottom-up flip
        let src_row = src_y * src_stride;
        let dst_row = y * rect_w * 4;
        for x in 0..copy_w {
            let si = src_row + x * 3;
            let di = dst_row + x * 4;
            if si + 2 < rgb24.len() {
                rgba[di] = rgb24[si];         // R (src is already RGB)
                rgba[di + 1] = rgb24[si + 1]; // G
                rgba[di + 2] = rgb24[si + 2]; // B
                rgba[di + 3] = 255;           // A
            }
        }
    }
    rgba
}

/// Convert BGR24 (B,G,R byte order from RLE 24bpp) to RGBA, flipping bottom-up → top-down.
fn bgr24_to_rgba_flip(bgr24: &[u8], src_w: usize, src_h: usize, rect_w: usize, rect_h: usize) -> Vec<u8> {
    let src_stride = src_w * 3;
    let copy_w = rect_w.min(src_w);
    let copy_h = rect_h.min(src_h);
    let mut rgba = vec![0u8; rect_w * rect_h * 4];
    for y in 0..copy_h {
        let src_y = src_h - 1 - y;
        let src_row = src_y * src_stride;
        let dst_row = y * rect_w * 4;
        for x in 0..copy_w {
            let si = src_row + x * 3;
            let di = dst_row + x * 4;
            if si + 2 < bgr24.len() {
                rgba[di] = bgr24[si + 2];     // R (src is BGR)
                rgba[di + 1] = bgr24[si + 1]; // G
                rgba[di + 2] = bgr24[si];     // B
                rgba[di + 3] = 255;           // A
            }
        }
    }
    rgba
}

fn rgb16_to_rgba_flip(rgb16: &[u8], src_w: usize, src_h: usize, rect_w: usize, rect_h: usize) -> Vec<u8> {
    let src_stride = src_w * 2;
    let copy_w = rect_w.min(src_w);
    let copy_h = rect_h.min(src_h);
    let mut rgba = vec![0u8; rect_w * rect_h * 4];
    for y in 0..copy_h {
        let src_y = src_h - 1 - y;
        let src_row = src_y * src_stride;
        let dst_row = y * rect_w * 4;
        for x in 0..copy_w {
            let si = src_row + x * 2;
            let di = dst_row + x * 4;
            if si + 1 < rgb16.len() {
                let pixel = u16::from_le_bytes([rgb16[si], rgb16[si + 1]]);
                let r = ((pixel >> 11) & 0x1F) as u8;
                let g = ((pixel >> 5) & 0x3F) as u8;
                let b = (pixel & 0x1F) as u8;
                rgba[di] = (r << 3) | (r >> 2);
                rgba[di + 1] = (g << 2) | (g >> 4);
                rgba[di + 2] = (b << 3) | (b >> 2);
                rgba[di + 3] = 255;
            }
        }
    }
    rgba
}

fn bgrx_to_rgba_flip(bgrx: &[u8], src_w: usize, src_h: usize, rect_w: usize, rect_h: usize) -> Vec<u8> {
    let src_stride = src_w * 4;
    let copy_w = rect_w.min(src_w);
    let copy_h = rect_h.min(src_h);
    let mut rgba = vec![0u8; rect_w * rect_h * 4];
    for y in 0..copy_h {
        let src_y = src_h - 1 - y;
        let src_row = src_y * src_stride;
        let dst_row = y * rect_w * 4;
        for x in 0..copy_w {
            let si = src_row + x * 4;
            let di = dst_row + x * 4;
            if si + 3 < bgrx.len() {
                rgba[di] = bgrx[si + 2];     // R
                rgba[di + 1] = bgrx[si + 1]; // G
                rgba[di + 2] = bgrx[si];     // B
                rgba[di + 3] = 255;          // A
            }
        }
    }
    rgba
}

fn merge_rect(a: InclusiveRectangle, b: InclusiveRectangle) -> InclusiveRectangle {
    InclusiveRectangle {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}

fn rect_update_from_image(image: &DecodedImage, rect: InclusiveRectangle) -> Option<FrameUpdate> {
    let width = rect.width();
    let height = rect.height();

    if width == 0 || height == 0 {
        return None;
    }

    let stride = image.stride();
    let bpp = image.bytes_per_pixel();
    if bpp != 4 {
        return None;
    }

    let row_bytes = usize::from(width) * bpp;
    let mut packed = Vec::with_capacity(row_bytes * usize::from(height));
    let data = image.data();

    for y in rect.top..=rect.bottom {
        let start = usize::from(y) * stride + usize::from(rect.left) * bpp;
        let end = start + row_bytes;
        if end > data.len() {
            return None;
        }
        packed.extend_from_slice(&data[start..end]);
    }

    Some(FrameUpdate::Rect {
        x: rect.left,
        y: rect.top,
        width,
        height,
        rgba: packed,
    })
}

type UpgradedFramed = Framed<MovableTokioStream<ironrdp_tls::TlsStream<tokio::net::TcpStream>>>;

// ── Phase 9-B-1: GFX DVC processor ──────────────────────────────────────────

/// Newtype wrapper so ClientPdu can be sent as a DvcMessage.
struct GfxClientMsg(ClientPdu);

impl IronEncode for GfxClientMsg {
    fn encode(&self, dst: &mut WriteCursor<'_>) -> EncodeResult<()> {
        self.0.encode(dst)
    }
    fn name(&self) -> &'static str { self.0.name() }
    fn size(&self) -> usize { self.0.size() }
}
impl DvcEncode for GfxClientMsg {}

/// Per-surface state tracked by GfxProcessor.
struct GfxSurface {
    output_origin_x: u32,
    output_origin_y: u32,
}

/// Minimal EGFX DVC processor: ZGFX decompress + ServerPdu dispatch.
/// Supports Uncompressed codec and FrameAcknowledge. Other codecs log a warning.
pub struct GfxProcessor {
    decompressor: Decompressor,
    surfaces: BTreeMap<u16, GfxSurface>,
    frame_tx: mpsc::UnboundedSender<Vec<crate::remote_display::FrameUpdate>>,
    frames_decoded: u32,
}

impl GfxProcessor {
    fn new(frame_tx: mpsc::UnboundedSender<Vec<crate::remote_display::FrameUpdate>>) -> Self {
        Self {
            decompressor: Decompressor::new(),
            surfaces: BTreeMap::new(),
            frame_tx,
            frames_decoded: 0,
        }
    }
}

impl_as_any!(GfxProcessor);

impl DvcProcessor for GfxProcessor {
    fn channel_name(&self) -> &str {
        "Microsoft::Windows::RDS::Graphics"
    }

    fn start(&mut self, _channel_id: u32) -> dvc_pdu::PduResult<Vec<DvcMessage>> {
        eprintln!("[GFX] channel opened, advertising capabilities V8..V10_7");
        let caps = ClientPdu::CapabilitiesAdvertise(CapabilitiesAdvertisePdu(vec![
            CapabilitySet::V8    { flags: CapabilitiesV8Flags::empty() },
            CapabilitySet::V8_1  { flags: CapabilitiesV81Flags::empty() },
            CapabilitySet::V10   { flags: CapabilitiesV10Flags::AVC_DISABLED },
            CapabilitySet::V10_1,
            CapabilitySet::V10_2 { flags: CapabilitiesV10Flags::AVC_DISABLED },
            CapabilitySet::V10_3 { flags: CapabilitiesV103Flags::AVC_DISABLED },
            CapabilitySet::V10_4 { flags: CapabilitiesV104Flags::AVC_DISABLED },
            CapabilitySet::V10_5 { flags: CapabilitiesV104Flags::AVC_DISABLED },
            CapabilitySet::V10_6 { flags: CapabilitiesV104Flags::AVC_DISABLED },
            CapabilitySet::V10_7 { flags: CapabilitiesV107Flags::AVC_DISABLED },
        ]));
        Ok(vec![Box::new(GfxClientMsg(caps))])
    }

    fn process(&mut self, _channel_id: u32, payload: &[u8]) -> dvc_pdu::PduResult<Vec<DvcMessage>> {
        let mut decompressed = Vec::new();
        if let Err(e) = self.decompressor.decompress(payload, &mut decompressed) {
            eprintln!("[GFX] ZGFX decompress error: {:?}", e);
            return Ok(vec![]);
        }

        let pdu = match ironrdp_core::decode::<ServerPdu>(&decompressed) {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[GFX] ServerPdu decode error: {:?}", e);
                return Ok(vec![]);
            }
        };

        let mut responses: Vec<DvcMessage> = Vec::new();

        match pdu {
            ServerPdu::CapabilitiesConfirm(confirm) => {
                eprintln!("[GFX] capabilities confirmed: {:?}", confirm);
            }
            ServerPdu::CreateSurface(create) => {
                self.surfaces.insert(create.surface_id, GfxSurface {
                    output_origin_x: 0,
                    output_origin_y: 0,
                });
            }
            ServerPdu::DeleteSurface(del) => {
                self.surfaces.remove(&del.surface_id);
            }
            ServerPdu::MapSurfaceToOutput(map) => {
                if let Some(s) = self.surfaces.get_mut(&map.surface_id) {
                    s.output_origin_x = map.output_origin_x;
                    s.output_origin_y = map.output_origin_y;
                }
            }
            ServerPdu::ResetGraphics(reset) => {
                self.surfaces.clear();
                eprintln!("[GFX] ResetGraphics: {}x{}", reset.width, reset.height);
            }
            ServerPdu::StartFrame(_) => {}
            ServerPdu::EndFrame(end) => {
                self.frames_decoded += 1;
                let ack = ClientPdu::FrameAcknowledge(FrameAcknowledgePdu {
                    queue_depth: QueueDepth::Unavailable,
                    frame_id: end.frame_id,
                    total_frames_decoded: self.frames_decoded,
                });
                responses.push(Box::new(GfxClientMsg(ack)));
            }
            ServerPdu::WireToSurface1(w2s) => {
                match w2s.codec_id {
                    Codec1Type::Uncompressed => {
                        let surface = match self.surfaces.get(&w2s.surface_id) {
                            Some(s) => s,
                            None => return Ok(responses),
                        };
                        let rect = &w2s.destination_rectangle;
                        let rect_w = rect.width();
                        let rect_h = rect.height();
                        let rgba = gfx_pixels_to_rgba(
                            &w2s.bitmap_data,
                            w2s.pixel_format,
                            rect_w as usize,
                            rect_h as usize,
                        );
                        let x = (surface.output_origin_x as u16).saturating_add(rect.left);
                        let y = (surface.output_origin_y as u16).saturating_add(rect.top);
                        let _ = self.frame_tx.send(vec![
                            crate::remote_display::FrameUpdate::Rect { x, y, width: rect_w, height: rect_h, rgba },
                        ]);
                    }
                    other => {
                        eprintln!("[GFX] WireToSurface1: unsupported codec {:?} — Phase 9-B-2/C", other);
                    }
                }
            }
            ServerPdu::WireToSurface2(w2s2) => {
                eprintln!("[GFX] WireToSurface2: unsupported codec {:?} — Phase 9-B-2/C", w2s2.codec_id);
            }
            _ => {}
        }

        Ok(responses)
    }

    fn close(&mut self, _channel_id: u32) {
        eprintln!("[GFX] channel closed");
    }
}

impl DvcClientProcessor for GfxProcessor {}

/// Convert GFX XRGB/ARGB (BGRX/BGRA in memory) pixel data to RGBA.
fn gfx_pixels_to_rgba(src: &[u8], fmt: GfxPixelFormat, width: usize, height: usize) -> Vec<u8> {
    let pixel_count = width * height;
    let mut dst = Vec::with_capacity(pixel_count * 4);
    for chunk in src.chunks_exact(4).take(pixel_count) {
        dst.push(chunk[2]); // R
        dst.push(chunk[1]); // G
        dst.push(chunk[0]); // B
        dst.push(match fmt { GfxPixelFormat::ARgb => chunk[3], _ => 255 }); // A
    }
    dst
}

// ─────────────────────────────────────────────────────────────────────────────

async fn connect(
    config: connector::Config,
    server_name: String,
    port: u16,
    gfx_frame_tx: mpsc::UnboundedSender<Vec<crate::remote_display::FrameUpdate>>,
    cliprdr_factory: Option<Box<dyn CliprdrBackendFactory + Send>>,
) -> Result<(ConnectionResult, UpgradedFramed), String> {
    let server_addr = tokio::net::lookup_host(format!("{}:{}", server_name, port))
        .await
        .map_err(|e| format!("resolve failed: {}", e))?
        .next()
        .ok_or_else(|| "socket address not found".to_string())?;

    let tcp_stream = tokio::net::TcpStream::connect(server_addr)
        .await
        .map_err(|e| format!("TCP connect failed: {}", e))?;

    let client_addr = tcp_stream
        .local_addr()
        .map_err(|e| format!("local_addr failed: {}", e))?;

    let mut connector = connector::ClientConnector::new(config, client_addr)
        .with_static_channel(Rdpsnd::new(Box::new(RdpsndBackend::new())))
        .with_static_channel(
            DrdynvcClient::new().with_dynamic_channel(GfxProcessor::new(gfx_frame_tx))
        );

    if let Some(factory) = cliprdr_factory {
        connector = connector.with_static_channel(Cliprdr::new(factory.build_cliprdr_backend()));
    }

    // Phase 1: pre-TLS handshake
    let mut framed: ironrdp_tokio::TokioFramed<tokio::net::TcpStream> = Framed::new(tcp_stream);
    let should_upgrade = connect_begin(&mut framed, &mut connector)
        .await
        .map_err(|e| format!("connect_begin failed: {}", e))?;

    // TLS upgrade via ironrdp-tls (replaces manual tls_upgrade + NoCertificateVerification)
    let initial_stream = framed.into_inner_no_leftover();
    let (tls_stream, tls_cert) = ironrdp_tls::upgrade(initial_stream, &server_name)
        .await
        .map_err(|e| format!("TLS upgrade failed: {}", e))?;

    let server_public_key = ironrdp_tls::extract_tls_server_public_key(&tls_cert)
        .ok_or_else(|| "missing server public key in TLS certificate".to_string())?
        .to_owned();

    let upgraded = mark_as_upgraded(should_upgrade, &mut connector);

    // Phase 2: post-TLS finalization
    let mut upgraded_framed: UpgradedFramed = Framed::new(tls_stream);
    let mut network_client = ironrdp_tokio::reqwest::ReqwestNetworkClient::new();

    let connection_result = connect_finalize(
        upgraded,
        connector,
        &mut upgraded_framed,
        &mut network_client,
        server_name.into(),
        server_public_key,
        None,
    )
    .await
    .map_err(|e| format!("connect_finalize failed: {:?}", e))?;

    Ok((connection_result, upgraded_framed))
}

fn build_config(username: String, password: String, domain: Option<String>) -> connector::Config {
    connector::Config {
        credentials: Credentials::UsernamePassword { username, password },
        domain,
        enable_tls: true,
        enable_credssp: true,
        keyboard_type: KeyboardType::IbmEnhanced,
        keyboard_subtype: 0,
        keyboard_layout: 0,
        keyboard_functional_keys_count: 12,
        ime_file_name: String::new(),
        dig_product_id: String::new(),
        desktop_size: connector::DesktopSize {
            width: 1280,
            height: 720,
        },
        bitmap: Some(BitmapConfig {
            lossy_compression: false,
            color_depth: 32,
            codecs: BitmapCodecs(vec![
                Codec {
                    id: 3, // CODEC_ID_REMOTEFX
                    property: CodecProperty::RemoteFx(RemoteFxContainer::ClientContainer(
                        RfxClientCapsContainer {
                            capture_flags: CaptureFlags::empty(),
                            caps_data: RfxCaps(RfxCapset(vec![RfxICap {
                                flags: RfxICapFlags::empty(),
                                entropy_bits: EntropyBits::Rlgr3,
                            }])),
                        },
                    )),
                },
            ]),
        }),
        client_build: 0,
        client_name: "kterm-rdp".to_owned(),
        client_dir: "C:\\Windows\\System32\\mstscax.dll".to_owned(),
        #[cfg(windows)]
        platform: MajorPlatformType::WINDOWS,
        #[cfg(target_os = "macos")]
        platform: MajorPlatformType::MACINTOSH,
        #[cfg(target_os = "ios")]
        platform: MajorPlatformType::IOS,
        #[cfg(target_os = "linux")]
        platform: MajorPlatformType::UNIX,
        #[cfg(target_os = "android")]
        platform: MajorPlatformType::ANDROID,
        #[cfg(target_os = "freebsd")]
        platform: MajorPlatformType::UNIX,
        #[cfg(target_os = "dragonfly")]
        platform: MajorPlatformType::UNIX,
        #[cfg(target_os = "openbsd")]
        platform: MajorPlatformType::UNIX,
        #[cfg(target_os = "netbsd")]
        platform: MajorPlatformType::UNIX,
        enable_server_pointer: false,
        request_data: None,
        autologon: false,
        enable_audio_playback: true,
        pointer_software_rendering: true,
        performance_flags: PerformanceFlags::ENABLE_FONT_SMOOTHING
            | PerformanceFlags::ENABLE_DESKTOP_COMPOSITION,
        desktop_scale_factor: 0,
        hardware_id: None,
        license_cache: None,
        timezone_info: TimezoneInfo::default(),
    }
}
