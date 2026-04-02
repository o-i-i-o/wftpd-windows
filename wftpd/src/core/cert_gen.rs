use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use tracing::{info, warn};

/// 生成自签名 SSL 证书
pub fn generate_self_signed_cert(cert_path: &str, key_path: &str) -> Result<()> {
    info!("正在为 FTPS 生成自签名证书...");

    // 确保证书目录存在
    let cert_dir = Path::new(cert_path)
        .parent()
        .ok_or_else(|| anyhow::anyhow!("无效的证书路径"))?;

    fs::create_dir_all(cert_dir).context("创建证书目录失败")?;

    // 使用 rcgen 生成自签名证书
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
    ];

    // 设置证书有效期：10 年
    params.not_before = time::OffsetDateTime::now_utc();
    params.not_after = params.not_before + time::Duration::days(3650);

    // 生成证书（rcgen 0.14+ API）
    let key_pair = rcgen::KeyPair::generate()?;
    let cert = params.self_signed(&key_pair)?;

    // 保存私钥（PEM 格式）
    let key_pem = key_pair.serialize_pem();
    fs::write(key_path, key_pem).context("保存私钥文件失败")?;
    info!("私钥已保存到：{}", key_path);

    // 保存证书（PEM 格式）
    let cert_pem = cert.pem();
    fs::write(cert_path, cert_pem).context("保存证书文件失败")?;
    info!("证书已保存到：{}", cert_path);

    info!("FTPS 自签名证书生成成功");
    Ok(())
}

/// 检查并生成证书（如果不存在）
pub fn ensure_cert_exists(cert_path: &str, key_path: &str) -> Result<bool> {
    let cert_file = Path::new(cert_path);
    let key_file = Path::new(key_path);

    // 如果两个文件都存在，不需要生成
    if cert_file.exists() && key_file.exists() {
        info!("FTPS 证书文件已存在");
        return Ok(false);
    }

    // 如果一个存在另一个不存在，警告用户
    if cert_file.exists() != key_file.exists() {
        warn!("证书文件或私钥文件只有一个存在，将重新生成");
        // 删除已存在的文件，避免混淆
        if cert_file.exists() {
            let _ = fs::remove_file(cert_file);
        }
        if key_file.exists() {
            let _ = fs::remove_file(key_file);
        }
    }

    // 生成新证书
    generate_self_signed_cert(cert_path, key_path)?;
    Ok(true)
}

/// 验证证书是否有效
pub fn validate_cert(cert_path: &str, key_path: &str) -> Result<()> {
    if !Path::new(cert_path).exists() {
        anyhow::bail!("证书文件不存在：{}", cert_path);
    }

    if !Path::new(key_path).exists() {
        anyhow::bail!("私钥文件不存在：{}", key_path);
    }

    // 尝试读取文件以验证权限
    fs::read(cert_path).context("无法读取证书文件")?;
    fs::read(key_path).context("无法读取私钥文件")?;

    Ok(())
}
