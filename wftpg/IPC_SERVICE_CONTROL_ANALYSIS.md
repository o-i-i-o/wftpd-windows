# IPC 通信与服务控制架构详解

## 概述

本文档详细分析了 wftpg 项目中 IPC（进程间通信）机制的设计，以及 GUI 如何控制 Windows 服务（wftpd）。

---

## 核心结论

### ✅ **IPC 中不需要服务控制命令传递**

**原因**：
1. **Windows 服务管理是直接的 API 调用**
   - GUI 通过 `ServerManager` 直接调用 Windows SCM API
   - 不需要经过 IPC 传递给后端服务
   
2. **服务控制与业务逻辑分离**
   - 服务安装/卸载/启动/停止 = **操作系统级别操作**
   - 配置重载 = **应用级别操作**（需要 IPC）

3. **清晰的职责边界**
   ```
   GUI (wftpg.exe) ──► Windows SCM ──► Service (wftpd.exe)
                      (服务控制)
   
   GUI (wftpg.exe) ──► IPC ──► Service (wftpd.exe)
                      (配置重载)
   ```

---

## 架构设计

### 双通道控制模型

```
┌──────────────────┐
│  wftpg.exe       │
│  (前端 GUI)      │
├──────────────────┤
│                  │
│  ┌────────────┐  │
│  │ ServiceTab │  │
│  │            │  │
│  │ install()  │──┼─────┐
│  │ start()    │──┼─────┐
│  │ stop()     │──┼─────┤
│  └────────────┘  │     │
│                  │     │
│  ┌────────────┐  │     │
│  │ ServerTab  │  │     │
│  │ SecurityTab│  │     │
│  │            │  │     │
│  │ save()     │──┼─────┼──────┐
│  └────────────┘  │     │      │
│                  │     │      │
└──────────────────┘     │      │
                         │      │
        ┌────────────────┘      │
        │                       │
        ▼                       ▼
┌──────────────┐       ┌──────────────┐
│ Windows SCM  │       │ Named Pipe   │
│ (服务控制)    │       │ (IPC 通信)    │
└───────┬──────┘       └───────┬──────┘
        │                       │
        │ dwCurrentState        │ ReloadCommand
        │ SERVICE_RUNNING       │ action="reload"
        │                       │
        ▼                       ▼
┌──────────────────────────────────────┐
│         wftpd.exe (服务进程)          │
├──────────────────────────────────────┤
│                                      │
│  ┌────────────┐    ┌────────────┐   │
│  │ SCM Handler│    │ IPC Server │   │
│  │            │    │            │   │
│  │ OnStart()  │    │ accept()   │   │
│  │ OnStop()   │    │ receive()  │   │
│  │            │    │            │   │
│  └────────────┘    └─────┬──────┘   │
│                          │           │
│                          ▼           │
│                   ┌────────────┐    │
│                   │ handle_    │    │
│                   │ command()  │    │
│                   │            │    │
│                   │ reload     │    │
│                   │ config     │    │
│                   └────────────┘    │
│                                      │
└──────────────────────────────────────┘
```

---

## 一、Windows 服务控制通道（直接调用）

### 1.1 GUI 端实现

**文件**: `src/core/server_manager.rs`

#### 核心方法

```rust
pub struct ServerManager;

impl ServerManager {
    /// 检查服务是否已安装
    pub fn is_service_installed(&self) -> bool {
        unsafe {
            let manager = OpenSCManagerW(None, None, SC_MANAGER_CONNECT);
            // ...
        }
    }
    
    /// 检查服务是否正在运行
    pub fn is_service_running(&self) -> bool {
        unsafe {
            let status = QueryServiceStatusEx(...);
            return status.dwCurrentState == SERVICE_RUNNING;
        }
    }
    
    /// 安装服务
    pub fn install_service(&self) -> Result<()> {
        unsafe {
            let service = CreateServiceW(
                manager,
                PCWSTR(service_name),
                PCWSTR(display_name),
                SERVICE_CHANGE_CONFIG | SERVICE_START,
                SERVICE_WIN32_OWN_PROCESS,
                SERVICE_AUTO_START,
                SERVICE_ERROR_NORMAL,
                PCWSTR(exe_path),
                // ...
            );
        }
    }
    
    /// 启动服务
    pub fn start_service(&self) -> Result<()> {
        unsafe {
            StartServiceW(service, None);
        }
    }
    
    /// 停止服务
    pub fn stop_service(&self) -> Result<()> {
        unsafe {
            ControlService(service, SERVICE_CONTROL_STOP, &mut status);
        }
    }
}
```

