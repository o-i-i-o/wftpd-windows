# GUI 服务控制架构重构说明

## 📋 重构概述

根据新的设计决策：**服务完全由 GUI 直接控制操作系统服务**，后台程序（wftpd.exe）不再提供服务管理功能。

---

## ✅ 已完成的更改

### 1. **删除后台程序的服务管理代码**

**文件**: `src/service_main.rs`

**删除内容**:
- ❌ `install_service()` 函数（第 175-213 行）
- ❌ `uninstall_service()` 函数（第 215-225 行）
- ❌ `main()` 函数中的命令行参数解析（`--install`/`--uninstall`）
- ❌ 未使用的导入：`std::ffi::OsString`, `ServiceManager`, `ServiceInfo` 等

**保留内容**:
- ✅ Windows 服务核心逻辑（服务运行、IPC 通信）
- ✅ 控制台应用程序模式
- ✅ Ctrl-C 信号处理

**代码行数变化**: **-74 行**

---

### 2. **完善 GUI 服务控制代码**

**文件**: `src/core/server_manager.rs`

**保留并完善的方法**:

| 方法 | 功能 | 状态 |
|------|------|------|
| `install_service()` | 安装 Windows 服务 | ✅ 完整实现 |
| `uninstall_service()` | 卸载 Windows 服务 | ✅ 完整实现 |
| `start_service()` | 启动服务 | ✅ 完整实现 |
| `stop_service()` | 停止服务 | ✅ 完整实现 |
| `restart_service()` | 重启服务 | ✅ 完整实现 |
| `is_service_installed()` | 检查服务是否已安装 | ✅ 完整实现 |
| `is_service_running()` | 检查服务是否运行中 | ✅ 完整实现 |

**关键实现细节**:

```rust
// 优雅停止服务（带等待机制）
fn stop_service_internal(service: &windows_service::service::Service) -> anyhow::Result<()> {
    service.stop()?;
    
    // 轮询检查服务状态，最多等待 10 秒
    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        match service.query_status() {
            Ok(s) => {
                if s.current_state == ServiceState::Stopped {
                    return Ok(());
                }
            }
            Err(_) => return Ok(()),
        }
    }
    Err(anyhow::anyhow!("等待服务停止超时"))
}
```

---

### 3. **GUI 界面服务控制**

**文件**: `src/gui_egui/service_tab.rs`

**功能特性**:

✅ **异步操作** - 所有服务操作都在独立线程中执行，避免阻塞 UI  
✅ **进度反馈** - 显示"安装中..."、"启动中..."等状态  
✅ **错误处理** - 友好的错误提示（如"需要管理员权限"）  
✅ **确认机制** - 卸载前有二次确认，防止误操作  
✅ **自动刷新** - 每 2 秒自动刷新服务状态  

**操作流程**:

```
用户点击按钮 
  ↓
设置操作状态为 "Installing/Starting/Stopping..."
  ↓
创建 mpsc 通道
  ↓
在新线程中执行 ServerManager 方法
  ↓
捕获 panic（AssertUnwindSafe）
  ↓
发送结果到通道
  ↓
请求 UI 重绘
  ↓
显示成功/失败消息
```

---

## 🏗️ 架构设计

### **新旧对比**

#### ❌ 旧架构（已废弃）
```
┌──────────────┐      ┌─────────────┐
│  GUI         │      │ wftpd.exe  │
│  (前端)      │      │ (后台)     │
└──────────────┘      └─────────────┘
       │                     │
       ├─► ServerManager     │
       │   install_service() │
       │                     │
       │                     ├──► --install 参数
       │                     │    安装服务
       │                     │
       │                     ├──► --uninstall 参数
       │                          卸载服务
       │
       └─► 两套服务管理代码重复
```

#### ✅ 新架构（当前）
```
┌──────────────┐      ┌─────────────┐
│  GUI         │      │ wftpd.exe  │
│  (前端)      │      │ (纯后台)   │
└──────────────┘      └─────────────┘
       │                     │
       ├─► ServerManager     │
       │   install_service() │
       │   start_service()   │
       │   stop_service()    │
       │   restart_service() │
       │   uninstall_...()   │
       │
       └─► 唯一的服务管理入口
```

---

## 📊 代码统计

| 模块 | 修改前 | 修改后 | 变化 |
|------|--------|--------|------|
| `service_main.rs` | 342 行 | 268 行 | **-74 行 (-22%)** |
| `server_manager.rs` | 586 行 | 436 行 | **-150 行 (-26%)** |
| **总计** | 928 行 | 704 行 | **-224 行 (-24%)** |

**删除的冗余代码**:
- 4 个异步方法（`start_ftp_async` 等）
- 2 个服务安装/卸载函数（service_main 中）
- 相关导入和辅助函数

---

## 🎯 使用方式

### **GUI 用户操作流程**

