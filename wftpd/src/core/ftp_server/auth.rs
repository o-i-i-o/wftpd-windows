//! libunftp authenticator adapter
//!
//! Adapts existing UserManager to libunftp's Authenticator trait
//! with Fail2Ban integration and user home directory support

use argon2::{Argon2, password_hash::PasswordHasher};
use async_trait::async_trait;
use parking_lot::Mutex;
use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use unftp_core::auth::{
    AuthenticationError, Authenticator, Credentials, Principal, UserDetail, UserDetailError,
    UserDetailProvider,
};
use unftp_sbe_restrict::{UserWithPermissions, VfsOperations};
use unftp_sbe_rooter::UserWithRoot;

use crate::core::config::Config;
use crate::core::fail2ban::Fail2BanManager;
use crate::core::users::{Permissions, UserManager};

#[derive(Debug, Default)]
pub struct SessionTracker {
    sessions: parking_lot::Mutex<HashMap<String, Vec<String>>>,
}

impl SessionTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, username: &str, client_ip: String) {
        let mut sessions = self.sessions.lock();
        sessions
            .entry(username.to_string())
            .or_default()
            .push(client_ip);
    }

    pub fn unregister(&self, username: &str) -> Option<String> {
        let mut sessions = self.sessions.lock();
        if let Some(ips) = sessions.get_mut(username)
            && !ips.is_empty()
        {
            let ip = ips.remove(0);
            if ips.is_empty() {
                sessions.remove(username);
            }
            return Some(ip);
        }
        None
    }
}

#[derive(Debug, Clone)]
pub struct WftpdUser {
    pub username: String,
    pub home_dir: PathBuf,
    pub enabled: bool,
    pub permissions: Permissions,
}

impl PartialEq for WftpdUser {
    fn eq(&self, other: &Self) -> bool {
        self.username == other.username
    }
}

impl Eq for WftpdUser {}

impl UserDetail for WftpdUser {
    fn account_enabled(&self) -> bool {
        self.enabled
    }

    fn home(&self) -> Option<&Path> {
        Some(&self.home_dir)
    }
}

impl fmt::Display for WftpdUser {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "User(username: {:?}, home: {:?})",
            self.username, self.home_dir
        )
    }
}

impl UserWithRoot for WftpdUser {
    fn user_root(&self) -> Option<PathBuf> {
        Some(self.home_dir.clone())
    }
}

impl UserWithPermissions for WftpdUser {
    fn permissions(&self) -> VfsOperations {
        let mut ops = VfsOperations::empty();

        if self.permissions.can_read {
            ops |= VfsOperations::GET;
        }
        if self.permissions.can_write {
            ops |= VfsOperations::PUT;
        }
        if self.permissions.can_delete {
            ops |= VfsOperations::DEL;
        }
        if self.permissions.can_list {
            ops |= VfsOperations::LIST;
        }
        if self.permissions.can_mkdir {
            ops |= VfsOperations::MK_DIR;
        }
        if self.permissions.can_rmdir {
            ops |= VfsOperations::RM_DIR;
        }
        if self.permissions.can_rename {
            ops |= VfsOperations::RENAME;
        }
        if self.permissions.can_append {
            ops |= VfsOperations::PUT;
        }

        ops
    }
}

fn reload_users_if_needed(users: &mut UserManager, users_path: &std::path::Path) {
    if let Err(e) = users.reload(users_path) {
        tracing::warn!("Failed to reload users from {:?}: {}", users_path, e);
    }
}

#[derive(Debug)]
pub struct WftpdAuthenticator {
    user_manager: Arc<Mutex<UserManager>>,
    users_path: std::path::PathBuf,
    fail2ban_manager: Option<Arc<Fail2BanManager>>,
    dummy_hash: String,
    config: Option<Arc<Mutex<Config>>>,
    session_tracker: Arc<SessionTracker>,
}

impl WftpdAuthenticator {
    pub fn new(
        user_manager: Arc<Mutex<UserManager>>,
        users_path: std::path::PathBuf,
        fail2ban_manager: Option<Arc<Fail2BanManager>>,
        config: Option<Arc<Mutex<Config>>>,
        session_tracker: Arc<SessionTracker>,
    ) -> Self {
        let dummy_hash = Self::generate_dummy_hash();
        WftpdAuthenticator {
            user_manager,
            users_path,
            fail2ban_manager,
            dummy_hash,
            config,
            session_tracker,
        }
    }

    fn generate_dummy_hash() -> String {
        use argon2::Params;

        let params = Params::new(65536, 3, 4, Some(32)).unwrap_or_else(|_| Params::default());
        let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
        argon2
            .hash_password(b"dummy_password_for_constant_time_verification")
            .map(|h| h.to_string())
            .unwrap_or_else(|_| {
                "$argon2id$v=19$m=65536,t=3,p=4$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_string()
            })
    }

    fn get_client_ip(creds: &Credentials) -> String {
        match creds.source_ip {
            IpAddr::V4(ip) => ip.to_string(),
            IpAddr::V6(ip) => ip.to_string(),
        }
    }