#### GUI 集成

**文件**: `src/gui_egui/service_tab.rs`

```rust
pub struct ServiceTab {
    manager: ServerManager,
    is_installed: bool,
    is_running: bool,
    operation_state: OperationState,
    // ...
}

impl ServiceTab {
    /// 异步安装服务
    fn install_service_async(&mut self, ctx: &egui::Context) {
        std::thread::spawn(move || {
            let manager = ServerManager::new();
            let result = manager.install_service();
            // 发送结果到主线程...
        });
    }
    
    /// 异步启动服务
    fn start_service_async(&mut self, ctx: &egui::Context) {
        std::thread::spawn(move || {
            let manager = ServerManager::new();
            let result = manager.start_service();
            // 发送结果到主线程...
        });
    }
}
```

### 1.2 服务端响应

**文件**: `wftpd/src/service_main.rs`

#### Windows 服务入口点

```rust
pub fn run_as_service() -> windows_service::Result<()> {
    define_windows_service!(ffi_service_main, service_main);
    
    fn service_main(arguments: Vec<OSString>) -> Result<()> {
        let handler = MyServiceHandler;
        let status_handle = service_dispatcher::start(SERVICE_NAME, ffi_service_main)?;
        
        // 注册服务控制处理器
        let event_handler = move |control_event| -> ServiceControlHandlerResult {
            match control_event {
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                ServiceControl::Stop => {
                    // 处理停止请求
                    status_handle.set_service_status(ServiceStatus {
                        current_state: ServiceState::Stopped,
                        ..Default::default()
                    });
                    ServiceControlHandlerResult::NoError
                }
                ServiceControl::ParamChange(_) => {
                    // 处理参数变更
                    ServiceControlHandlerResult::NoError
                }
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        };
        
        status_handler.execute(event_handler);
        Ok(())
    }
}
```

#### 关键要点

1. **服务控制通过 SCM（Service Control Manager）**
   - GUI 调用 Windows API → SCM → 服务进程
   - 不经过命名管道/IPC

2. **服务状态由 Windows 管理**
   - `SERVICE_RUNNING` - 运行中
   - `SERVICE_STOPPED` - 已停止
   - `SERVICE_START_PENDING` - 启动中
   - `SERVICE_STOP_PENDING` - 停止中

3. **服务生命周期回调**
   - `OnStart()` - 服务启动时调用
   - `OnStop()` - 服务停止时调用
   - `OnPause()` / `OnContinue()` - 暂停/继续

---

## 二、配置重载通道（IPC 通信）

### 2.1 协议设计

**文件**: `src/core/ipc.rs`

#### 消息结构

```rust
/// 重载命令
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadCommand {
    pub action: String,  // "reload"
}

impl ReloadCommand {
    pub fn reload() -> Self {
        ReloadCommand {
            action: "reload".to_string(),
        }
    }
}

/// 重载响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReloadResponse {
    pub success: bool,
    pub message: String,
}

impl ReloadResponse {
    pub fn ok() -> Self {
        ReloadResponse {
            success: true,
            message: "配置已重新加载".to_string(),
        }
    }
    
    pub fn error(msg: &str) -> Self {
        ReloadResponse {
            success: false,
            message: msg.to_string(), 
        }
    }
}
```

#### 消息格式

```
┌─────────────────┬─────────────────┐
│  Length (4B)    │   JSON Data     │
│  (big-endian)   │   (UTF-8)       │
└─────────────────┴─────────────────┘

示例：
00 00 00 2A {"action":"reload"}
```

---

### 2.2 GUI 客户端实现

**文件**: `src/core/ipc.rs`

#### 客户端 API

