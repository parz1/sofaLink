# sofaLink 桌面浮窗

Windows 原生 Rust 浮窗，使用 core 的 FocusCore 与 FocusSnapshot。界面支持拿回最近的工作窗口、返回游戏，以及将偏离的游戏窗口移回指定的串流屏幕。当前处于独立游戏空间实验阶段，尚未成为正式主机控制界面。

从仓库根目录构建和运行：

```powershell
cargo build -p sofalink-desktop
./desktop.ps1 -GameProcess LostJudgment
```

绑定电视专用屏幕时，先通过 `experiments/game-space/display-probe.ps1` 获取当前 HMONITOR，再传入 `desktop.ps1 -MonitorHandle <handle>`；原生显示句柄每次拓扑变化后都要重新探测，不能保存为长期配置。完整 Fire TV 实验使用 `experiments/game-space/run-focus.ps1`，参数和回收流程见 [架构说明](../../docs/desktop-focus-architecture.md)。

Ctrl+Alt+F9 拿回电脑，Ctrl+Alt+F10 返回游戏。先实际选择一个工作窗口，core 才能记住并返回它。浮窗不激活自身，不自动抢回焦点；快捷键被占用或 Windows 拒绝切换会显示对应处理路径。位置和前台窗口都经过实际确认，游戏或工作进程退出后不复用旧身份。

关闭浮窗不会强制退出游戏。通过完整实验运行器启动时，关闭浮窗会正常停止该测试串流、返回电视启动页并恢复拥有记录的游戏窗口位置。用户主动退出游戏时，运行器结束测试；失效的原窗口记录在下一次实验前归档，不能恢复到新的游戏进程。