    fn check_ip_security(&self, client_ip: &str) -> Result<(), AuthenticationError> {
        if let Some(config) = &self.config {
            let cfg = config.lock();

            if !cfg.is_ip_allowed(client_ip) {
                tracing::warn!(
                    ip = %client_ip,
                    action = "IP_BLOCKED",
                    protocol = "FTP",
                    "Connection from IP {} blocked by security policy", client_ip
                );
                return Err(AuthenticationError::BadPassword);
            }

            if !cfg.check_connection_limits(client_ip) {
                tracing::warn!(
                    ip = %client_ip,
                    action = "CONNECTION_LIMIT",
                    protocol = "FTP",
                    "Connection limit exceeded for IP {}", client_ip
                );
                return Err(AuthenticationError::BadPassword);
            }
        }
        Ok(())
    }

    fn try_register_connection(&self, client_ip: &str) -> bool {
        if let Some(config) = &self.config {
            let cfg = config.lock();
            cfg.try_register_connection(client_ip)
        } else {
            true
        }
    }

    fn unregister_connection(&self, client_ip: &str) {
        if let Some(config) = &self.config {
            let cfg = config.lock();
            cfg.unregister_connection(client_ip);
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
        let client_ip = Self::get_client_ip(creds);

        self.check_ip_security(&client_ip)?;

        if let Some(ref fail2ban_manager) = self.fail2ban_manager
            && fail2ban_manager.is_banned(&client_ip).await
        {
            tracing::warn!(
                ip = %client_ip,
                username = %username,
                action = "AUTH_BANNED",
                protocol = "FTP",
                "Rejected login from banned IP {} for user {}", client_ip, username
            );
            return Err(AuthenticationError::BadPassword);
        }

        if !self.try_register_connection(&client_ip) {
            tracing::warn!(
                ip = %client_ip,
                username = %username,
                action = "CONNECTION_LIMIT",
                protocol = "FTP",
                "Connection limit reached for IP {}", client_ip
            );
            return Err(AuthenticationError::BadPassword);
        }

        let password = creds
            .password
            .as_ref()
            .ok_or(AuthenticationError::BadPassword)?;

        let auth_result = {
            let mut users = self.user_manager.lock();
            reload_users_if_needed(&mut users, &self.users_path);

            let user = users.get_user(username).cloned();
            match user {
                Some(_) => {
                    let auth_ok = users.authenticate(username, password);
                    (true, auth_ok)
                }
                None => {
                    let _ = UserManager::verify_password(password, &self.dummy_hash);
                    (false, Ok(false))
                }
            }
        };

        match auth_result {
            (true, Ok(true)) => {
                self.session_tracker.register(username, client_ip.clone());
                if let Some(ref fail2ban_manager) = self.fail2ban_manager {
                    fail2ban_manager.reset_failures(&client_ip).await;
                }
                tracing::info!(
                    username = %username,
                    ip = %client_ip,
                    action = "LOGIN",
                    protocol = "FTP",
                    "User {} logged in via password from {}", username, client_ip
                );
                Ok(Principal {
                    username: username.to_string(),
                })
            }
            (true, Ok(false)) => {
                self.unregister_connection(&client_ip);
                if let Some(ref fail2ban_manager) = self.fail2ban_manager {
                    fail2ban_manager.add_failure(&client_ip).await;
                }
                tracing::warn!(
                    username = %username,
                    ip = %client_ip,
                    action = "AUTH_FAIL",
                    protocol = "FTP",
                    "Failed login attempt for user {} from {}", username, client_ip
                );
                Err(AuthenticationError::BadPassword)
            }
            (false, _) => {
                self.unregister_connection(&client_ip);
                if let Some(ref fail2ban_manager) = self.fail2ban_manager {
                    fail2ban_manager.add_failure(&client_ip).await;
                }
                tracing::warn!(
                    username = %username,
                    ip = %client_ip,
                    action = "AUTH_FAIL",
                    protocol = "FTP",
                    "Failed login attempt for user {} from {}", username, client_ip
                );
                Err(AuthenticationError::BadPassword)
            }
            (_, Err(_)) => {
                self.unregister_connection(&client_ip);
                if let Some(ref fail2ban_manager) = self.fail2ban_manager {
                    fail2ban_manager.add_failure(&client_ip).await;
                }
                tracing::warn!(
                    username = %username,
                    ip = %client_ip,
                    action = "AUTH_FAIL",
                    protocol = "FTP",
                    "Failed login attempt for user {} from {}", username, client_ip
                );
                Err(AuthenticationError::BadPassword)
            }
        }
    }

    fn name(&self) -> &str {
        "WftpdAuthenticator"
    }
}

#[derive(Debug)]
pub struct WftpdUserDetailProvider {
    user_manager: Arc<Mutex<UserManager>>,
    users_path: std::path::PathBuf,
}

impl WftpdUserDetailProvider {
    pub fn new(user_manager: Arc<Mutex<UserManager>>, users_path: std::path::PathBuf) -> Self {
        WftpdUserDetailProvider {
            user_manager,
            users_path,
        }
    }
}

#[async_trait]
impl UserDetailProvider for WftpdUserDetailProvider {
    type User = WftpdUser;

