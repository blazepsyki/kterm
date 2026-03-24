// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::futures::{self, StreamExt};
use std::io::Write as _;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use ironrdp::connector;
use ironrdp::connector::{BitmapConfig, ConnectionResult, Credentials};
use ironrdp::graphics::image_processing::PixelFormat;
use ironrdp::pdu::geometry::{InclusiveRectangle, Rectangle};
use ironrdp::pdu::input::fast_path::{FastPathInputEvent, KeyboardFlags};
use ironrdp::pdu::input::mouse::PointerFlags;
use ironrdp::pdu::input::MousePdu;
use ironrdp::pdu::gcc::KeyboardType;
use ironrdp::pdu::rdp::capability_sets::{MajorPlatformType, BitmapCodecs, Codec, CodecProperty, NsCodec};
use ironrdp::pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp::pdu::rdp::headers::ShareDataPdu;
use ironrdp::pdu::bitmap::{BitmapUpdateData, Compression};
use ironrdp::pdu::Action;
use ironrdp::session::image::DecodedImage;
use ironrdp::session::{ActiveStage, ActiveStageOutput};
use ironrdp_core::{ReadCursor, Decode as _};
use sspi::network_client::reqwest_network_client::ReqwestNetworkClient;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio_rustls::rustls;
use x509_cert::der::Decode as _;

use ironrdp_rdpsnd::client::Rdpsnd;
use ironrdp_rdpsnd_native::cpal::RdpsndBackend;

use crate::connection::{ConnectionEvent, ConnectionInput, RdpInput, RdpMouseButton};
use crate::remote_display::FrameUpdate;

