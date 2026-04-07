//! 用户管理模块
//!
//! 提供用户认证、权限管理和密码哈希功能
//!
//! # 安全特性
//!
//! - 使用 Argon2 进行密码哈希（推荐的安全算法）
//! - 支持用户启用/禁用状态
//! - 细粒度的权限控制
//! - 审计日志记录

use anyhow::{Context, Result};
use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::Path;

use crate::core::error::UserError;

/// 用户信息
///
/// 包含用户的基本信息、权限和状态
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct User {
    pub username: String,
    pub password_hash: String,
    #[serde(default)]
    pub home_dir: String,
    #[serde(default)]
    pub permissions: Permissions,
    pub created_at: DateTime<Utc>,
    pub last_login: Option<DateTime<Utc>>,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub is_admin: bool,
}

fn default_enabled() -> bool {
    true
}

/// 用户权限
///
/// 定义用户对文件和目录的操作权限
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct Permissions {
    pub can_read: bool,
    pub can_write: bool,
    pub can_delete: bool,
    pub can_list: bool,
    pub can_mkdir: bool,
    pub can_rmdir: bool,
    pub can_rename: bool,
    pub can_append: bool,
    pub quota_mb: Option<u64>,
    pub speed_limit_kbps: Option<u64>,
}

impl Permissions {
    /// 创建完全权限
    pub fn full() -> Self {
        Permissions {
            can_read: true,
            can_write: true,
            can_delete: true,
            can_list: true,
            can_mkdir: true,
            can_rmdir: true,
            can_rename: true,
            can_append: true,
            quota_mb: None,
            speed_limit_kbps: None,
        }
    }

    /// 检查是否有读取权限
    pub fn can_read(&self) -> bool {
        self.can_read
    }

    /// 检查是否有写入权限
    pub fn can_write(&self) -> bool {
        self.can_write
    }

    /// 检查是否有删除权限
    pub fn can_delete(&self) -> bool {
        self.can_delete
    }
}

impl fmt::Display for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut perms = Vec::new();
        if self.can_read {
            perms.push("读");
        }
        if self.can_write {
            perms.push("写");
        }
        if self.can_delete {
            perms.push("删");
        }
        if self.can_list {
            perms.push("列表");
        }
        if self.can_mkdir {
            perms.push("建目录");
        }
        if self.can_rmdir {
            perms.push("删目录");
        }
        if self.can_rename {
            perms.push("重命名");
        }
        if self.can_append {
            perms.push("追加");
        }
        write!(f, "{}", perms.join(","))
    }
}

/// 用户管理器
///
/// 管理用户的增删改查和认证
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserManager {
    users: HashMap<String, User>,
}

impl UserManager {
    /// 创建新的空用户管理器
    pub fn new() -> Self {
        Self::default()
    }

