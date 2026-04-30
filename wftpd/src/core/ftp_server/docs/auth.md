# auth.rs - 认证系统

## 功能概述

本文件实现了FTP服务器的认证系统，包括用户认证、会话跟踪、权限映射等功能，并与Fail2Ban集成实现防暴力破解保护。

## 设计原理

### 认证流程

```
客户端连接
    ↓
IP安全检查（白名单、连接限制）
    ↓
Fail2Ban检查（IP是否被封禁）
    ↓
连接数检查（是否超过限制）
    ↓
凭证验证（用户名/密码）
    ↓
成功 → 注册会话、重置Fail2Ban计数
失败 → 注销连接、增加Fail2Ban计数
```

### 安全设计

1. **时间攻击防护**: 对不存在的用户也执行密码哈希验证
2. **Fail2Ban集成**: 失败次数过多自动封禁IP
3. **连接限制**: 防止单IP占用过多连接
4. **IP白名单**: 可选的IP访问控制

## 核心结构

### SessionTracker - 会话跟踪器

```rust
#[derive(Debug, Default)]
pub struct SessionTracker {
    sessions: Mutex<HashMap<String, Vec<String>>>,      // 用户名 -> IP列表
    trace_to_ip: Mutex<HashMap<String, String>>,        // trace_id -> IP
    trace_to_username: Mutex<HashMap<String, String>>,  // trace_id -> 用户名
}
```

**功能**: 跟踪活跃的FTP会话

**方法**:
| 方法 | 功能 |
|------|------|
| `register(username, client_ip)` | 注册用户会话 |
| `get_ip_for_user(username)` | 获取用户最后登录IP |
| `register_trace(trace_id, username, client_ip)` | 注册trace映射 |
| `unregister(username)` | 注销用户会话 |
| `unregister_by_trace(trace_id)` | 通过trace_id注销 |

### WftpdUser - 用户详情

```rust
#[derive(Debug, Clone)]
pub struct WftpdUser {
    pub username: String,        // 用户名
    pub home_dir: PathBuf,       // 主目录
    pub enabled: bool,           // 是否启用
    pub permissions: Permissions, // 权限配置
}
```

**实现的trait**:
- `UserDetail`: libunftp用户详情接口
- `UserWithRoot`: 提供用户根目录
- `UserWithPermissions`: 提供VFS权限

### WftpdAuthenticator - 认证器

```rust
#[derive(Debug)]
pub struct WftpdAuthenticator {
    user_manager: Arc<Mutex<UserManager>>,     // 用户管理器
    users_path: PathBuf,                        // 用户文件路径
    fail2ban_manager: Option<Arc<Fail2BanManager>>, // Fail2Ban管理器
    dummy_hash: String,                         // 假哈希（防时间攻击）
    config: Option<Arc<Mutex<Config>>>,        // 全局配置
    session_tracker: Arc<SessionTracker>,       // 会话跟踪器
}
```

### WftpdUserDetailProvider - 用户详情提供者

```rust
#[derive(Debug)]
pub struct WftpdUserDetailProvider {
    user_manager: Arc<Mutex<UserManager>>,
    users_path: PathBuf,
}
```

**功能**: 认证成功后提供用户详细信息

## 权限映射

### Permissions → VfsOperations

| 用户权限 | VFS操作 | 说明 |
|----------|---------|------|
| `can_read` | `GET` | 下载文件 |
| `can_write` | `PUT` | 上传文件 |
| `can_delete` | `DEL` | 删除文件 |
| `can_list` | `LIST` | 列出目录 |
| `can_mkdir` | `MK_DIR` | 创建目录 |
| `can_rmdir` | `RM_DIR` | 删除目录 |
| `can_rename` | `RENAME` | 重命名 |
| `can_append` | `PUT` | 追加写入 |

### 权限映射实现

```rust
impl UserWithPermissions for WftpdUser {
    fn permissions(&self) -> VfsOperations {
        let mut ops = VfsOperations::empty();
        
        if self.permissions.can_read { ops |= VfsOperations::GET; }
        if self.permissions.can_write { ops |= VfsOperations::PUT; }
        // ... 其他权限
        
        ops
    }
}
```

## 认证实现

### Authenticator trait实现

```rust
#[async_trait]
impl Authenticator for WftpdAuthenticator {
    async fn authenticate(
        &self,
        username: &str,
        creds: &Credentials,
    ) -> Result<Principal, AuthenticationError>;
    
    fn name(&self) -> &str { "WftpdAuthenticator" }
}
```

