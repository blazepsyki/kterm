// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::futures::{self, StreamExt};
use std::io::Write as _;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};

use ironrdp::connector;
use ironrdp::connector::{ConnectionResult, Credentials};
use ironrdp::graphics::image_processing::PixelFormat;
use ironrdp::pdu::geometry::{InclusiveRectangle, Rectangle};
use ironrdp::pdu::input::fast_path::{FastPathInputEvent, KeyboardFlags};
use ironrdp::pdu::input::mouse::PointerFlags;
use ironrdp::pdu::input::MousePdu;
use ironrdp::pdu::gcc::KeyboardType;
use ironrdp::pdu::rdp::capability_sets::MajorPlatformType;
use ironrdp::pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use ironrdp::session::image::DecodedImage;
use ironrdp::session::{ActiveStage, ActiveStageOutput};
use sspi::network_client::reqwest_network_client::ReqwestNetworkClient;
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TryRecvError;
use tokio_rustls::rustls;
use x509_cert::der::Decode as _;

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

    let initial_state = RdpState::Init {
        host,
        port,
        user,
        password,
        tx_to_rdp,
        rx_from_iced,
        rx_from_worker,
        tx_from_worker,
    };

    futures::stream::unfold(initial_state, |state| async move {
        match state {
            RdpState::Init {
                host,
                port,
                user,
                password,
                tx_to_rdp,
                rx_from_iced,
                rx_from_worker,
                tx_from_worker,
            } => {
                tokio::task::spawn_blocking(move || {
                    run_rdp_worker(host, port, user, password, rx_from_iced, tx_from_worker);
                });

                Some((
                    ConnectionEvent::Connected(tx_to_rdp),
                    RdpState::Connected { rx_from_worker },
                ))
            }
            RdpState::Connected { mut rx_from_worker } => match rx_from_worker.recv().await {
                Some(event) => Some((event, RdpState::Connected { rx_from_worker })),
                None => None,
            }
        }
    })
    .boxed()
}

enum RdpState {
    Init {
        host: String,
        port: u16,
        user: String,
        password: String,
        tx_to_rdp: mpsc::UnboundedSender<ConnectionInput>,
        rx_from_iced: mpsc::UnboundedReceiver<ConnectionInput>,
        rx_from_worker: mpsc::UnboundedReceiver<ConnectionEvent>,
        tx_from_worker: mpsc::UnboundedSender<ConnectionEvent>,
    },
    Connected {
        rx_from_worker: mpsc::UnboundedReceiver<ConnectionEvent>,
    },
}

fn run_rdp_worker(
    host: String,
    port: u16,
    username: String,
    password: String,
    rx_from_iced: mpsc::UnboundedReceiver<ConnectionInput>,
    tx_from_worker: mpsc::UnboundedSender<ConnectionEvent>,
) {
    let tx_err = tx_from_worker.clone();
    if let Err(err) = run_rdp_worker_inner(host, port, username, password, rx_from_iced, tx_from_worker) {
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
) -> Result<(), String> {
    let config = build_config(username, password, None);
    let (connection_result, mut framed) = connect(config, host.clone(), port)?;

    let width = connection_result.desktop_size.width;
    let height = connection_result.desktop_size.height;

    let mut image = DecodedImage::new(PixelFormat::RgbA32, width, height);
    let mut active_stage = ActiveStage::new(connection_result);

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
                        let pixel_w = u32::from(cols).saturating_mul(16).max(200).min(8192);
                        let pixel_h = u32::from(rows).saturating_mul(16).max(200).min(8192);
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
                continue;
            }
            Err(e) => return Err(format!("active stage read failed: {}", e)),
        };

        processed_pdus += 1;

        let outputs = active_stage
            .process(&mut image, action, &payload)
            .map_err(|e| format!("active stage process failed: {}", e))?;

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

                    if last_frame_emit.elapsed() >= Duration::from_millis(33) {
                        if let Some(rect) = pending_rect.take() {
                            if let Some(update) = rect_update_from_image(&image, rect) {
                                let _ = tx_from_worker.send(ConnectionEvent::Frame(update));
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

        if graphics_updates > 0 && last_frame_emit.elapsed() >= Duration::from_millis(120) {
            if let Some(rect) = pending_rect.take() {
                if let Some(update) = rect_update_from_image(&image, rect) {
                    let _ = tx_from_worker.send(ConnectionEvent::Frame(update));
                }
            }
            last_frame_emit = Instant::now();
        }
    }
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

    tcp_stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("set_read_timeout failed: {}", e))?;

    let client_addr = tcp_stream
        .local_addr()
        .map_err(|e| format!("local_addr failed: {}", e))?;

    let mut framed = ironrdp_blocking::Framed::new(tcp_stream);
    let mut connector = connector::ClientConnector::new(config, client_addr);

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
    .map_err(|e| format!("connect_finalize failed: {}", e))?;

    Ok((connection_result, upgraded_framed))
}

fn build_config(username: String, password: String, domain: Option<String>) -> connector::Config {
    connector::Config {
        credentials: Credentials::UsernamePassword { username, password },
        domain,
        enable_tls: false,
        enable_credssp: true,
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
        bitmap: None,
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
        enable_audio_playback: false,
        pointer_software_rendering: true,
        performance_flags: PerformanceFlags::default(),
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
