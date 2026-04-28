//! libunftp authenticator adapter
//!
//! Adapts existing UserManager to libunftp's Authenticator trait
//! with Fail2Ban integration

use async_trait::async_trait;
use parking_lot::Mutex;
use std::sync::Arc;
use unftp_core::auth::{Authenticator, AuthenticationError, Credentials, Principal};

use crate::core::fail2ban::Fail2BanManager;
use crate::core::users::UserManager;

#[derive(Debug)]
pub struct WftpdAuthenticator {
    user_manager: Arc<Mutex<UserManager>>,
    users_path: std::path::PathBuf,
    fail2ban_manager: Option<Arc<Fail2BanManager>>,
}

impl WftpdAuthenticator {
    pub fn new(
        user_manager: Arc<Mutex<UserManager>>,
        users_path: std::path::PathBuf,
        fail2ban_manager: Option<Arc<Fail2BanManager>>,
    ) -> Self {
        WftpdAuthenticator {
            user_manager,
            users_path,
            fail2ban_manager,
        }
    }
}

#[async_trait]
impl Authenticator for WftpdAuthenticator {
    async fn authenticate(
        &self,
        username: &str,
        creds: &Credentials,
    ) -> Result<Principal, AuthenticationError> {
        let password = creds
            .password
            .as_ref()
            .ok_or(AuthenticationError::BadPassword)?;

        let auth_result = {
            let mut users = self.user_manager.lock();

            if users.get_user(username).is_none()
                && let Err(e) = users.reload(&self.users_path)
            {
                tracing::warn!("Failed to reload users during FTP authentication: {}", e);
            }

            users.authenticate(username, password)
        };

        match auth_result {
            Ok(true) => {
                tracing::info!(
                    username = %username,
                    action = "LOGIN",
                    protocol = "FTP",
                    "User {} logged in via password", username
                );
                Ok(Principal {
                    username: username.to_string(),
                })
            }
            Ok(false) => {
                if let Some(ref fail2ban_manager) = self.fail2ban_manager {
                    fail2ban_manager.add_failure(username).await;
                }
                tracing::warn!(
                    username = %username,
                    action = "AUTH_FAIL",
                    protocol = "FTP",
                    "Failed login attempt for user {}", username
                );
                Err(AuthenticationError::BadPassword)
            }
            Err(e) => {
                tracing::error!(
                    username = %username,
                    action = "AUTH_ERROR",
                    protocol = "FTP",
                    "Authentication error for user {}: {}", username, e
                );
                Err(AuthenticationError::BadUser)
            }
        }
    }

    fn name(&self) -> &str {
        "WftpdAuthenticator"
    }
}
