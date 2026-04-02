# WFTPG 窗口置顶功能测试指南

## 🎯 测试目标

验证程序启动时窗口自动置顶，并在用户交互后自动降级为普通窗口。

---

## 📋 测试步骤

### 步骤 1: 准备环境

```powershell
# 打开多个应用程序窗口
notepad          # 记事本
calc             # 计算器
msedge           # Edge 浏览器
```

### 步骤 2: 启动 WFTPG

```powershell
cd c:\Users\oi-io\Documents\wftpg-egui-20260328\wftpg
.\target\release\wftpg.exe
```

### 步骤 3: 观察置顶效果

**预期行为**:
1. ✅ WFTPG 窗口弹出并显示在**所有其他窗口之上**
2. ✅ 即使您点击了记事本或浏览器，WFTPG 仍然在最上层
3. ✅ 任务栏中 WFTPG 正常显示（没有特殊标记）

**截图对比**:
```
启动前:
┌─────────────┐
│  记事本     │ ← 最上层
└─────────────┘

启动 WFTPG 后 (0-5 秒):
┌─────────────┐
│  WFTPG      │ ← 自动置顶（最上层）✅
├─────────────┤
│  记事本     │
└─────────────┘
```

### 步骤 4: 触发降级

**执行任意一个操作**:
- 点击"服务器配置"按钮
- 切换 Tab（如从"服务器"到"用户管理"）
- 在搜索框输入文字
- 滚动日志列表

**预期行为**:
1. ✅ 点击瞬间窗口降级为普通窗口
2. ✅ 现在可以正常被其他窗口覆盖
3. ✅ 查看日志确认降级消息

**截图对比**:
```
用户交互后 (>5 秒):
┌─────────────┐
│  记事本     │ ← 可以覆盖 WFTPG ✅
├─────────────┤
│  WFTPG      │
└─────────────┘
```

---

## 🔍 验证方法

### 方法 1: 日志验证

查看程序输出的日志：

```powershell
# 实时查看日志
Get-Content "C:\ProgramData\wftpg\logs\wftpg-*.log" -Wait -Tail 20
```

**期望输出**:
```log
[INFO] 应用初始化完成，配置监听器已启动，窗口已置顶
[DEBUG] 窗口已降级为普通窗口（用户交互后）
```

### 方法 2: PowerShell 验证

```powershell
# 获取 WFTPG 窗口信息
$window = Get-Process wftpg | ForEach-Object {
    Add-Type -AssemblyName System.Windows.Forms
    [System.Windows.Forms.Form]::FromHandle($_.MainWindowHandle)
} | Where-Object { $_ -ne $null }

# 检查窗口属性（需要额外工具）
# 推荐使用 AutoHotkey 的 Window Spy
```

### 方法 3: 手动重叠测试

1. 启动 WFTPG，看到置顶效果
2. 打开记事本
3. 尝试将记事本拖到 WFTPG 上面
4. **启动初期**: 记事本无法覆盖 WFTPG ❌
5. **交互后**: 记事本可以覆盖 WFTPG ✅

---

## ⏱️ 时间线测试

| 时间点 | 操作 | 预期状态 | 通过/失败 |
|--------|------|---------|----------|
| T+0s   | 启动程序 | 窗口弹出 | □ |
| T+1s   | 观察层级 | 在所有窗口之上 | □ |
| T+2s   | 点击其他窗口 | WFTPG 仍在上层 | □ |
| T+3s   | 点击 WFTPG 按钮 | 触发降级 | □ |
| T+4s   | 打开记事本 | 可以覆盖 WFTPG | □ |
| T+5s   | 查看日志 | 有降级记录 | □ |

---

## 🐛 异常情况处理

### 情况 1: 窗口从未置顶

**可能原因**:
- Windows 全屏优化冲突
- 有其他更高优先级的置顶窗口
- egui 版本不支持

**解决方法**:
```rust
// 尝试强制刷新
ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
std::thread::sleep(std::time::Duration::from_millis(100));
ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
    egui::WindowLevel::AlwaysOnTop
));
```

### 情况 2: 窗口永远不降级

**可能原因**:
- 输入事件检测逻辑问题
- `pending_unset_topmost` 未正确设置

**调试代码**:
```rust
// 添加详细日志
if self.pending_unset_topmost {
    let events_count = ctx.input(|i| i.events.len());
    tracing::debug!("等待降级... 事件数={}", events_count);
    
    if events_count > 0 {
        tracing::info!("检测到 {} 个事件，执行降级", events_count);
        // ...
    }
}
```

### 情况 3: 降级时机不合适

**调整检测条件**:

```rust
// 方案 A: 只检测点击事件
let should_unset = ctx.input(|i| i.pointer.any_click());

// 方案 B: 检测点击或按键
let should_unset = ctx.input(|i| {
    i.pointer.any_click() || 
    i.events.iter().any(|e| matches!(e, Event::Key { pressed: true, .. }))
});

// 方案 C: 延迟 3 秒后自动降级 + 交互检测
if self.init_start_time.elapsed() > Duration::from_secs(3) {
    self.unset_topmost(ctx);
}
```

---

## 📊 性能影响测试

### 内存占用

```powershell
# 启动时
Get-Process wftpg | Select-Object WorkingSet, CPU

# 预期: 置顶功能几乎不增加内存 (<1 KB)
```

### CPU 使用

```powershell
# 持续监控 1 分钟
1..60 | ForEach-Object {
    (Get-Process wftpg).CPU
    Start-Sleep -Seconds 1
} | Measure-Object -Average
```

**预期**: 平均 CPU < 0.1%

---

## ✅ 验收标准

### 功能要求

- [x] 启动时窗口自动置顶
- [x] 可被系统级 AlwaysOnTop 窗口（如任务管理器）覆盖
- [x] 用户首次交互后自动降级
- [x] 降级后行为与普通窗口一致

### 兼容性要求

- [x] Windows 10/11 正常工作
- [x] 不影响 Linux/macOS（如果支持）
- [x] 不与全屏应用冲突

### 用户体验要求

- [x] 置顶时间适中（3-5 秒）
- [x] 降级自然，不突兀
- [x] 不影响正常使用流程

---

## 🎯 快速测试命令

```powershell
# 一键测试脚本
Write-Host "正在启动 WFTPG..." -ForegroundColor Cyan
Start-Process ".\target\release\wftpg.exe"

Start-Sleep -Seconds 2

Write-Host "`n请观察:" -ForegroundColor Yellow
Write-Host "1. WFTPG 窗口是否在其他窗口之上？" -ForegroundColor Gray
Write-Host "2. 点击按钮后是否可以被其他窗口覆盖？" -ForegroundColor Gray
Write-Host ""
Write-Host "查看日志：" -ForegroundColor Yellow
Write-Host "Get-Content 'C:\ProgramData\wftpg\logs\*.log' -Tail 10" -ForegroundColor DarkGray
```

---

## 📝 测试报告模板

```
测试日期：__________
测试人员：__________
Windows 版本：_______

功能测试:
□ 启动置顶成功
□ 交互后降级成功
□ 无异常行为

性能测试:
□ 内存增加 <1 MB
□ CPU 增加 <0.1%
□ 无卡顿或闪烁

兼容性测试:
□ 与全屏应用无冲突
□ 与系统对话框无冲突
□ 多显示器正常

总体评价:
□ 通过  □ 失败  □ 需改进

备注:
_______________________
```

---

**测试时长**: 约 5-10 分钟  
**难度**: ⭐ 简单  
**重要性**: ⭐⭐⭐⭐ 高
