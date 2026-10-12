# sofalink-core

共享状态契约与会话控制。唯一外部依赖为 serde；不依赖模拟器、TUI、终端、网络、进程或媒体框架。

- `src/model.rs`：Command、Snapshot、Effect、Event、Token 与资源/指标模型。
- `src/session.rs`：状态机、证据判断、超时、恢复与可用命令。
- `src/runtime.rs`：Backend 契约及 effect/event 调度。
- `src/lib.rs`：稳定的公共导出；调用方不依赖内部文件布局。
- `tests/contracts.rs`：状态与命令契约测试。

后端实现 Backend；界面提交 Command 并读取 Snapshot。core 决定能否开始、是否正在播放、是否恢复以及清理是否完成。后端负责非阻塞执行及真实证据，界面负责文案与布局。

当前仅支持 DesktopStream。MediaPlayback 为未来活动类型，返回 UnsupportedActivity。桌面壳和 Jellyfin 接入尚未实现。
