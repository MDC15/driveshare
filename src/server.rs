use std::net::SocketAddr;
use std::sync::Arc;

use axum::extract::connect_info::ConnectInfo;
use axum::routing::{any, get, post, put};
use axum::Router;
use hyper::body::Incoming;
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::broadcast;
use tokio_rustls::TlsAcceptor;
use hyper_util::rt::TokioIo;
use tower::ServiceExt;
use tower_http::trace::TraceLayer;
use tracing::info;

use crate::config::Config;
use crate::device_manager;
use crate::ui;
use crate::watcher;
use crate::webdav::{self, AppState};

fn is_wsl() -> bool {
    std::env::var("WSL_DISTRO_NAME").is_ok()
        || std::path::Path::new("/proc/sys/fs/binfmt_misc/WSLInterop").exists()
}

fn get_windows_ip() -> Option<String> {
    let output = std::process::Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-Command",
            "(Get-NetIPAddress -AddressFamily IPv4 | Where-Object { \
             $_.InterfaceAlias -notlike '*Loopback*' -and \
             $_.InterfaceAlias -notlike '*vEthernet*' -and \
             $_.PrefixOrigin -ne 'WellKnown' }).IPAddress | Select-Object -First 1",
        ])
        .output()
        .ok()?;
    if output.status.success() {
        let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !ip.is_empty() {
            return Some(ip);
        }
    }
    None
}

fn get_local_ip() -> String {
    if is_wsl() {
        if let Some(ip) = get_windows_ip() {
            return ip;
        }
    }

    let socket = match std::net::UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return "127.0.0.1".to_string(),
    };
    if socket.connect("8.8.8.8:80").is_err() {
        return "127.0.0.1".to_string();
    }
    match socket.local_addr() {
        Ok(addr) => addr.ip().to_string(),
        Err(_) => "127.0.0.1".to_string(),
    }
}

fn load_certs(path: &str) -> anyhow::Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let certfile = std::fs::File::open(path)
        .map_err(|e| anyhow::anyhow!("Failed to open cert file '{}': {}", path, e))?;
    let mut reader = std::io::BufReader::new(certfile);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("Failed to load certs: {}", e))?;
    Ok(certs)
}

fn load_key(path: &str) -> anyhow::Result<rustls::pki_types::PrivateKeyDer<'static>> {
    let keyfile = std::fs::File::open(path)
        .map_err(|e| anyhow::anyhow!("Failed to open key file '{}': {}", path, e))?;
    let mut reader = std::io::BufReader::new(keyfile);
    let key = rustls_pemfile::private_key(&mut reader)
        .map_err(|e| anyhow::anyhow!("Failed to load private key: {}", e))?
        .ok_or_else(|| anyhow::anyhow!("No private key found in '{}'", path))?;
    Ok(key)
}

pub async fn start(config: Config) -> anyhow::Result<()> {
    for share in &config.shares {
        let path = std::path::Path::new(&share.path);
        if !path.exists() {
            info!("Creating share directory: {}", share.path);
            std::fs::create_dir_all(path)?;
        }
    }

    let server_ip = get_local_ip();
    let (event_tx, _) = broadcast::channel::<watcher::FsEvent>(256);
    let watcher_manager = watcher::WatcherManager::new(&config.shares, event_tx.clone());
    let device_manager = Arc::new(device_manager::DeviceManager::new());

    let state = AppState {
        config: Arc::new(config.clone()),
        server_ip: server_ip.clone(),
        event_tx: event_tx.clone(),
        _watchers: Arc::new(watcher_manager),
        device_manager: device_manager.clone(),
    };

    let app = Router::new()
        .route("/", get(ui::dashboard))
        .route("/api/shares", get(ui::api_shares))
        .route("/api/files/*path", get(ui::api_files))
        .route("/api/ip", get(ui::api_ip))
        .route("/api/zip/*path", get(ui::api_zip))
        .route("/api/events", get(ui::api_events))
        .route("/api/devices", get(ui::api_devices_list))
        .route("/api/devices/online", get(ui::api_devices_online))
        .route("/api/devices/block/:ip", post(ui::api_device_block))
        .route("/api/devices/unblock/:ip", post(ui::api_device_unblock))
        .route("/api/devices/permissions/:ip", put(ui::api_device_permissions))
        .route("/*path", any(webdav::handler))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr = format!("{}:{}", config.server.host, config.server.port);

    let is_tls = config.server.tls_cert.is_some() && config.server.tls_key.is_some();
    let protocol = if is_tls { "https" } else { "http" };

    let local_url = format!("{}://127.0.0.1:{}", protocol, config.server.port);

    info!("DriveShare starting on {}://{}", protocol, addr);
    info!("Local:    {}", local_url);
    info!("Network:  {}://{}:{}", protocol, server_ip, config.server.port);
    info!("Shares:");
    for share in &config.shares {
        let abs_path = std::path::Path::new(&share.path)
            .canonicalize()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| share.path.clone());
        info!("  /{} -> {}  (absolute: {})", share.name, share.path, abs_path);
    }

    if !is_tls {
        if let Err(e) = webbrowser::open(&local_url) {
            info!("Could not open browser: {}", e);
        }
    }

    let listener = match TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            if e.kind() == std::io::ErrorKind::AddrInUse {
                anyhow::bail!(
                    "Port {} is already in use by another process.\n\
                     Use a different port:  driveshare -P <port>\n\
                     Or check which process is using it:\n\
                     Windows: netstat -ano | findstr :{}\n\
                     Linux:   ss -tlnp | grep {}",
                    config.server.port, config.server.port, config.server.port
                );
            }
            return Err(e.into());
        }
    };

    let event_tx_for_shutdown = event_tx.clone();
    let graceful = async move {
        shutdown_signal().await;
        let _ = event_tx_for_shutdown.send(watcher::FsEvent { kind: "shutdown".to_string() });
    };

    if is_tls {
        serve_tls(listener, app, &config, event_tx.clone()).await
    } else {
        axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
            .with_graceful_shutdown(graceful)
            .await?;
        info!("Server stopped.");
        Ok(())
    }
}