```rust
pub struct IpcClient;

impl IpcClient {
    /// 内部方法：发送命令并接收响应（带超时）
    fn send_command_internal(cmd: ReloadCommand) -> Result<ReloadResponse> {
        let stream = IpcStream::connect()?;
        
        // 设置超时
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        
        // 序列化并发送
        let json = serde_json::to_vec(&cmd)?;
        write_message(&mut writer, &json)?;
        
        // 读取响应
        let buffer = read_message(&mut reader)?;
        let response: ReloadResponse = serde_json::from_slice(&buffer)?;
        
        Ok(response)
    }
    
    /// 通知后端重新加载配置
    pub fn notify_reload() -> Result<ReloadResponse> {
        Self::send_command_internal(ReloadCommand::reload())
    }
    
    /// 检查后端服务是否运行
    pub fn is_server_running() -> bool {
        IpcStream::connect().is_ok()
    }
}
```

#### GUI 集成

**文件**: `src/gui_egui/server_tab.rs`

```rust
pub fn save_config_async(&mut self, ctx: &egui::Context, config: Config) {
    // ... 验证配置 ...
    
    let config_manager = self.config_manager.clone();
    std::thread::spawn(move || {
        let result = match config_manager.save(&Config::get_config_path()) {
            Ok(_) => {
                tracing::info!("服务器配置保存成功");
                
                // 检查后端服务是否运行
                if IpcClient::is_server_running() {
                    // 通过 IPC 通知后端重载配置
                    match IpcClient::notify_reload() {
                        Ok(response) => {
                            if response.success {
                                Ok("配置已保存，后端服务已重新加载配置".to_string())
                            } else {
                                Ok(format!("配置已保存，但后端重新加载失败：{}", response.message))
                            }
                        }
                        Err(e) => {
                            Ok(format!("配置已保存，但通知后端失败：{}。请手动重启服务。", e))
                        }
                    }
                } else {
                    Ok("配置已保存（后端服务未运行）".to_string())
                }
            }
            Err(e) => Err(format!("保存失败：{}", e))
        };
        
        // 发送结果到 GUI 线程...
    });
}
```

---

### 2.3 服务端实现

**文件**: `wftpd/src/core/ipc.rs` + `wftpd/src/service_main.rs`

#### IPC 服务器

```rust
pub struct IpcServer {
    inner: IpcServerInner,  // Windows 命名管道服务器
}

impl IpcServer {
    pub fn new() -> Result<Self> {
        Ok(IpcServer {
            inner: IpcServerInner::new()?,
        })
    }
    
    /// 接受客户端连接
    pub fn accept(&self) -> Result<IpcConnection> {
        let stream = self.inner.accept()?;
        Ok(IpcConnection::new(stream))
    }
}
```

#### 连接处理器

```rust
pub struct IpcConnection {
    stream: IpcStream,
}

impl IpcConnection {
    /// 接收命令（带超时）
    pub fn receive_command(&mut self) -> Result<ReloadCommand> {
        self.stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        
        let mut reader = BufReader::new(&self.stream);
        let buffer = read_message(&mut reader)?;
        
        let command: ReloadCommand = serde_json::from_slice(&buffer)?;
        Ok(command)
    }
    
    /// 发送响应（带超时）
    pub fn send_response(&mut self, response: &ReloadResponse) -> Result<()> {
        self.stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        
        let json = serde_json::to_vec(response)?;
        let mut writer = BufWriter::new(&self.stream);
        write_message(&mut writer, &json)?;
        
        Ok(())
    }
}
```

#### 命令处理

**文件**: `wftpd/src/service_main.rs`

```rust
fn run_main_loop(state: &Arc<AppState>, ipc_server: &IpcServer) {
    loop {
        match ipc_server.accept() {
            Ok(mut connection) => {
                let state_clone = Arc::clone(state);
                thread::spawn(move || {
                    // 接收命令
                    if let Ok(cmd) = connection.receive_command() {
                        // 处理命令
                        let response = handle_command(&state_clone, &cmd);
                        
                        // 发送响应
                        if let Err(e) = connection.send_response(&response) {
                            tracing::error!("发送 IPC 响应失败：{e}");
                        }
                    } else {
                        tracing::warn!("接收 IPC 命令失败");
                    }
                });
            }
            Err(e) => {
                tracing::error!("接受 IPC 连接失败：{e}");
            }
        }
    }
}

fn handle_command(state: &AppState, cmd: &ReloadCommand) -> ReloadResponse {
    match cmd.action.as_str() {
        "reload" => {
            // 重新加载配置
            match state.reload_config() {
                Ok(_) => ReloadResponse::ok(),
                Err(e) => ReloadResponse::error(&format!("重载失败：{}", e)),
            }
        }
        _ => ReloadResponse::error("未知命令"),
    }
}
```

