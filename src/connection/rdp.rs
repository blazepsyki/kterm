// SPDX-License-Identifier: MIT OR Apache-2.0

use iced::futures::{self, StreamExt};
use std::io::Write as _;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use ironrdp::connector;
use ironrdp::connector::{ConnectionResult, Credentials};
use ironrdp::pdu::gcc::KeyboardType;
use ironrdp::pdu::rdp::capability_sets::MajorPlatformType;
use ironrdp::pdu::rdp::client_info::{PerformanceFlags, TimezoneInfo};
use sspi::network_client::reqwest_network_client::ReqwestNetworkClient;
use tokio::sync::mpsc;
use tokio_rustls::rustls;
use x509_cert::der::Decode as _;

use crate::connection::{ConnectionEvent, ConnectionInput};

pub fn connect_and_subscribe(
    host: String,
    port: u16,
    user: String,
    password: String,
) -> futures::stream::BoxStream<'static, ConnectionEvent> {
    let (tx_to_rdp, rx_from_iced) = mpsc::unbounded_channel::<ConnectionInput>();

    let initial_state = RdpState::Init {
        host,
        port,
        user,
        password,
        tx_to_rdp,
        rx_from_iced,
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
            } => {
                let host_for_connect = host.clone();
                let user_for_connect = user.clone();
                let password_for_connect = password;

                let connect_result = tokio::task::spawn_blocking(move || {
                    perform_rdp_handshake(host_for_connect, port, user_for_connect, password_for_connect)
                })
                .await;

                match connect_result {
                    Ok(Ok(summary)) => {
                        let banner = format!(
                            "\r\n[RDP] IronRDP handshake completed: {}\r\n",
                            summary
                        )
                        .into_bytes();

                        Some((
                            ConnectionEvent::Connected(tx_to_rdp.clone()),
                            RdpState::Connected {
                                tx_to_rdp,
                                rx_from_iced,
                                pending_banner: Some(banner),
                            },
                        ))
                    }
                    Ok(Err(e)) => Some((ConnectionEvent::Error(format!("RDP Connect Error: {}", e)), RdpState::Finished)),
                    Err(e) => Some((ConnectionEvent::Error(format!("RDP Task Error: {}", e)), RdpState::Finished)),
                }
            }
            RdpState::Connected {
                tx_to_rdp,
                mut rx_from_iced,
                mut pending_banner,
            } => {
                if let Some(banner) = pending_banner.take() {
                    return Some((
                        ConnectionEvent::Data(banner),
                        RdpState::Connected {
                            tx_to_rdp,
                            rx_from_iced,
                            pending_banner,
                        },
                    ));
                }

                match rx_from_iced.recv().await {
                    Some(ConnectionInput::Data(_)) => Some((
                        ConnectionEvent::Data(vec![]),
                        RdpState::Connected {
                            tx_to_rdp,
                            rx_from_iced,
                            pending_banner,
                        },
                    )),
                    Some(ConnectionInput::Resize { cols, rows }) => {
                        let msg = format!(
                            "\r\n[RDP] Resize request queued: {}x{} (graphics pipeline mapping pending).\r\n",
                            cols, rows
                        )
                        .into_bytes();
                        Some((
                            ConnectionEvent::Data(msg),
                            RdpState::Connected {
                                tx_to_rdp,
                                rx_from_iced,
                                pending_banner,
                            },
                        ))
                    }
                    None => Some((ConnectionEvent::Disconnected, RdpState::Finished)),
                }
            }
            RdpState::Finished => None,
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
    },
    Connected {
        tx_to_rdp: mpsc::UnboundedSender<ConnectionInput>,
        rx_from_iced: mpsc::UnboundedReceiver<ConnectionInput>,
        pending_banner: Option<Vec<u8>>,
    },
    Finished,
}

fn perform_rdp_handshake(host: String, port: u16, username: String, password: String) -> Result<String, String> {
    let config = build_config(username, password, None);
    let (connection_result, _framed) = connect(config, host.clone(), port)?;

    Ok(format!(
        "server={} port={} desktop={}x{}",
        host,
        port,
        connection_result.desktop_size.width,
        connection_result.desktop_size.height
    ))
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
