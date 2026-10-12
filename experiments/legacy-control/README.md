# 旧 C# 控制原型

此目录保存迁移到 Rust 之前的 C#/.NET 控制核心、设备适配器、TUI 和验证程序，保留用于实机对照。它不属于 Cargo workspace，也不是默认入口。

从仓库根目录运行：

```powershell
dotnet build experiments/legacy-control/LivingroomHub.sln
dotnet run --project experiments/legacy-control/tests/Checks
dotnet run --project experiments/legacy-control/src/Cli -- help
```

doctor、inspect、launch、home 等诊断选项见 help。旧 tui/session-check 会调用仓库根目录的 config.local.json、local 工具和 experiments/stream 主机，可能操作真实设备；不用于 Rust 状态模拟验证。

内部项目的相对引用保持不变。新的核心位于 crates/core，新的 TUI 位于 apps/tui。
