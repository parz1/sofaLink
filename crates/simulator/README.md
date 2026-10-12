# sofalink-simulator

实现 core 的 Backend，提供内存资源、虚拟时间和可重复故障。依赖方向为 simulator → core，不依赖 TUI 或真实设备工具。

- `src/scenario.rs`：场景标识与说明。
- `src/backend.rs`：模拟 effect 执行、资源和反馈。
- `src/check.rs`：通过共享 Runtime 自动演练全部流程。
- `src/main.rs`：无界面的检查命令和 JSON 输出。
- `tests/sessions.rs`：取消、连续会话与序列化契约测试。

模拟器不判定 authoritative 会话状态；它回传事件，状态由 core 决定。无需 Android 模拟器、网络、ADB 或电视。

```powershell
cargo run --locked -p sofalink-simulator -- check
cargo run --locked -p sofalink-simulator -- check --json
```
