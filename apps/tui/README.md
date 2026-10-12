# sofalink-tui

Windows 上验证的状态与设备交互入口。应用组装 core Runtime 与 simulator/emulator 后端；TUI 不维护第二份播放状态机。

- `src/app.rs`：模拟器选择、命令提交、详情显示和退出意图等应用控制。
- `src/view.rs`：只读渲染，输入为 Snapshot + ViewContext，不访问模拟器或 Runtime。
- `src/lib.rs`：应用组装及无终端渲染入口。
- `src/main.rs`：终端初始化、时钟、键盘事件和退出恢复。
- `src/verify.rs`：可选的 Android TV 实际播放、中断、恢复和清理验收。
- `tests/presentation.rs`：共享命令及实际 widgets 呈现测试。

available_commands 来自 core；健康判断、恢复与清理结果也来自 core。ViewContext 仅包含场景说明、显示选项、提示与状态历史。未来桌面版直接共用 core 的命令和快照，不依赖本 TUI 的 Ratatui widgets。

```powershell
./start.ps1
./start.ps1 -TestSource
./start.ps1 -Simulate
cargo run --locked -p sofalink-tui -- --scenario audio-stall --preview
```
