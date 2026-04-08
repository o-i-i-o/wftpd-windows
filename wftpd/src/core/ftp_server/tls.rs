//! FTP TLS 配置和加密连接
//!
//! 处理 FTP over TLS (FTPS) 的加密连接和证书配置

use anyhow::Result;
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::TlsAcceptor;
use tokio_rustls::server::TlsStream as AsyncTlsStream;

use super::cert_gen;

pub type AsyncTlsTcpStream = AsyncTlsStream<TcpStream>;

#[derive(Clone)]
pub struct TlsConfig {
    pub acceptor: Option<Arc<TlsAcceptor>>,
}

impl TlsConfig {
    pub fn new(cert_path: Option<&str>, key_path: Option<&str>, _require_ssl: bool) -> Self {
        match (cert_path, key_path) {
            (Some(cert), Some(key)) => {
                // 检查并自动生成证书
                match cert_gen::ensure_cert_exists(cert, key) {
                    Ok(true) => tracing::info!("已为 FTPS 生成自签名证书"),
                    Ok(false) => tracing::info!("使用现有的 FTPS 证书"),
                    Err(e) => tracing::warn!("检查证书失败：{}", e),
                }

                match load_tls_acceptor(cert, key) {
                    Ok(acceptor) => {
                        tracing::info!("TLS enabled with certificate: {}", cert);
                        TlsConfig {
                            acceptor: Some(Arc::new(acceptor)),
                        }
                    }
                    Err(e) => {
                        tracing::error!("Failed to load TLS certificate: {}", e);
                        TlsConfig { acceptor: None }
                    }
                }
            }
            _ => TlsConfig { acceptor: None },
        }
    }

    pub fn is_tls_available(&self) -> bool {
        self.acceptor.is_some()
    }
}

fn load_tls_acceptor(cert_path: &str, key_path: &str) -> Result<TlsAcceptor> {
    let cert_file = Path::new(cert_path);
    let key_file = Path::new(key_path);

    // 读取证书文件
    let cert_data = fs::read(cert_file)?;

    // 读取私钥文件
    let key_data = fs::read(key_file)?;

    // 解析 PEM 格式的证书
    let mut cert_chain: Vec<CertificateDer<'static>> = Vec::new();
    let cert_str = String::from_utf8_lossy(&cert_data);
    for pem in pem::parse_many(cert_str.as_bytes())? {
        if pem.tag() == "CERTIFICATE" {
            cert_chain.push(CertificateDer::from(pem.contents().to_vec()));
        }
    }

    if cert_chain.is_empty() {
        anyhow::bail!("未找到有效的证书");
    }

    // 解析私钥（支持 PKCS8、PKCS1 或 EC）
    let mut private_key: Option<PrivateKeyDer<'static>> = None;
    let key_str = String::from_utf8_lossy(&key_data);
    for pem in pem::parse_many(key_str.as_bytes())? {
        match pem.tag() {
            "PRIVATE KEY" | "RSA PRIVATE KEY" | "EC PRIVATE KEY" => {
                private_key = Some(
                    PrivateKeyDer::try_from(pem.contents().to_vec())
                        .map_err(|e| anyhow::anyhow!("私钥解析失败：{}", e))?,
                );
                break;
            }
            _ => {}
        }
    }

    let key = private_key.ok_or_else(|| anyhow::anyhow!("未找到有效的私钥"))?;

    // 构建 TLS 配置，rustls 0.23 默认已启用安全协议（TLS 1.2/1.3）
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)?;

    tracing::info!("TLS acceptor configured with secure defaults (TLS 1.2/1.3 only)");

    let acceptor = TlsAcceptor::from(Arc::new(config));
    Ok(acceptor)
}