    /// 从文件加载用户数据
    ///
    /// 如果文件不存在或解析失败，返回空的用户管理器
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::new());
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read users file: {}", e);
                return Ok(Self::new());
            }
        };

        if content.trim().is_empty() {
            return Ok(Self::new());
        }

        let manager: UserManager = match serde_json::from_str(&content) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("Failed to parse users file: {}", e);
                return Ok(Self::new());
            }
        };

        Ok(manager)
    }

    /// 保存用户数据到文件
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("Failed to create users directory")?;
        }

        let content = serde_json::to_string_pretty(self).context("Failed to serialize users")?;

        fs::write(path, content).context("Failed to write users file")?;

        Ok(())
    }

    /// 对密码进行哈希处理
    ///
    /// # Security
    ///
    /// 使用 Argon2 算法，自动生成随机盐值
    fn hash_password(password: &str) -> Result<String, UserError> {
        // 生成随机盐值
        let mut salt_bytes = [0u8; 16];
        getrandom::getrandom(&mut salt_bytes)
            .map_err(|e| UserError::PasswordHashFailed(e.to_string()))?;
        let salt = SaltString::encode_b64(&salt_bytes)
            .map_err(|e| UserError::PasswordHashFailed(e.to_string()))?;

        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| UserError::PasswordHashFailed(e.to_string()))?
            .to_string();
        Ok(hash)
    }

    /// 验证密码
    ///
    /// # Arguments
    /// * `password` - 待验证的明文密码
    /// * `hash` - 存储的密码哈希
    ///
    /// # Returns
    /// 验证成功返回 true，否则返回 false
    fn verify_password(password: &str, hash: &str) -> bool {
        let parsed_hash = match PasswordHash::new(hash) {
            Ok(h) => h,
            Err(_) => return false,
        };
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed_hash)
            .is_ok()
    }

    /// 添加新用户
    ///
    /// # Arguments
    /// * `username` - 用户名
    /// * `password` - 密码
    /// * `home_dir` - 用户主目录
    /// * `is_admin` - 是否为管理员
    ///
    /// # Errors
    /// 如果用户已存在或主目录无效，返回错误
    pub fn add_user(
        &mut self,
        username: &str,
        password: &str,
        home_dir: &str,
        is_admin: bool,
    ) -> Result<(), UserError> {
        if self.users.contains_key(username) {
            return Err(UserError::UserAlreadyExists(username.to_string()));
        }

        let home_dir = home_dir.trim();
        if home_dir.is_empty() {
            return Err(UserError::InvalidHomeDirectory(
                "用户主目录不能为空".to_string(),
            ));
        }

        Self::validate_and_prepare_home_dir(home_dir)
            .map_err(|e| UserError::InvalidHomeDirectory(e.to_string()))?;

        let password_hash = Self::hash_password(password)?;
        let user = User {
            username: username.to_string(),
            password_hash,
            home_dir: home_dir.to_string(),
            permissions: Permissions::full(),
            created_at: Utc::now(),
            last_login: None,
            enabled: true,
            is_admin,
        };

        self.users.insert(username.to_string(), user);
        tracing::info!("用户已添加：{}", username);
        Ok(())
    }

    /// 验证并准备用户主目录
    ///
    /// 验证逻辑：
    /// 1. 目录不能为空
    /// 2. 如果目录存在，验证是有效目录且路径可规范化
    /// 3. 如果目录不存在，尝试创建（确保父目录存在）
    fn validate_and_prepare_home_dir(home_dir: &str) -> Result<()> {
        let path = std::path::Path::new(home_dir);

        if home_dir.trim().is_empty() {
            anyhow::bail!("用户主目录不能为空");
        }

        if path.exists() {
            if !path.is_dir() {
                anyhow::bail!("用户主目录不是有效目录：{}", home_dir);
            }
            match path.canonicalize() {
                Ok(_) => Ok(()),
                Err(e) => anyhow::bail!("用户主目录路径无效 '{}': {}", home_dir, e),
            }
        } else {
            // 目录不存在时尝试创建
            match std::fs::create_dir_all(path) {
                Ok(_) => {
                    tracing::info!("已创建用户主目录：{}", home_dir);
                    Ok(())
                }
                Err(e) => anyhow::bail!("无法创建用户主目录 '{}': {}", home_dir, e),
            }
        }
    }

    /// 删除用户
    pub fn remove_user(&mut self, username: &str) -> Result<(), UserError> {
        if self.users.remove(username).is_none() {
            return Err(UserError::UserNotFound(username.to_string()));
        }
        tracing::info!("用户已删除：{}", username);
        Ok(())
    }

    /// 更新用户密码
    pub fn update_password(&mut self, username: &str, new_password: &str) -> Result<(), UserError> {
        let user = self
            .users
            .get_mut(username)
            .ok_or_else(|| UserError::UserNotFound(username.to_string()))?;

        user.password_hash = Self::hash_password(new_password)?;
        tracing::info!("用户密码已更新：{}", username);
        Ok(())
    }

    /// 更新用户主目录
    pub fn update_home_dir(&mut self, username: &str, home_dir: &str) -> Result<(), UserError> {
        let user = self
            .users
            .get_mut(username)
            .ok_or_else(|| UserError::UserNotFound(username.to_string()))?;

        let home_dir = home_dir.trim();
        if home_dir.is_empty() {
            return Err(UserError::InvalidHomeDirectory(
                "用户主目录不能为空".to_string(),
            ));
        }

        Self::validate_and_prepare_home_dir(home_dir)
            .map_err(|e| UserError::InvalidHomeDirectory(e.to_string()))?;

        user.home_dir = home_dir.to_string();
        tracing::info!("用户主目录已更新：{} -> {}", username, home_dir);
        Ok(())
    }

    /// 更新用户权限
    pub fn update_permissions(
        &mut self,
        username: &str,
        permissions: Permissions,
    ) -> Result<(), UserError> {
        let user = self
            .users
            .get_mut(username)
            .ok_or_else(|| UserError::UserNotFound(username.to_string()))?;

        user.permissions = permissions;
        tracing::info!("用户权限已更新：{}", username);
        Ok(())
    }

    /// 设置用户启用/禁用状态
    pub fn set_user_enabled(&mut self, username: &str, enabled: bool) -> Result<(), UserError> {
        let user = self
            .users
            .get_mut(username)
            .ok_or_else(|| UserError::UserNotFound(username.to_string()))?;

        user.enabled = enabled;
        tracing::info!("用户状态已更新：{} (enabled={})", username, enabled);
        Ok(())
    }

    /// 设置用户管理员状态
    pub fn set_user_admin(&mut self, username: &str, is_admin: bool) -> Result<(), UserError> {
        let user = self
            .users
            .get_mut(username)
            .ok_or_else(|| UserError::UserNotFound(username.to_string()))?;

        user.is_admin = is_admin;
        tracing::info!("用户管理员状态已更新：{} (is_admin={})", username, is_admin);
        Ok(())
    }

    /// 用户认证
    ///
    /// # Arguments
    /// * `username` - 用户名
    /// * `password` - 密码
    ///
    /// # Returns
    /// 认证成功返回 Ok(true)，失败返回 Ok(false) 或错误
    pub fn authenticate(&mut self, username: &str, password: &str) -> Result<bool, UserError> {
        let user = self
            .users
            .get_mut(username)
            .ok_or_else(|| UserError::UserNotFound(username.to_string()))?;

        if !user.enabled {
            tracing::warn!("用户已禁用：{}", username);
            return Ok(false);
        }

        if Self::verify_password(password, &user.password_hash) {
            user.last_login = Some(Utc::now());
            tracing::info!("用户认证成功：{}", username);
            return Ok(true);
        }

        tracing::warn!("用户认证失败：{}", username);
        Ok(false)
    }

    /// 从文件重新加载用户数据
    ///
    /// 如果文件不存在或解析失败，静默忽略错误
    pub fn reload(&mut self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return Ok(()),
        };

        if content.trim().is_empty() {
            return Ok(());
        }

        let manager: UserManager = match serde_json::from_str(&content) {
            Ok(m) => m,
            Err(_) => return Ok(()),
        };

        self.users = manager.users;
        tracing::info!("用户数据已从文件重新加载：{:?}", path);
        Ok(())
    }

    pub fn get_user(&self, username: &str) -> Option<&User> {
        self.users.get(username)
    }

    pub fn get_users(&self) -> &std::collections::HashMap<String, User> {
        &self.users
    }

    /// 返回所有用户的 Vec（clone），兼容旧代码
    pub fn get_all_users(&self) -> Vec<User> {
        self.users.values().cloned().collect()
    }

    /// 返回用户数量，无需 clone
    pub fn user_count(&self) -> usize {
        self.users.len()
    }

    /// 返回用户引用迭代器，避免不必要的 clone
    pub fn iter_users(&self) -> impl Iterator<Item = &User> {
        self.users.values()
    }

    /// 返回可变用户引用迭代器
    pub fn iter_users_mut(&mut self) -> impl Iterator<Item = &mut User> {
        self.users.values_mut()
    }
}