    async fn provide_user_detail(
        &self,
        principal: &Principal,
    ) -> Result<WftpdUser, UserDetailError> {
        let username = &principal.username;

        let user = {
            let mut users = self.user_manager.lock();
            reload_users_if_needed(&mut users, &self.users_path);
            users.get_user(username).cloned()
        };

        match user {
            Some(u) => Ok(WftpdUser {
                username: u.username,
                home_dir: PathBuf::from(&u.home_dir),
                enabled: u.enabled,
                permissions: u.permissions,
            }),
            None => Err(UserDetailError::new(format!(
                "User not found: {}",
                username
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::users::Permissions;
    use std::path::Path;
    use unftp_sbe_restrict::VfsOperations;

    fn create_test_user() -> WftpdUser {
        WftpdUser {
            username: "testuser".to_string(),
            home_dir: PathBuf::from("/home/testuser"),
            enabled: true,
            permissions: Permissions::full(),
        }
    }

    #[test]
    fn test_wftpd_user_display() {
        let user = create_test_user();
        let display = format!("{}", user);
        assert!(display.contains("testuser"));
    }

    #[test]
    fn test_wftpd_user_debug() {
        let user = create_test_user();
        let debug = format!("{:?}", user);
        assert!(debug.contains("testuser"));
    }

    #[test]
    fn test_user_detail_account_enabled() {
        let user = create_test_user();
        assert!(user.account_enabled());

        let mut disabled_user = create_test_user();
        disabled_user.enabled = false;
        assert!(!disabled_user.account_enabled());
    }

    #[test]
    fn test_user_detail_home() {
        let user = create_test_user();
        assert_eq!(user.home(), Some(Path::new("/home/testuser")));
    }

    #[test]
    fn test_user_with_root() {
        let user = create_test_user();
        let root = user.user_root();
        assert_eq!(root, Some(PathBuf::from("/home/testuser")));
    }

    #[test]
    fn test_user_with_permissions_full() {
        let user = create_test_user();
        let perms = user.permissions();

        assert!(perms.contains(VfsOperations::GET));
        assert!(perms.contains(VfsOperations::PUT));
        assert!(perms.contains(VfsOperations::DEL));
        assert!(perms.contains(VfsOperations::LIST));
        assert!(perms.contains(VfsOperations::MK_DIR));
        assert!(perms.contains(VfsOperations::RM_DIR));
        assert!(perms.contains(VfsOperations::RENAME));
    }

    #[test]
    fn test_user_with_permissions_partial() {
        let mut user = create_test_user();
        user.permissions = Permissions {
            can_read: true,
            can_write: false,
            can_delete: false,
            can_list: true,
            can_mkdir: false,
            can_rmdir: false,
            can_rename: false,
            can_append: false,
            quota_mb: None,
            speed_limit_kbps: None,
        };

        let perms = user.permissions();
        assert!(perms.contains(VfsOperations::GET));
        assert!(perms.contains(VfsOperations::LIST));
        assert!(!perms.contains(VfsOperations::PUT));
        assert!(!perms.contains(VfsOperations::DEL));
    }

    #[test]
    fn test_user_with_permissions_empty() {
        let mut user = create_test_user();
        user.permissions = Permissions::default();

        let perms = user.permissions();
        assert_eq!(perms, VfsOperations::empty());
    }

    #[test]
    fn test_permissions_mapping_can_read() {
        let mut user = create_test_user();
        user.permissions = Permissions {
            can_read: true,
            ..Permissions::default()
        };

        let perms = user.permissions();
        assert!(perms.contains(VfsOperations::GET));
        assert!(!perms.contains(VfsOperations::PUT));
    }

    #[test]
    fn test_permissions_mapping_can_write() {
        let mut user = create_test_user();
        user.permissions = Permissions {
            can_write: true,
            ..Permissions::default()
        };

        let perms = user.permissions();
        assert!(perms.contains(VfsOperations::PUT));
    }

    #[test]
    fn test_permissions_mapping_can_append() {
        let mut user = create_test_user();
        user.permissions = Permissions {
            can_append: true,
            ..Permissions::default()
        };

        let perms = user.permissions();
        assert!(perms.contains(VfsOperations::PUT));
    }

    #[test]
    fn test_permissions_mapping_can_mkdir() {
        let mut user = create_test_user();
        user.permissions = Permissions {
            can_mkdir: true,
            ..Permissions::default()
        };

        let perms = user.permissions();
        assert!(perms.contains(VfsOperations::MK_DIR));
    }

    #[test]
    fn test_permissions_mapping_can_rmdir() {
        let mut user = create_test_user();
        user.permissions = Permissions {
            can_rmdir: true,
            ..Permissions::default()
        };

        let perms = user.permissions();
        assert!(perms.contains(VfsOperations::RM_DIR));
    }

    #[test]
    fn test_permissions_mapping_can_rename() {
        let mut user = create_test_user();
        user.permissions = Permissions {
            can_rename: true,
            ..Permissions::default()
        };

        let perms = user.permissions();
        assert!(perms.contains(VfsOperations::RENAME));
    }
}