---

## 三、对比总结

### 3.1 两种控制通道对比

| 特性 | Windows 服务控制 | IPC 配置重载 |
|------|-----------------|-------------|
| **用途** | 安装/卸载/启动/停止服务 | 通知配置变更 |
| **实现方式** | Windows SCM API | 命名管道 (Named Pipe) |
| **调用层级** | 操作系统级别 | 应用级别 |
| **是否需要 IPC** | ❌ 否 | ✅ 是 |
| **典型场景** | • 安装服务<br>• 启动服务<br>• 停止服务<br>• 查询状态 | • 保存配置后通知<br>• 动态重载配置<br>• 检查服务是否运行 |
| **权限要求** | 管理员权限 | 普通用户权限 |
| **阻塞时间** | 较长（秒级） | 较短（毫秒级） |
| **错误处理** | Windows 错误码 | anyhow::Result |

---

### 3.2 完整控制流程

#### 场景 1: 用户点击"安装服务"按钮

```
GUI (ServiceTab)
    │
    ├─► ServerManager::install_service()
    │     │
    │     └─► Windows API: CreateServiceW()
    │           │
    │           └─► Windows SCM
    │                 │
    │                 └─► 注册表 (HKLM\SYSTEM\CurrentControlSet\Services\wftpd)
    │
    └─► 返回结果："服务安装成功，开机将自动启动"
```

**不涉及 IPC** ✅

---

#### 场景 2: 用户修改 FTP 端口并保存

```
GUI (ServerTab)
    │
    ├─► ConfigManager::save()
    │     │
    │     └─► 写入 config.toml
    │
    ├─► IpcClient::is_server_running()
    │     │
    │     └─► 尝试连接命名管道
    │           │
    │           ├─ OK → 服务运行中
    │           └─ Err → 服务未运行
    │
    ├─► (如果服务运行) IpcClient::notify_reload()
    │     │
    │     ├─► 序列化：{"action":"reload"}
    │     │
    │     ├─► 通过命名管道发送
    │     │
    │     └─► 等待响应
    │           │
    │           ├─ OK → {"success":true,"message":"配置已重新加载"}
    │           └─ Err → 连接断开/超时
    │
    └─► 显示结果给用户
```

**使用 IPC** ✅

---

#### 场景 3: 用户点击"启动服务"按钮

```
GUI (ServiceTab)
    │
    ├─► ServerManager::start_service()
    │     │
    │     └─► Windows API: StartServiceW()
    │           │
    │           └─► Windows SCM
    │                 │
    │                 └─► wftpd.exe (服务进程)
    │                       │
    │                       ├─► service_main() 入口
    │                       │
    │                       ├─► 初始化日志系统
    │                       │
    │                       ├─► 加载配置文件
    │                       │
    │                       ├─► 创建 IPC 服务器（命名管道）
    │                       │
    │                       └─► 启动 FTP/SFTP 监听器
    │
    └─► 返回结果："服务已启动"
```

**不涉及 IPC** ✅

---

### 3.3 为什么服务控制不需要 IPC？

1. **Windows 服务架构决定**
   ```
   应用程序 ──► SCM ──► 服务进程
               ↑
          (统一管理)
   ```
   - 所有服务控制都通过 SCM
   - 应用程序不能直接控制其他服务进程

2. **权限隔离**
   - 服务运行在 `LocalSystem` 或特定账户
   - GUI 运行在用户会话
   - SCM 作为中介进行权限检查

3. **生命周期管理**
   - 服务启动前，进程不存在 → 无法建立 IPC
   - 服务停止后，进程结束 → IPC 断开
   - 只有运行时才能建立 IPC 连接

4. **设计原则**
   - **服务控制** = 操作系统职责
   - **配置重载** = 应用内部职责
   - 职责分离，互不干扰

---

## 四、代码统计

### 4.1 服务控制相关代码

| 文件 | 行数 | 功能 |
|------|------|------|
| `src/core/server_manager.rs` | 219 | Windows 服务管理 |
| `src/gui_egui/service_tab.rs` | 409 | 服务控制 UI |
| **总计** | **628** | **服务控制** |

