# 日志自动刷新功能修复

## 问题描述
前端日志页面没有根据日志文件变化自动刷新，需要手动点击刷新按钮才能看到新日志。

## 根本原因
虽然代码中已经实现了基于 `notify` crate 的文件监听功能，但是在检测到文件变化后，没有通知 egui 框架进行 UI 重绘，导致即使数据已经更新，界面也没有反映出来。

## 修复方案

### 1. 修改 `check_log_events` 方法
**文件**: `src/gui_egui/log_tab.rs`

在检测到日志文件变化时，调用 `ctx.request_repaint()` 主动请求 UI 重绘：

```rust
pub fn check_log_events(&mut self, ctx: &egui::Context) {
    // ... 现有的事件处理逻辑 ...
    
    if self.last_event_time.is_none_or(|t| t.elapsed() >= Duration::from_millis(100)) {
        self.needs_refresh = true;
        self.last_event_time = Some(now);
        tracing::debug!("Log file changed: {:?}, will refresh", path);
        // 新增：请求 UI 重绘，确保日志能够立即显示
        ctx.request_repaint();
    }
}
```

### 2. 在 UI 渲染时传入 Context
**文件**: `src/gui_egui/log_tab.rs`

在 `ui` 方法中获取 `egui::Context` 并传递给 `check_log_events`：

```rust
pub fn ui(&mut self, ui: &mut egui::Ui) {
    styles::page_header(ui, "📋", "系统日志");

    // 获取 context 用于请求重绘
    let ctx = ui.ctx().clone();
    self.check_log_events(&ctx);

    // 如果有新日志触发，则加载（防抖动已处理）
    if self.needs_refresh && !self.loading {
        self.incrementally_read_logs();
        self.needs_refresh = false;
    }
    
    // ... 其余 UI 渲染逻辑 ...
}
```

## 技术细节

### 事件驱动架构
1. **文件监听**: 使用 `notify::RecommendedWatcher` 监听日志目录的变化
2. **防抖处理**: 设置 100ms 的防抖间隔，避免短时间内重复触发
3. **增量读取**: 只读取新增的日志内容，避免全量重新加载
4. **UI 通知**: 通过 `egui::Context::request_repaint()` 通知框架重绘

### 性能优化
- **轮询间隔**: 500ms（通过 `notify::Config::with_poll_interval` 设置）
- **防抖时间**: 100ms（减少频繁刷新）
- **最大显示条数**: 500 条（避免内存占用过大）
- **增量读取**: 每次最多读取 20 条新日志

## 测试方法

### 自动化测试脚本
运行 PowerShell 测试脚本：
```powershell
cd c:\Users\oi-io\Documents\wftpg-egui-20260328\wftpg
.\test_log_refresh.ps1
```

### 手动测试步骤
1. 启动 WFTPG GUI 程序
2. 切换到【运行日志】标签页
3. 使用文本编辑器打开日志文件（通常在 `C:\ProgramData\wftpg\logs\wftpg-yyyy-MM-dd.log`）
4. 追加新的日志行（JSON 格式）
5. 保存文件
6. 观察 GUI 是否在 1 秒内自动刷新显示新日志

### 预期结果
- ✅ 新日志自动出现在列表中（无需手动刷新）
- ✅ 日志级别颜色正确（INFO=绿色，WARN=橙色，ERROR=红色）
- ✅ 时间、协议、客户端 IP 等信息正确显示
- ✅ 如果用户不在底部，会显示"X 条新日志"提示按钮

## 相关文件
- `src/gui_egui/log_tab.rs` - 日志标签页实现
- `src/core/logger.rs` - 日志系统核心实现
- `Cargo.toml` - 依赖配置（包含 `notify = "8.2.0"`）
- `test_log_refresh.ps1` - 自动化测试脚本

## 注意事项
1. 确保日志目录存在（`C:\ProgramData\wftpg\logs`）
2. 日志文件必须是 JSON 格式
3. 文件监听器仅在日志目录存在时才会激活
4. 如果日志文件被删除或轮转，会自动重新初始化

## 版本信息
- 修复日期：2026-04-01
- 涉及版本：v3.2.11
- 依赖版本：notify 8.2.0, egui 0.34.0