### 认证详细流程

#### 1. 获取客户端IP
```rust
let client_ip = Self::get_client_ip(creds);
```

#### 2. IP安全检查
```rust
self.check_ip_security(&client_ip)?;
```
- 检查IP是否在白名单
- 检查是否超过连接限制

#### 3. Fail2Ban检查
```rust
if fail2ban_manager.is_banned(&client_ip).await {
    return Err(AuthenticationError::BadPassword);
}
```

#### 4. 连接数检查
```rust
if !self.try_register_connection(&client_ip) {
    return Err(AuthenticationError::BadPassword);
}
```

#### 5. 密码验证
```rust
let auth_result = {
    let mut users = self.user_manager.lock();
    reload_users_if_needed(&mut users, &self.users_path);
    
    match users.get_user(username) {
        Some(_) => (true, users.authenticate(username, password)),
        None => {
            // 对不存在的用户也执行哈希验证
            let _ = UserManager::verify_password(password, &self.dummy_hash);
            (false, Ok(false))
        }
    }
};
```

#### 6. 结果处理

**成功**:
```rust
self.session_tracker.register(username, client_ip.clone());
fail2ban_manager.reset_failures(&client_ip).await;
return Ok(Principal { username: username.to_string() });
```

**失败**:
```rust
self.unregister_connection(&client_ip);
fail2ban_manager.add_failure(&client_ip).await;
return Err(AuthenticationError::BadPassword);
```

## 安全特性

### 时间攻击防护

**原理**: 攻击者可能通过响应时间差异判断用户是否存在

**防护措施**:
1. 预生成一个固定的假哈希值
2. 对不存在的用户也执行完整的密码验证
3. 验证时间与正常验证相近

```rust
fn generate_dummy_hash() -> String {
    let params = Params::new(65536, 3, 4, Some(32)).unwrap();
    let argon2 = Argon2::new(argon2::Algorithm::Argon2id, argon2::Version::V0x13, params);
    argon2.hash_password(b"dummy_password_for_constant_time_verification")
        .map(|h| h.to_string())
        .unwrap_or_else(|_| DEFAULT_DUMMY_HASH.to_string())
}
```

### Fail2Ban集成

**工作原理**:
1. 每次认证失败，记录IP和失败时间
2. 失败次数达到阈值，封禁IP一段时间
3. 认证成功，清除该IP的失败记录

### 连接限制

**限制类型**:
- 单IP最大连接数
- 全局最大连接数
- 连接速率限制

## UserDetailProvider实现

```rust
#[async_trait]
impl UserDetailProvider for WftpdUserDetailProvider {
    type User = WftpdUser;
    
    async fn provide_user_detail(
        &self,
        principal: &Principal,
    ) -> Result<WftpdUser, UserDetailError>;
}
```

**功能**: 认证成功后，提供用户的完整信息

**返回内容**:
- 用户名
- 主目录路径
- 启用状态
- 权限配置

## 辅助函数

### reload_users_if_needed

```rust
fn reload_users_if_needed(users: &mut UserManager, users_path: &Path)
```

**功能**: 检查并重新加载用户配置文件

### check_ip_security

```rust
fn check_ip_security(&self, client_ip: &str) -> Result<(), AuthenticationError>
```

**功能**: 检查IP是否允许连接

### try_register_connection

```rust
fn try_register_connection(&self, client_ip: &str) -> bool
```

**功能**: 尝试注册连接，返回是否成功

### unregister_connection

```rust
fn unregister_connection(&self, client_ip: &str)
```

**功能**: 注销连接

## 日志记录

认证过程记录以下日志：

| 事件 | 级别 | 内容 |
|------|------|------|
| IP被封禁 | WARN | 拒绝被封禁IP的登录 |
| 连接限制 | WARN | 超过连接限制 |
| 登录成功 | INFO | 用户名、IP |
| 登录失败 | WARN | 用户名、IP |
| 权限映射 | WARN | 用户权限详情 |

## 使用示例

```rust
// 创建认证器
let authenticator = Arc::new(WftpdAuthenticator::new(
    user_manager,
    users_path,
    Some(fail2ban_manager),
    Some(config),
    session_tracker,
));

// 创建用户详情提供者
let user_detail_provider = Arc::new(WftpdUserDetailProvider::new(
    user_manager,
    users_path,
));

// 配置到服务器
let server = ServerBuilder::new(storage_factory)
    .authenticator(authenticator)
    .user_detail_provider(user_detail_provider)
    .build();
```