### 4.2 IPC 通信相关代码

| 文件 | 行数 | 功能 |
|------|------|------|
| `src/core/ipc.rs` | 188 | IPC 协议层 |
| `src/core/windows_ipc.rs` | 342 | Windows 命名管道 |
| **总计** | **530** | **IPC 通信** |

### 4.3 后端对应代码

**wftpd 项目**:

| 文件 | 行数 | 功能 |
|------|------|------|
| `wftpd/src/service_main.rs` | 285 | 服务入口 + 命令处理 |
| `wftpd/src/core/ipc.rs` | ~120 | IPC 服务器端 |
| **总计** | **~405** | **后端响应** |

---

## 五、最佳实践

### 5.1 服务控制建议

✅ **推荐做法**:
```rust
// 1. 使用 ServerManager 封装 Windows API
let manager = ServerManager::new();
manager.install_service()?;

// 2. 异步执行长时间操作
std::thread::spawn(move || {
    let result = manager.start_service();
    // 发送到 GUI 线程...
});

// 3. 提供清晰的错误提示
if let Err(e) = result {
    format!("启动失败：{}（需要管理员权限）", e)
}
```

❌ **不推荐做法**:
```rust
// ❌ 试图通过 IPC 控制服务
// IPC 只用于配置重载，不用于服务控制

// ❌ 在主线程同步调用
manager.install_service();  // 可能阻塞数秒
```

---

### 5.2 IPC 通信建议

✅ **推荐做法**:
```rust
// 1. 先检查服务是否运行
if IpcClient::is_server_running() {
    // 2. 发送配置重载通知
    match IpcClient::notify_reload() {
        Ok(response) => { /* 处理响应 */ }
        Err(e) => { /* 处理错误 */ }
    }
}

// 3. 设置合理的超时
stream.set_read_timeout(Some(Duration::from_secs(10)))?;
```

❌ **不推荐做法**:
```rust
// ❌ 无超时限制
// 可能导致永久阻塞

// ❌ 忽略错误处理
IpcClient::notify_reload().unwrap();  // panic!

// ❌ 在服务未运行时尝试通信
// 应先检查 is_server_running()
```

---

## 六、故障排查

### 6.1 常见问题

#### Q1: 服务安装失败
```
错误：无法创建服务
原因：没有管理员权限
解决：以管理员身份运行 wftpg.exe
```

#### Q2: 服务启动失败
```
错误：服务无法启动
原因：端口被占用/配置文件错误
解决：检查日志文件 C:\ProgramData\wftpg\logs\
```

#### Q3: IPC 通信失败
```
错误：连接 IPC 服务器失败
原因：服务未运行/命名管道不存在
解决：先启动服务，再保存配置
```

#### Q4: 配置重载无效
```
现象：保存配置后服务未生效
原因：IPC 通知失败，服务未收到重载命令
解决：手动重启服务
```

---

### 6.2 调试技巧

#### 检查服务状态
```powershell
# PowerShell
Get-Service wftpd

# 输出示例：
# Name                Status
# ----                ------
# wftpd               Running
```

#### 查看服务日志
```powershell
# 实时查看日志
Get-Content "C:\ProgramData\wftpg\logs\wftpd.log" -Wait -Tail 50
```

#### 测试 IPC 连接
```rust
// Rust 代码
if IpcClient::is_server_running() {
    println!("✓ IPC 服务器运行中");
} else {
    println!("✗ IPC 服务器未运行");
}
```

---

## 七、总结

### 核心要点

1. ✅ **服务控制不经过 IPC**
   - 通过 Windows SCM API 直接管理
   - 安装/启动/停止都是操作系统行为

2. ✅ **配置重载使用 IPC**
   - 通过命名管道发送重载命令
   - 应用级别的通信机制

3. ✅ **清晰的职责分离**
   - Windows SCM = 服务生命周期管理
   - IPC = 应用配置管理

4. ✅ **双通道设计合理**
   - 各司其职，互不干扰
   - 符合 Windows 服务架构规范

---

**文档版本**: v1.0  
**更新日期**: 2026-04-02  
**适用版本**: wftpg v3.2.12+