pub fn connect_and_subscribe(
    host: String,
    port: u16,
    user: String,
    password: String,
) -> futures::stream::BoxStream<'static, ConnectionEvent> {
    let (tx_to_rdp, rx_from_iced) = mpsc::unbounded_channel::<ConnectionInput>();
    let (tx_from_worker, rx_from_worker) = mpsc::unbounded_channel::<ConnectionEvent>();

    tokio::task::spawn_blocking(move || {
        run_rdp_worker(host, port, user, password, rx_from_iced, tx_from_worker, tx_to_rdp);
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

fn run_rdp_worker(
    host: String,
    port: u16,
    username: String,
    password: String,
    rx_from_iced: mpsc::UnboundedReceiver<ConnectionInput>,
    tx_from_worker: mpsc::UnboundedSender<ConnectionEvent>,
    tx_to_rdp: mpsc::UnboundedSender<ConnectionInput>,
) {
    let tx_err = tx_from_worker.clone();
    if let Err(err) = run_rdp_worker_inner(host, port, username, password, rx_from_iced, tx_from_worker, tx_to_rdp) {
        let _ = tx_err.send(ConnectionEvent::Error(err));
    }
}

fn run_rdp_worker_inner(
    host: String,
    port: u16,
    username: String,
    password: String,
    mut rx_from_iced: mpsc::UnboundedReceiver<ConnectionInput>,
    tx_from_worker: mpsc::UnboundedSender<ConnectionEvent>,
    tx_to_rdp: mpsc::UnboundedSender<ConnectionInput>,
) -> Result<(), String> {
    let config = build_config(username, password, None);
    let (connection_result, mut framed) = connect(config, host.clone(), port)?;

    let width = connection_result.desktop_size.width;
    let height = connection_result.desktop_size.height;

    let mut image = DecodedImage::new(PixelFormat::RgbA32, width, height);
    let mut active_stage = ActiveStage::new(connection_result);

    // Emit Connected only AFTER handshake succeeds
    let _ = tx_from_worker.send(ConnectionEvent::Connected(tx_to_rdp));

    // Switch to fast polling timeout for the main loop
    set_fast_timeout(&framed);

    let mut graphics_updates = 0usize;
    let mut response_frames = 0usize;
    let mut processed_pdus = 0usize;
    let mut last_frame_emit = Instant::now();
    let mut pending_rect: Option<InclusiveRectangle> = None;
    let mut cursor_x: u16 = 0;
    let mut cursor_y: u16 = 0;

    let summary = format!(
        "\r\n[RDP] IronRDP handshake completed: server={} port={} desktop={}x{}\r\n",
        host, port, width, height
    );
    let _ = tx_from_worker.send(ConnectionEvent::Data(summary.into_bytes()));

    loop {
        loop {
            match rx_from_iced.try_recv() {
                Ok(input) => match input {
                    ConnectionInput::Resize { cols, rows } => {
                        let pixel_w = u32::from(cols).max(200).min(8192);
                        let pixel_h = u32::from(rows).max(200).min(8192);
                        if let Some(encoded) = active_stage.encode_resize(pixel_w, pixel_h, None, None) {
                            if let Ok(buf) = encoded {
                                let _ = framed.write_all(&buf);
                            }
                        }
                    }
                    ConnectionInput::Data(_) => {
                        // Keyboard/mouse input mapping is implemented in next step.
                    }
                    ConnectionInput::RdpInput(input) => {
                        let events = rdp_input_to_fastpath(input, &mut cursor_x, &mut cursor_y);
                        if !events.is_empty() {
                            if let Ok(outputs) = active_stage.process_fastpath_input(&mut image, &events) {
                                for out in outputs {
                                    match out {
                                        ActiveStageOutput::ResponseFrame(frame) => {
                                            let _ = framed.write_all(&frame);
                                        }
                                        ActiveStageOutput::GraphicsUpdate(rect) => {
                                            pending_rect = Some(match pending_rect {
                                                Some(prev) => merge_rect(prev, rect),
                                                None => rect,
                                            });
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                },
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    let _ = tx_from_worker.send(ConnectionEvent::Data(
                        b"\r\n[RDP] input channel closed; stopping RDP worker.\r\n".to_vec(),
                    ));
                    let _ = tx_from_worker.send(ConnectionEvent::Disconnected);
                    return Ok(());
                }
            }
        }

        let (action, payload) = match framed.read_pdu() {
            Ok((action, payload)) => (action, payload),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            // Windows ERROR_IO_PENDING (997) — non-blocking overlap, treat like WouldBlock
            Err(e) if e.raw_os_error() == Some(997) => {
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            Err(e) => return Err(format!("active stage read failed: {}", e)),
        };

        processed_pdus += 1;

        let outputs = match active_stage.process(&mut image, action, &payload) {
            Ok(outputs) => outputs,
            Err(e) => {
                // IronRDP does not handle slow-path Update PDUs natively.
                // Try to decode as slow-path bitmap update and send directly.
                if action == Action::X224 {
                    let frame_updates = try_handle_slowpath_bitmap(&payload);
                    if !frame_updates.is_empty() {
                        // Batch all rects from this PDU into a single event —
                        // UI side will apply() each to the buffer and flush once.
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
                        if let Some(rect) = pending_rect.take() {
                            if let Some(update) = rect_update_from_image(&image, rect) {
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
                _ => {}
            }
        }

        if graphics_updates > 0 && last_frame_emit.elapsed() >= Duration::from_millis(50) {
            if let Some(rect) = pending_rect.take() {
                if let Some(update) = rect_update_from_image(&image, rect) {
                    let _ = tx_from_worker.send(ConnectionEvent::Frames(vec![update]));
                }
            }
            last_frame_emit = Instant::now();
        }
    }
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

fn rdp_input_to_fastpath(input: RdpInput, cursor_x: &mut u16, cursor_y: &mut u16) -> Vec<FastPathInputEvent> {
    match input {
        RdpInput::KeyboardScancode { code, extended, down } => {
            let mut flags = KeyboardFlags::empty();
            if !down {
                flags |= KeyboardFlags::RELEASE;
            }
            if extended {
                flags |= KeyboardFlags::EXTENDED;
            }
            vec![FastPathInputEvent::KeyboardEvent(flags, code)]
        }
        RdpInput::KeyboardUnicode { codepoint, down } => {
            let mut flags = KeyboardFlags::empty();
            if !down {
                flags |= KeyboardFlags::RELEASE;
            }
            vec![FastPathInputEvent::UnicodeKeyboardEvent(flags, codepoint)]
        }
        RdpInput::MouseMove { x, y } => {
            *cursor_x = x;
            *cursor_y = y;
            vec![FastPathInputEvent::MouseEvent(MousePdu {
                flags: PointerFlags::MOVE,
                number_of_wheel_rotation_units: 0,
                x_position: x,
                y_position: y,
            })]
        }
        RdpInput::MouseButton { button, down } => {
            let mut flags = match button {
                RdpMouseButton::Left => PointerFlags::LEFT_BUTTON,
                RdpMouseButton::Right => PointerFlags::RIGHT_BUTTON,
                RdpMouseButton::Middle => PointerFlags::MIDDLE_BUTTON_OR_WHEEL,
            };
            if down {
                flags |= PointerFlags::DOWN;
            }

            vec![FastPathInputEvent::MouseEvent(MousePdu {
                flags,
                number_of_wheel_rotation_units: 0,
                x_position: *cursor_x,
                y_position: *cursor_y,
            })]
        }
        RdpInput::MouseWheel { delta } => {
            vec![FastPathInputEvent::MouseEvent(MousePdu {
                flags: PointerFlags::VERTICAL_WHEEL | PointerFlags::MIDDLE_BUTTON_OR_WHEEL,
                number_of_wheel_rotation_units: delta,
                x_position: *cursor_x,
                y_position: *cursor_y,
            })]
        }
    }
}

type UpgradedFramed = ironrdp_blocking::Framed<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>;

fn connect(
    config: connector::Config,
    server_name: String,
    port: u16,
) -> Result<(ConnectionResult, UpgradedFramed), String> {
    let server_addr = lookup_addr(&server_name, port)?;
    let tcp_stream = TcpStream::connect(server_addr).map_err(|e| format!("TCP connect failed: {}", e))?;

    // Use generous timeout during handshake; caller reduces after connect.
    tcp_stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| format!("set_read_timeout failed: {}", e))?;

    let client_addr = tcp_stream
        .local_addr()
        .map_err(|e| format!("local_addr failed: {}", e))?;

    let mut framed = ironrdp_blocking::Framed::new(tcp_stream);
    let mut connector = connector::ClientConnector::new(config, client_addr)
        .with_static_channel(Rdpsnd::new(Box::new(RdpsndBackend::new())));

    let should_upgrade = ironrdp_blocking::connect_begin(&mut framed, &mut connector)
        .map_err(|e| format!("connect_begin failed: {}", e))?;

    let initial_stream = framed.into_inner_no_leftover();
    let (upgraded_stream, server_public_key) = tls_upgrade(initial_stream, server_name.clone())?;
    let upgraded = ironrdp_blocking::mark_as_upgraded(should_upgrade, &mut connector);

    let mut upgraded_framed = ironrdp_blocking::Framed::new(upgraded_stream);
    let mut network_client = ReqwestNetworkClient;

    let connection_result = ironrdp_blocking::connect_finalize(
        upgraded,
        connector,
        &mut upgraded_framed,
        &mut network_client,
        server_name.into(),
        server_public_key,
        None,
    )
    .map_err(|e| format!("connect_finalize failed: {:?}", e))?;

    Ok((connection_result, upgraded_framed))
}

/// Reduce TCP read timeout for the main loop (non-blocking polling).
fn set_fast_timeout(framed: &UpgradedFramed) {
    let (stream, _) = framed.get_inner();
    stream
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(16)))
        .ok();
}

fn build_config(username: String, password: String, domain: Option<String>) -> connector::Config {
    connector::Config {
        credentials: Credentials::UsernamePassword { username, password },
        domain,
        enable_tls: true,
        enable_credssp: false,
        keyboard_type: KeyboardType::IbmEnhanced,
        keyboard_subtype: 0,
        keyboard_layout: 0,
        keyboard_functional_keys_count: 12,
        ime_file_name: String::new(),
        dig_product_id: String::new(),
        desktop_size: connector::DesktopSize {
            width: 1280,
            height: 1024,
        },
        bitmap: Some(BitmapConfig {
            lossy_compression: false,
            color_depth: 32,
            codecs: BitmapCodecs(vec![
                Codec {
                    id: 1, // NSCodec
                    property: CodecProperty::NsCodec(NsCodec {
                        is_dynamic_fidelity_allowed: true,
                        is_subsampling_allowed: false,
                        color_loss_level: 1, // minimum loss
                    }),
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

fn lookup_addr(hostname: &str, port: u16) -> Result<std::net::SocketAddr, String> {
    let addr = (hostname, port)
        .to_socket_addrs()
        .map_err(|e| format!("resolve failed: {}", e))?
        .next()
        .ok_or_else(|| "socket address not found".to_string())?;
    Ok(addr)
}

fn tls_upgrade(
    stream: TcpStream,
    server_name: String,
) -> Result<(rustls::StreamOwned<rustls::ClientConnection, TcpStream>, Vec<u8>), String> {
    let mut config = rustls::client::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(std::sync::Arc::new(danger::NoCertificateVerification))
        .with_no_client_auth();

    config.resumption = rustls::client::Resumption::disabled();

    let config = std::sync::Arc::new(config);
    let server_name = server_name
        .try_into()
        .map_err(|e: rustls::pki_types::InvalidDnsNameError| format!("invalid server name: {}", e))?;

    let client = rustls::ClientConnection::new(config, server_name)
        .map_err(|e| format!("TLS client creation failed: {}", e))?;

    let mut tls_stream = rustls::StreamOwned::new(client, stream);
    tls_stream
        .flush()
        .map_err(|e| format!("TLS flush failed: {}", e))?;

    let cert = tls_stream
        .conn
        .peer_certificates()
        .and_then(|certificates| certificates.first())
        .ok_or_else(|| "peer certificate is missing".to_string())?;

    let server_public_key = extract_tls_server_public_key(cert.as_ref())?;

    Ok((tls_stream, server_public_key))
}

fn extract_tls_server_public_key(cert: &[u8]) -> Result<Vec<u8>, String> {
    let cert = x509_cert::Certificate::from_der(cert).map_err(|e| format!("certificate decode failed: {}", e))?;

    let server_public_key = cert
        .tbs_certificate
        .subject_public_key_info
        .subject_public_key
        .as_bytes()
        .ok_or_else(|| "subject public key BIT STRING is not aligned".to_string())?
        .to_owned();

    Ok(server_public_key)
}

mod danger {
    use tokio_rustls::rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
    use tokio_rustls::rustls::{DigitallySignedStruct, Error, SignatureScheme, pki_types};

    #[derive(Debug)]
    pub(super) struct NoCertificateVerification;

    impl ServerCertVerifier for NoCertificateVerification {
        fn verify_server_cert(
            &self,
            _: &pki_types::CertificateDer<'_>,
            _: &[pki_types::CertificateDer<'_>],
            _: &pki_types::ServerName<'_>,
            _: &[u8],
            _: pki_types::UnixTime,
        ) -> Result<ServerCertVerified, Error> {
            Ok(ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _: &[u8],
            _: &pki_types::CertificateDer<'_>,
            _: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _: &[u8],
            _: &pki_types::CertificateDer<'_>,
            _: &DigitallySignedStruct,
        ) -> Result<HandshakeSignatureValid, Error> {
            Ok(HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
            vec![
                SignatureScheme::RSA_PKCS1_SHA1,
                SignatureScheme::ECDSA_SHA1_Legacy,
                SignatureScheme::RSA_PKCS1_SHA256,
                SignatureScheme::ECDSA_NISTP256_SHA256,
                SignatureScheme::RSA_PKCS1_SHA384,
                SignatureScheme::ECDSA_NISTP384_SHA384,
                SignatureScheme::RSA_PKCS1_SHA512,
                SignatureScheme::ECDSA_NISTP521_SHA512,
                SignatureScheme::RSA_PSS_SHA256,
                SignatureScheme::RSA_PSS_SHA384,
                SignatureScheme::RSA_PSS_SHA512,
                SignatureScheme::ED25519,
                SignatureScheme::ED448,
            ]
        }
    }
}
