use crate::config::Config;

pub fn load_certs(path: &str) -> anyhow::Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let certfile = std::fs::File::open(path)
        .map_err(|e| anyhow::anyhow!("Failed to open cert file '{}': {}", path, e))?;
    let mut reader = std::io::BufReader::new(certfile);
    let certs = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("Failed to load certs: {}", e))?;
    Ok(certs)
}

pub fn load_key(path: &str) -> anyhow::Result<rustls::pki_types::PrivateKeyDer<'static>> {
    let keyfile = std::fs::File::open(path)
        .map_err(|e| anyhow::anyhow!("Failed to open key file '{}': {}", path, e))?;
    let mut reader = std::io::BufReader::new(keyfile);
    let key = rustls_pemfile::private_key(&mut reader)
        .map_err(|e| anyhow::anyhow!("Failed to load private key: {}", e))?
        .ok_or_else(|| anyhow::anyhow!("No private key found in '{}'", path))?;
    Ok(key)
}

pub fn ensure_self_signed_cert(cfg: &mut Config) -> anyhow::Result<()> {
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
        let lan_ip = super::get_local_ip();
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
