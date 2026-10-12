# Rust core 与模拟会话

当前在 Windows 验证状态交互及 Android TV 上的实际音视频。默认入口 `start.ps1` 使用 Rust TUI 和设备后端，复用现有 GStreamer/Python 主机；`start.ps1 -Simulate` 只运行状态模拟器。Rust core 是 library crate，没有终端、网络、进程、GStreamer 或操作系统依赖；当前没有承诺 macOS 实机支持。

## 调用边界

目录与依赖策略见 [monorepo 说明](monorepo.md)。core 内部分为 `model.rs`（共享命令、事件与快照）、`session.rs`（状态机）和 `runtime.rs`（Backend 接口与 effect/event 调度）。模拟器分别管理场景、模拟后端与验收。TUI 的 `app.rs` 负责输入和运行时组装，`view.rs` 只读取快照及显示参数，不访问模拟器或执行命令。未来桌面版直接复用 core 的状态契约。

```text
TUI / 后续 TypeScript 桌面壳
         ↓ Command
     core::Runtime<Backend>
         ↓ Effect       ↑ Event
状态模拟器 / Android TV 后端 / 后续 Fire TV 后端
         ↑ Snapshot
TUI / 后续 TypeScript 桌面壳
```

UI 提交 Command 并读取 Snapshot。Runtime 将 core 的 effects 交给 backend，再处理 backend 回传的 events 和定时器。所有调用保持非阻塞；模拟后端无系统副作用。Android TV 后端在 worker 处理进程/ADB，在 poll 中返回结果与最新实际反馈。

快照包含状态、revision、Token(session/generation)、音画信号、资源、指标、问题、supported_activities 和 available_commands。主操作由 available_commands 决定。序列化格式版本为 1，桌面 IPC 和 TypeScript 自动类型生成是后续工作。

命令 JSON 示例：

```json
{"command":"start","activity":"desktop_stream"}
```

还有 stop、retry 和 recover。重复开始、未完成清理时重试启动、不可用操作返回 CommandError；media_playback 当前返回 UnsupportedActivity。

## 状态与证据

| 状态 | 条件 / 可用操作 |
| --- | --- |
| Idle | 已确认资源释放；开始 |
| Checking / StartingHost / StartingClient | 准备步骤；取消 |
| WaitingMedia | 已连接，尚未证明音画进度；取消 |
| Playing | 音画计数均推进，画面帧率达健康下限；结束 |
| Degraded | 画面/声音进度停止，或反馈过期；恢复连接、结束 |
| Recovering | 重新建立媒体连接；取消 |
| Stopping | 等待主机与客户端资源释放确认；主操作暂停 |
| Faulted + 资源已释放 | 启动失败已经清理；重试启动 |
| Faulted + 资源未确认 | 清理失败；重试清理，禁止重新开始 |

两份递增反馈才足以判定播放。第一份恢复反馈仍显示 WaitingMedia。counter、sequence、合法数值、会话与 generation 都参与验证。恢复和结束使旧 generation 失效；backend 必须取消旧工作，不能只靠 UI 忽略迟到事件。

core 使用主机单调时钟的毫秒数，测试传虚拟时间，TUI 传 Instant 的 elapsed。不会相减两个设备的未同步时钟。默认模拟超时：启动 5 秒、反馈 1.5 秒、恢复 4 秒、清理 3 秒；通过 Timeouts 配置设备预算。Android TV 使用 20/2/20/8 秒，超时判定仍由 core 执行。

45 fps 是识别明显卡顿的健康下限。1080p60 性能验收仍需独立实测，模拟呈现 65 ms、音频缓冲 24 ms 均为合成值，不能用于性能结论。真实音频断流、underrun 等指标尚未接入 Rust 后端。

## 模拟与验收

模拟器在内存中保留主机/客户端资源状态，按固定延时回传结果。故障每个场景注入一次，因此启动失败可重试成功，断流可恢复，首次清理失败可再次清理。

| 按键 / 场景 ID | 行为 |
| --- | --- |
| 1 / healthy | 正常开始、播放、结束 |
| 2 / startup-fail | 主机部分启动后失败，确认清理，再重试 |
| 3 / startup-timeout | 客户端无响应，超时清理，再重试 |
| 4 / audio-stall | 播放后音频计数停止，恢复并重新确认 |
| 5 / video-stall | 播放后呈现计数停止，恢复并重新确认 |
| 6 / disconnect | 播放后反馈中断，过期变异常，恢复 |
| 7 / stop-fail | 首次结束未能释放主机，重试清理 |

```powershell
cargo run --locked -p sofalink-simulator -- check
cargo run --locked -p sofalink-simulator -- check --json
cargo run --locked -p sofalink-tui -- --scenario disconnect
cargo run --locked -p sofalink-tui -- --scenario audio-stall --preview
cargo run --locked -p sofalink-tui -- --scenario audio-stall --snapshot
./scripts/check.ps1
```

测试包括纯 core 的错误/旧反馈/重复操作判断、全部场景、多个启动阶段取消、连续 20 次会话、JSON 契约往返、实际 widgets 渲染、窄终端和退出清理。交互终端另验证 Enter 开始/结束、清理失败提示、重试清理和 Q 退出。

Q / Ctrl+C 请求清理后退出。退出时首次清理失败会自动再尝试清理；持续失败按超时报错，不会宣称资源已释放。raw mode 和 alternate screen 由 Ratatui 管理并在正常退出或错误时恢复。

桌面串流的可选电脑静音策略也由 core 驱动（设备入口默认启用）。共享快照 `host_audio` 区分原状态、正在静音、已静音、正在恢复与错误；`HostAudio` effect/event 带操作序号，防止同一 generation 内迟到的静音确认覆盖恢复请求。实际播放才触发静音，异常或停止触发恢复；清理成功同时要求电脑声音恢复成功。Windows 后端另有持久记录与独立守护进程，已验证拥有者强制退出后的声音恢复，见 [电脑静音实测](host-audio-mute-2026-10-11.md)。强制退出后的终端显示与全部媒体资源恢复仍不属于该声音测试的范围。

## 未来媒体服务

core 关注当前活动的生命周期，媒体服务提供目录与播放资源。Jellyfin 和本地媒体库将作为 provider 候选，桌面串流与媒体播放使用不同后端；会话操作和状态呈现仍复用 core。

当前仅预留活动类型，不实现 provider、鉴权、文件扫描、电影播放或转码服务。这样可以先完成控制流程，同时避免 UI、媒体目录和串流引擎互相绑定。