async fn serve_tls(
    listener: TcpListener,
    app: Router,
    config: &Config,
    event_tx: broadcast::Sender<watcher::FsEvent>,
) -> anyhow::Result<()> {
    let cert_path = config.server.tls_cert.as_ref().unwrap();
    let key_path = config.server.tls_key.as_ref().unwrap();

    let certs = load_certs(cert_path)?;
    let key = load_key(key_path)?;

    let tls_server_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|e| anyhow::anyhow!("Invalid TLS certificate/key: {}", e))?;

    let tls_acceptor = TlsAcceptor::from(Arc::new(tls_server_config));

    info!("TLS enabled (cert: {}, key: {})", cert_path, key_path);

    let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);

    tokio::spawn(async move {
        shutdown_signal().await;
        let _ = event_tx.send(watcher::FsEvent { kind: "shutdown".to_string() });
        let _ = stop_tx.send(true);
    });

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, peer_addr) = match result {
                    Ok(s) => s,
                    Err(e) => {
                        info!("Accept error: {}", e);
                        continue;
                    }
                };

                let app = app.clone();
                let tls_acceptor = tls_acceptor.clone();

                tokio::spawn(async move {
                    let tls_stream = match tls_acceptor.accept(stream).await {
                        Ok(s) => s,
                        Err(e) => {
                            info!("TLS handshake error: {}", e);
                            return;
                        }
                    };

                    let svc = hyper::service::service_fn(move |req: hyper::Request<Incoming>| {
                        let app = app.clone();
                        let addr = peer_addr;
                        async move {
                            let (mut parts, body) = req.into_parts();
                            parts.extensions.insert(ConnectInfo::<SocketAddr>(addr));
                            use http_body_util::BodyExt as _;
                            let body = body
                                .map_err(|e| anyhow::Error::from(e))
                                .boxed();
                            let req = axum::http::Request::from_parts(parts, body);
                            let res = match ServiceExt::oneshot(app, req).await {
                                Ok(r) => r,
                                Err(e) => match e {},
                            };
                            Ok::<_, anyhow::Error>(res)
                        }
                    });

                    let io = TokioIo::new(tls_stream);

                    if let Err(e) = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, svc)
                        .await
                    {
                        info!("TLS connection error: {}", e);
                    }
                });
            }
            _ = stop_rx.changed() => {
                info!("Received shutdown signal, stopping TLS server...");
                break;
            }
        }
    }

    info!("Server stopped.");
    Ok(())
}

pub fn ensure_self_signed_cert(cfg: &mut crate::config::Config) -> anyhow::Result<()> {
    if cfg.server.tls_cert.is_some() && cfg.server.tls_key.is_some() {
        return Ok(());
    }

    let cert_dir = directories::ProjectDirs::from("com", "driveshare", "driveshare")
        .map(|d| d.config_dir().join("tls"))
        .unwrap_or_else(|| std::path::PathBuf::from("./tls"));
    std::fs::create_dir_all(&cert_dir)?;

    let cert_file = cert_dir.join("cert.pem");
    let key_file = cert_dir.join("key.pem");

    if !cert_file.exists() || !key_file.exists() {
        let lan_ip = get_local_ip();
        let (cert_pem, key_pem) = generate_self_signed_cert(&["localhost", "127.0.0.1", &lan_ip])?;
        std::fs::write(&cert_file, &cert_pem)?;
        std::fs::write(&key_file, &key_pem)?;
    }

    cfg.server.tls_cert = Some(cert_file.to_string_lossy().to_string());
    cfg.server.tls_key = Some(key_file.to_string_lossy().to_string());

    Ok(())
}

fn generate_self_signed_cert(sans: &[&str]) -> anyhow::Result<(String, String)> {
    let sans: Vec<String> = sans.iter().map(|s| s.to_string()).collect();
    let cert = rcgen::generate_simple_self_signed(sans)?;
    let cert_pem = cert.serialize_pem()?;
    let key_pem = cert.get_key_pair().serialize_pem();
    Ok((cert_pem, key_pem))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C, shutting down...");
        }
        _ = terminate => {
            info!("Received SIGTERM, shutting down...");
        }
    }
}
