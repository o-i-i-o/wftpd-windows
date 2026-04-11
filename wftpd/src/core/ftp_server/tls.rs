//! FTP TLS configuration and encrypted connections
//!
//! Handles encrypted connections and certificate configuration for FTP over TLS (FTPS)

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
                // Check and auto-generate certificate
                match cert_gen::ensure_cert_exists(cert, key) {
                    Ok(true) => tracing::info!("Generated self-signed certificate for FTPS"),
                    Ok(false) => tracing::info!("Using existing FTPS certificate"),
                    Err(e) => tracing::warn!("Certificate check failed: {}", e),
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

    // Read certificate file
    let cert_data = fs::read(cert_file)?;

    // Read private key file
    let key_data = fs::read(key_file)?;

    // Parse PEM format certificate
    let mut cert_chain: Vec<CertificateDer<'static>> = Vec::new();
    let cert_str = String::from_utf8_lossy(&cert_data);
    for pem in pem::parse_many(cert_str.as_bytes())? {
        if pem.tag() == "CERTIFICATE" {
            cert_chain.push(CertificateDer::from(pem.contents().to_vec()));
        }
    }

    if cert_chain.is_empty() {
        anyhow::bail!("No valid certificate found");
    }

    // Parse private key (supports PKCS8, PKCS1 or EC)
    let mut private_key: Option<PrivateKeyDer<'static>> = None;
    let key_str = String::from_utf8_lossy(&key_data);
    for pem in pem::parse_many(key_str.as_bytes())? {
        match pem.tag() {
            "PRIVATE KEY" | "RSA PRIVATE KEY" | "EC PRIVATE KEY" => {
                private_key = Some(
                    PrivateKeyDer::try_from(pem.contents().to_vec())
                        .map_err(|e| anyhow::anyhow!("Private key parsing failed: {}", e))?,
                );
                break;
            }
            _ => {}
        }
    }

    let key = private_key.ok_or_else(|| anyhow::anyhow!("No valid private key found"))?;

    // Build TLS configuration, rustls 0.23 has secure protocols enabled by default (TLS 1.2/1.3)
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)?;

    tracing::info!("TLS acceptor configured with secure defaults (TLS 1.2/1.3 only)");

    let acceptor = TlsAcceptor::from(Arc::new(config));
    Ok(acceptor)
}