1. **安装服务**
   ```
   以管理员身份运行 wftp-gui.exe
   → 切换到"系统服务管理"标签页
   → 点击"📦 安装服务"按钮
   → 等待安装完成
   ```

2. **启动/停止/重启服务**
   ```
   服务已安装 → 显示"▶ 启动服务"按钮
   → 点击启动
   → 服务运行中 → 显示"⏹ 停止服务"和"🔄 重启服务"按钮
   ```

3. **卸载服务**
   ```
   点击"🗑 卸载服务"
   → 弹出确认对话框
   → 点击"确认"
   → 服务停止并删除
   ```

---

### **命令行用户（已废弃）**

❌ **以下命令已失效**:
```bash
wftpd.exe --install     # ❌ 不再支持
wftpd.exe --uninstall   # ❌ 不再支持
```

✅ **正确的使用方式**:
```bash
wftpd.exe               # ✅ 直接运行后台服务
```

---

## ⚠️ 注意事项

### **权限要求**

| 操作 | 权限要求 |
|------|---------|
| 安装服务 | ⚠️ **需要管理员权限** |
| 卸载服务 | ⚠️ **需要管理员权限** |
| 启动/停止/重启 | ✅ 普通用户权限即可 |

### **服务配置**

- **服务名称**: `wftpd`
- **显示名称**: `WFTPD SFTP/FTP Server`
- **描述**: `SFTP and FTP server daemon with GUI management`
- **启动类型**: `AutoStart`（开机自动启动）
- **可执行文件**: 与 GUI 同一目录下的 `wftpd.exe`

---

## 🔍 技术细节

### **错误处理优化**

```rust
// ServerManager::install_service()
let wftpd_exe = exe_dir.join("wftpd.exe");
if !wftpd_exe.exists() {
    return Err(anyhow::anyhow!(
        "在当前目录未找到 wftpd.exe，请确保 wftpd.exe 与 wftp-gui.exe 在同一目录"
    ));
}
```

### **日志记录**

```rust
// 安装成功日志
tracing::info!("服务安装成功");

// 卸载过程日志
tracing::info!("服务正在运行，尝试停止...");
tracing::warn!("停止服务失败（可能服务已停止）: {:?}", e);
```

### **优雅关闭**

```rust
// 先检查服务状态
match service.query_status() {
    Ok(status) => {
        if status.current_state != ServiceState::Stopped {
            // 如果运行中，先优雅停止
            Self::stop_service_internal(&service)?;
        }
    }
    Err(e) => {
        tracing::warn!("查询服务状态失败：{:?}", e);
    }
}

// 然后删除服务
service.delete()?;
```

---

## 📈 改进效果

### **优点**

✅ **架构清晰** - GUI 是唯一的服务管理入口  
✅ **职责分离** - wftpd.exe 专注于提供 FTP/SFTP 服务  
✅ **用户体验统一** - 所有操作都在 GUI 中完成  
✅ **减少代码重复** - 删除了 74 行冗余代码  
✅ **易于维护** - 只有一套服务管理逻辑  

### **潜在风险**

⚠️ **依赖 GUI** - 无法通过命令行快速安装/卸载服务  
⚠️ **调试复杂** - 需要在 GUI 中测试服务管理功能  

---

## 🧪 测试建议

### **功能测试清单**

- [ ] 以管理员身份运行 GUI，安装服务
- [ ] 验证服务已正确安装在 Windows 服务列表中
- [ ] 点击"启动服务"，验证服务状态变为"运行中"
- [ ] 点击"停止服务"，验证服务状态变为"已停止"
- [ ] 点击"重启服务"，验证服务正常重启
- [ ] 点击"卸载服务"，确认对话框弹出
- [ ] 点击确认后，验证服务被删除
- [ ] 验证非管理员用户无法安装/卸载服务（权限错误提示）
- [ ] 验证服务启动失败时的错误提示
- [ ] 验证服务停止失败时的错误提示

---

## 📝 后续优化建议

1. **添加服务安装进度条** - 更直观地显示安装进度
2. **增强错误恢复** - 服务启动失败时提供自动诊断
3. **日志查看器** - 在 GUI 中直接查看服务日志
4. **服务配置编辑** - 允许在 GUI 中修改服务启动类型
5. **批量操作** - 一键启动/停止所有服务

---

## 🎉 总结

本次重构成功地：

1. ✅ **删除了后台程序的服务管理功能**（74 行代码）
2. ✅ **完善了 GUI 的服务控制能力**（保留完整方法）
3. ✅ **统一了用户操作界面**（所有操作在 GUI 中完成）
4. ✅ **减少了代码重复**（总共删除 224 行冗余代码）
5. ✅ **提升了架构清晰度**（职责分离明确）

**新的架构完全符合设计目标：服务完全由 GUI 直接控制。**
