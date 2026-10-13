# sofaLink 桌面浮窗

Windows 原生 Rust 浮窗，使用 core 的 FocusCore 与 GameSpaceCore。支持开始／结束串流、虚拟屏幕回收、设置里的彻底卸载，以及拿回工作窗口、返回游戏、把游戏移到电视屏幕。当前处于 Windows／Fire TV／Lost Judgment 实验阶段。

在仓库根目录、PowerShell 7 中构建和运行：

```powershell
cargo build -p sofalink-desktop
./desktop.ps1
```

入口读取本机配对设备和 GStreamer 环境，也可传入 `-Device <IP> -Python <路径> -Shell <pwsh路径>`。打开界面只检查状态，点击开始才检查电视授权、申请 Windows UAC 并创建屏幕。结束依次停止媒体、确认游戏回到主屏、回收设备；设置里的卸载移除 Windows 驱动包，下次开始可重新安装。回收失败则显示恢复操作并禁止新会话。

Ctrl+Alt+F9 拿回电脑，Ctrl+Alt+F10 返回游戏。浮窗不激活自身，不自动抢回焦点。保留 `-FocusOnly -GameProcess LostJudgment -MonitorHandle <handle>` 供已有会话使用。显示句柄每次拓扑变化后重新探测。

关闭管理浮窗会等待同一套有序回收流程，不强制退出游戏。游戏退出、媒体进程结束或电视音画停滞会进入恢复；桌面进程被强杀后，媒体运行器停止，下次打开界面检查遗留归属记录再恢复。生命周期见 [架构说明](../../docs/desktop-focus-architecture.md)。

本轮已检查编译、状态契约、脚本语法与无设备状态恢复。Fire TV 创建／结束／卸载实机回归仍待电视 ADB 授权后进行。
