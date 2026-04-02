//! 配置管理模块
//! 
//! 提供统一的配置管理接口，支持配置变更通知机制
//! 
//! # 设计说明
//! 
//! - 使用 `RwLock` 保证读多写少场景下的高性能
//! - 配置变更通过事件监听器通知所有订阅者
//! - 避免在持有锁时调用外部代码，防止死锁

use std::sync::Arc;
use parking_lot::RwLock;
use crate::core::config::Config;
use crate::core::error::Result;

/// 配置变更事件类型
#[derive(Debug, Clone)]
pub enum ConfigChangeEvent {
    /// FTP 配置变更
    FtpChanged,
    /// SFTP 配置变更
    SftpChanged,
    /// 安全配置变更
    SecurityChanged,
    /// 日志配置变更
    LoggingChanged,
    /// 配置完全重载
    ConfigReloaded,
}

/// 配置变更监听器 trait
pub trait ConfigChangeListener: Send + Sync {
    /// 当配置变更时调用
    fn on_config_changed(&self, event: &ConfigChangeEvent);
}

/// 配置管理器
/// 
/// 统一管理配置访问和变更通知，解决 AppState 与 GUI 配置分离的问题
/// 
/// # 线程安全
/// 
/// 本结构体是线程安全的，可以在多个线程中共享使用
pub struct ConfigManager {
    config: Arc<RwLock<Config>>,
    listeners: RwLock<Vec<Box<dyn ConfigChangeListener>>>,
}

impl std::fmt::Debug for ConfigManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConfigManager")
            .field("config", &self.config.read())
            .finish()
    }
}

impl ConfigManager {
    /// 创建新的配置管理器
    pub fn new(config: Config) -> Self {
        ConfigManager {
            config: Arc::new(RwLock::new(config)),
            listeners: RwLock::new(Vec::new()),
        }
    }

    /// 从文件加载配置并创建管理器
    pub fn load(path: &std::path::Path) -> Result<Self> {
        let config = Config::load(path)?;
        Ok(Self::new(config))
    }

    /// 添加配置变更监听器
    /// 
    /// # Arguments
    /// * `listener` - 实现 ConfigChangeListener trait 的监听器
    pub fn add_listener(&self, listener: Box<dyn ConfigChangeListener>) {
        self.listeners.write().push(listener);
    }

    /// 移除所有监听器
    pub fn clear_listeners(&self) {
        self.listeners.write().clear();
    }

    /// 获取配置读锁（只读访问）
    /// 
    /// # 注意
    /// 
    /// 读锁允许多个读取者同时访问，但会阻塞写入操作
    pub fn read(&self) -> parking_lot::RwLockReadGuard<'_, Config> {
        self.config.read()
    }

    /// 获取配置写锁（不触发变更通知）
    /// 
    /// # 注意
    /// 
    /// 写锁是独占的，会阻塞所有其他读写操作
    /// 如果需要触发变更通知，请使用 modify 方法
    pub fn write(&self) -> parking_lot::RwLockWriteGuard<'_, Config> {
        self.config.write()
    }

    /// 修改配置并触发变更通知
    /// 
    /// # 设计说明
    /// 
    /// 此方法确保在释放写锁后才触发变更通知，避免死锁
    /// 
    /// # Arguments
    /// * `f` - 修改配置的闭包函数
    /// 
    /// # Returns
    /// 返回闭包函数的执行结果
    pub fn modify<F, T>(&self, f: F) -> T
    where
        F: FnOnce(&mut Config) -> T,
    {
        let mut config = self.config.write();
        let result = f(&mut config);
        drop(config); // 先释放写锁，再触发通知，避免死锁
        
        // 触发变更通知（简化处理，通知所有变更）
        self.notify_listeners(&ConfigChangeEvent::ConfigReloaded);
        
        result
    }

    /// 保存配置到文件
    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        let config = self.config.read();
        config.save(path)
            .map_err(crate::core::error::AppError::Config)
    }

    /// 从文件重新加载配置
    /// 
    /// # Arguments
    /// * `path` - 配置文件路径
    /// 
    /// # Returns
    /// 成功返回 Ok(())，失败返回错误
    pub fn reload_from_file(&self, path: &std::path::Path) -> Result<()> {
        let new_config = Config::load(path)
            .map_err(crate::core::error::AppError::Config)?;
        let mut config = self.config.write();
        *config = new_config;
        drop(config);
        
        self.notify_listeners(&ConfigChangeEvent::ConfigReloaded);
        Ok(())
    }

    /// 通知所有监听器配置已变更
    fn notify_listeners(&self, event: &ConfigChangeEvent) {
        let listeners = self.listeners.read();
        for listener in listeners.iter() {
            listener.on_config_changed(event);
        }
    }
}

impl Clone for ConfigManager {
    fn clone(&self) -> Self {
        ConfigManager {
            config: Arc::clone(&self.config),
            listeners: RwLock::new(Vec::new()), // 不克隆监听器，避免重复通知
        }
    }
}

/// 简单的配置变更监听器实现
pub struct SimpleConfigListener<F>
where
    F: Fn(&ConfigChangeEvent) + Send + Sync,
{
    callback: F,
}

impl<F> SimpleConfigListener<F>
where
    F: Fn(&ConfigChangeEvent) + Send + Sync,
{
    pub fn new(callback: F) -> Self {
        SimpleConfigListener { callback }
    }
}

impl<F> ConfigChangeListener for SimpleConfigListener<F>
where
    F: Fn(&ConfigChangeEvent) + Send + Sync,
{
    fn on_config_changed(&self, event: &ConfigChangeEvent) {
        (self.callback)(event);
    }
}
