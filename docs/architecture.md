# 家庭娱乐控制架构

sofaLink 负责协调电脑和客厅设备。Rust 主线已在 Windows、Android TV 模拟器和真实 Fire TV 上验证音视频会话。Python 控制原生 GStreamer 主机管线，Java/C 客户端执行硬件解码和音频输出，均通过设备后端接入 core。Rust 媒体引擎迁移仍是后续工作。适配范围、能力协商与平台边界见 [设备适配设计](device-adaptation.md)。

工程哲学：底层实现一切向着性能，GUI 一切向着直观。底层通过硬件路径、有界缓冲和分段测量验证选择；界面提供清楚的连接、开始、结束与错误状态，技术细节保留在诊断入口。

设备探测、版本化音视频请求、纯协商规则、实际计划与共享快照已接入；当前接口和 Windows / Fire TV 回归见 [通用音视频协商](media-negotiation-2026-10-12.md)。

`crates/core` 是唯一的主线状态来源，既不依赖终端，也不执行设备 I/O。`crates/simulator` 实现模拟后端，`crates/emulator` 负责实际设备与进程 I/O；`apps/tui` 使用与未来桌面应用相同的 Runtime、Command 和 Snapshot。Fire TV 已验证电视有声、电脑静音、颜色稳定和异常恢复；物理音画同步偏差与电视显示延迟仍需进一步测量。

## 模块与边界

| 模块 | 责任 |
| --- | --- |
| `crates/core` / Rust | 命令、状态、证据判断、effect/event、超时、清理与可用操作；已实现 |
| `crates/simulator` / Rust | 确定性虚拟时钟、模拟资源、故障与自动验证；已实现 |
| `crates/emulator` / Rust | Android TV ADB、主机进程、反馈、取消与释放确认；已实现 |
| `crates/input` / Rust | 独立输入回传协议、按键状态与断线释放、Windows 扫描码/鼠标注入；已实现 |
| `apps/tui` / Rust + Ratatui | 呈现快照与提交命令；已实现 |
| Fire TV backend | 已在 `crates/emulator` 内接入真实电视控制、客户端能力预检与音视频反馈；后续拆分适配器 |
| Media engine / Rust + GStreamer | 捕获、硬件编码、传输与同步；待迁移实验 |
| Desktop UI / TypeScript + Tauri | 共用 core，提供游戏库与媒体库入口；未实现 |
| Media provider | 连接 Jellyfin 等服务，提供目录、元数据、鉴权与播放资源；未实现 |

核心状态和命令支持 serde 序列化，快照包含 schema_version、revision、会话/代次标识、音画状态、资源状态、问题和 available_commands。后续桌面壳通过同一 Runtime 调用；IPC 传输和 TypeScript 类型生成尚未实现。TUI 不复制状态机，也不自行推断恢复按钮是否可用。

## 会话流程

Idle → Checking → StartingHost → StartingClient → WaitingMedia → Playing → Stopping → Idle。

异常路径：Playing → Degraded → Recovering → WaitingMedia → Playing。启动失败或超时先进入 Stopping，清理确认后进入 Faulted；用户可以重试启动。清理失败进入 Faulted 并保留未知资源，主操作为重试清理，禁止重新开始。

应用已启动不等于播放成立。core 要求两份递增且有效的音画反馈，画面呈现和音频实际播放计数均推进，且视频帧率至少 45 fps（健康下限，不是 1080p60 性能验收）。反馈过期、画面停止或声音停止都会进入异常状态。恢复必须重新确认进度，旧会话/代次、重复序号和非法指标被忽略。

core 发出非阻塞 effects，backend 回传实际 evidence/events。取消、结束和恢复会更换 generation，backend 必须取消旧操作；两端资源释放都得到确认才显示 Idle。模拟器已验证此行为。未来真实设备需要补充持久化设置快照、进程崩溃恢复和用户设置冲突处理，当前没有实现这些系统能力。

## Jellyfin 与媒体库边界

媒体来源和会话执行分别设计。未来 provider 提供目录、搜索、鉴权、媒体描述和播放地址；session backend 使用这些资源控制客户端播放。视频文件播放优先考虑客户端直接播放兼容资源，不必先在电脑打开电影再捕获桌面。是否需要转码由客户端能力和资源格式决定。

当前 ActivityKind 预留 MediaPlayback，但 core 会返回 UnsupportedActivity；仅支持 DesktopStream 会话。没有实现 Jellyfin 客户端、服务端、转码、账号或媒体库。先接现有服务还是自建服务留待具体需求明确，核心不绑定某一个提供商。Jellyfin 的能力和现有 SDK 参考 [官方文档](https://jellyfin.org/docs/index.html) 和 [TypeScript SDK](https://typescript-sdk.jellyfin.org/)。

## 历史 Fire TV 验证与后续接入

首先验证 ADB 授权、Fire OS 版本、Moonlight 实际包名、启动入口、UI 层级与截图可用性，再选择控制路径。优先使用经验证的应用入口和状态查询；界面操作读取当前状态后再执行，并确认结果，避免仅依赖坐标和固定等待。

初次 ADB 授权与 Moonlight 配对需要设备端交互。Fire TV 深度休眠后的连接恢复、电视电源和 HDMI 切换需分别验证，不预设 ADB 能完成所有操作。

## Agent 工具接口

规划提供 get_devices、get_session_status、start_session、stop_session、get_diagnostics、capture_client_screen 等结构化工具。工具返回状态、耗时和错误码，Agent 与桌面按钮使用同一套执行逻辑。

正常流程由确定性程序执行；Agent 处理意外界面与故障解释。设备命令限定到适配器支持的操作；截图按需获取，日志与公开问题报告隐藏设备地址和凭证。模型提供商和密钥配置留到 Agent 接入阶段确定。

## 延迟诊断

当前音频优化与逐帧前后对照见 [2026-10-11 音频实测](audio-optimization-2026-10-11.md)，主机和客户端分层对照见 [Sunshine 与当前架构](sunshine-comparison.md)。

记录本程序的步骤耗时与设备状态；可获取时分别收集主机编码、网络、客户端解码和丢帧信息，并保留数据来源。控制流程耗时不能代替游戏输入到电视显示的端到端延迟。

先用同一输入比较电脑与电视响应，再控制分辨率、帧率、码率和网络条件做对照。现有 Sunshine 日志的编码器探测不能证明实际会话性能。

## 发布与依赖

首版通过进程和协议调用已安装的软件，暂不复制 Sunshine 或 Moonlight 源码，也不打包其二进制。公开发布前核对各依赖许可证、再分发条款和署名要求，并选择本项目许可证。
