//! FTPS certificate generator
//!
//! Generates self-signed X.509 SSL/TLS certificates using rcgen

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use tracing::{info, warn};

pub fn generate_self_signed_cert(cert_path: &str, key_path: &str) -> Result<()> {
    info!("Generating self-signed certificate for FTPS...");

    let cert_dir = Path::new(cert_path)
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Invalid certificate path"))?;

    fs::create_dir_all(cert_dir).context("Failed to create certificate directory")?;

    let mut params = rcgen::CertificateParams::default();
    params.distinguished_name = rcgen::DistinguishedName::new();
    params
        .distinguished_name
        .push(rcgen::DnType::CommonName, "WFTPG FTP Server");
    params
        .distinguished_name
        .push(rcgen::DnType::OrganizationName, "WFTPG");
    params.subject_alt_names = vec![
        rcgen::SanType::DnsName("localhost".try_into()?),
        rcgen::SanType::IpAddress(std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1))),
        rcgen::SanType::IpAddress(std::net::IpAddr::V6(std::net::Ipv6Addr::new(
            0, 0, 0, 0, 0, 0, 0, 1,
        ))),
    ];

    params.not_before = time::OffsetDateTime::now_utc();
    params.not_after = params.not_before + time::Duration::days(3650);

    let key_pair = rcgen::KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    let key_pem = key_pair.serialize_pem();
    fs::write(key_path, key_pem).context("Failed to save private key file")?;
    info!("Private key saved to: {}", key_path);

    let cert_pem = cert.pem();
    fs::write(cert_path, cert_pem).context("保存证书文件失败")?;
    info!("证书已保存到：{}", cert_path);

    info!("FTPS 自签名证书生成成功");
    Ok(())
}

pub fn ensure_cert_exists(cert_path: &str, key_path: &str) -> Result<bool> {
    let cert_file = Path::new(cert_path);
    let key_file = Path::new(key_path);

    if cert_file.exists() && key_file.exists() {
        info!("FTPS certificate file already exists");
        return Ok(false);
    }

    if cert_file.exists() != key_file.exists() {
        warn!("证书文件或私钥文件只有一个存在，将重新生成");
        if cert_file.exists() {
            let _ = fs::remove_file(cert_file);
        }
        if key_file.exists() {
            let _ = fs::remove_file(key_file);
        }
    }

    generate_self_signed_cert(cert_path, key_path)?;
    Ok(true)
}
