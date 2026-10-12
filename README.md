# sofaLink

游戏串流当前入口：`./start.ps1 -SteamBigPicture`，打开 Steam 大屏幕并运行实际桌面/系统声音串流，模拟器方向键/确认回传到 Windows。键鼠输入状态独立显示，断线释放已按住的键。操作与支持范围见 [输入回传](crates/input/README.md)。

把电脑游戏带到电视上的串流项目。先完成从电视一键进入 Steam 大屏幕、游玩和退出的完整闭环，再考虑其他游戏入口与独立媒体能力。

**第一个 milestone：电视端一键进入 PC 的 Steam 大屏幕，选择并游玩游戏，结束游戏后退出大屏幕与串流，恢复电脑状态，回到电视启动页。** 首次配对后日常操作从电视发起；Windows 已开机、Steam 已安装并登录。当前仍需电脑端启动，电视发起控制与完整游戏闭环尚未完成；验收标准见 [开发路线](docs/roadmap.md)。

当前开发范围为 Windows。主线采用 Rust core、状态模拟器与 Ratatui TUI；未来 TypeScript 桌面版共用同一个 core。Android TV 后端已连接现有 GStreamer 主机与自写客户端，支持模拟器与真实 Fire TV；Rust 媒体引擎迁移留待后续。

工程哲学：**底层实现一切向着性能，GUI 一切向着直观。** 通过有界队列、硬件路径和分段测量验证性能；操作入口逐步采用 TUI，再提供直观的桌面界面。

## 当前状态

显示呈现已加入通用 VSYNC 相位对齐，保持音频时钟同步与原缓冲。Fire TV 的约 42ms 原始指标包含约 33.7ms 的 MediaCodec 时间报告偏差；独立合成器围栏对照、测量边界和回归见 [呈现优化实测](docs/presentation-optimization-2026-10-12.md)。

音视频请求、设备能力与协商结果已整理为 core 的版本化接口；Windows 实际试编码，Fire TV 检查解码、显示与实际音频模式，开始和恢复都重新协商。默认要求低延迟音频，TUI 与未来桌面版共用 Snapshot。配置、验证与限制见 [通用音视频协商](docs/media-negotiation-2026-10-12.md)。

已完成同源编码闪动对照，并修复Fire TV低延迟音频路径退回普通输出的问题。正常Fire TV入口采用能力协商、无限GOP、一帧VBV与按需IDR；实际同步播放、用户“有声音，不闪”确认及异常恢复已验证。实测与剩余边界见 [音频修复和Fire TV验收](docs/audio-mmap-recovery-2026-10-11.md)，编码依据见 [Sunshine对照](docs/encoder-flicker-ab-2026-10-11.md)。

真实 Fire TV 入口：`./start.ps1 -FireTv`（读取本机 `config.local.json`），或 `./start.ps1 -FireTv -Device <IP>`；Steam 大屏幕可加 `-SteamBigPicture`。已现场确认电脑静音时电视仍有环境声音，延迟、同步偏差与恢复证据见 [Fire TV 验证记录](docs/fire-tv-validation-2026-10-11.md)。

逐帧测量、时钟探测、Rust 离线报告和共享性能状态已接入。使用 `./start.ps1 -Diagnostics` 记录完整 trace，D 查看阶段 P95；自动采样与当前瓶颈结论见 [性能诊断](docs/performance-diagnostics.md)。

AAudio 回调、实际音频时间戳映射和缓冲优化已完成模拟器对照，见 [音频优化实测](docs/audio-optimization-2026-10-11.md)；与上游媒体路径的区别见 [Sunshine 架构对照](docs/sunshine-comparison.md)。

Steam 大屏幕与本地游戏默认在客户端实际音画播放后静音电脑，结束或异常时恢复原静音状态；不安装虚拟扬声器、不切换默认设备、不改变音量。静音后的进程回环捕获和完整模拟器会话已验证，见 [电脑静音实测](docs/host-audio-mute-2026-10-11.md)。在这台电脑上听模拟器声音时用 `./start.ps1 -KeepHostAudio`。

模拟器颜色偏移已通过固定 RGB / 灰阶截图定位并加入专用兼容配置；复现检查与边界见 [颜色诊断](docs/color-diagnostics-2026-10-11.md)。

已实现 Rust core、确定性的状态模拟器和状态驱动的 TUI。默认入口启动 Android TV 模拟器；按 Enter 开始实际桌面音视频串流，再按 Enter 结束。core 根据实际客户端音画播放进度判断播放，根据进程退出及客户端消失确认清理；客户端被中断时显示异常，可按 Enter 恢复连接。

状态模拟器仍可单独运行，不连接设备，不需要 Python、.NET、ADB 或 GStreamer。

7 个合成场景覆盖正常播放、启动失败、启动超时、声音停止、画面停止、反馈中断和清理失败。设备模式使用实际反馈并在界面明确标注；合成模式的指标不能用于性能结论。Android TV 虚拟解码的播放结果也不代表 Fire TV 硬件性能。

## 运行 Rust 主线

需要 Windows 和 Rust 工具链（Rust 1.88 或更新版本；本机使用 1.96）。设备模式还需要本地 GStreamer/Python 工具链与已安装客户端的 Android TV AVD，准备方法见 [模拟器说明](docs/android-tv-emulator.md)。在项目根目录运行：

```powershell
./start.ps1
# 在电脑上听模拟器声音，保持电脑原静音状态
./start.ps1 -KeepHostAudio
# 可重复的测试图与测试音，仍使用真实编码/解码/播放
./start.ps1 -TestSource

# 只运行状态模拟器
./start.ps1 -Simulate
cargo run --locked -p sofalink-tui

# 直接选择断线场景
./start.ps1 -Scenario disconnect

# 无需终端交互，自动验证全部场景
cargo run --locked -p sofalink-simulator -- check

# 实际设备验收：开始、播放、中断客户端与主机、恢复、结束、启动取消
cargo run --locked -p sofalink-tui -- --emulator --test-source --verify

# 查看真正的 TUI widgets 渲染结果和共享 JSON 快照
cargo run --locked -p sofalink-tui -- --scenario audio-stall --preview
cargo run --locked -p sofalink-tui -- --scenario audio-stall --snapshot

# 统一检查依赖边界、格式、Clippy、测试与全部模拟场景
./scripts/check.ps1
```

Enter 执行当前主操作；S / Esc 结束或取消；D 切换详情；合成模式就绪时按 1–7 选择场景；Q / Ctrl+C 清理后退出。建议终端至少 56 列、21 行，小窗口保留主要状态和操作。默认设备模式捕获桌面与系统声音，并排除模拟器输出以避免回采。

TUI 显示电脑声音的实际确认状态：等待播放 → 正在静音 → 已静音；结束或异常时显示正在恢复，确认后才显示已恢复。此前的 `-VirtualAudio` 实验仍可显式选择，行为与限制见 [虚拟音频说明](docs/virtual-audio.md)；默认入口使用直接静音。

代码入口：[core](crates/core/src/lib.rs)、[模拟器](crates/simulator/src/lib.rs)、[TUI](apps/tui/src/lib.rs)。目录归属与依赖方向见 [monorepo 说明](docs/monorepo.md)，状态契约、超时和验证范围见 [Rust core 说明](docs/rust-core.md)。根目录 `cargo run --locked` 默认运行 TUI。

## 保留的实机实验与旧诊断原型

Android TV 客户端验证使用独立 AVD，启动与安装入口见 [Android TV 模拟器说明](docs/android-tv-emulator.md)。设备后端与 Rust 状态模拟器使用相同 core 和 TUI。

先前的 C# 设备/会话原型及测试集中保存在 [experiments/legacy-control](experiments/legacy-control/README.md)，保留作对照，不再是默认入口。Python GStreamer 主机、Java/C 客户端位于 [串流实验](experiments/stream/README.md)。当前 Fire TV 音视频与恢复验收见上方记录；完整游戏游玩与电视端一键控制仍属于未完成的第一个 milestone。

需要 .NET 9 SDK 和独立安装的 [Android SDK Platform Tools](https://developer.android.com/tools/releases/platform-tools)。ADB 连接步骤参考 [Amazon 官方说明](https://developer.amazon.com/docs/fire-tv/connecting-adb-to-device.html)，首次连接需在电视上授权。

```powershell
dotnet build experiments/legacy-control/LivingroomHub.sln
dotnet run --project experiments/legacy-control/src/Cli -- doctor
dotnet run --project experiments/legacy-control/src/Cli -- inspect --device 192.168.1.50
dotnet run --project experiments/legacy-control/src/Cli -- launch --device 192.168.1.50
dotnet run --project experiments/legacy-control/src/Cli -- home --device 192.168.1.50
dotnet run --project experiments/legacy-control/tests/Checks
```

示例 IP 需要替换为自己的 Fire TV 地址。ADB 不在 PATH 时附加 `--adb D:\tools\platform-tools\adb.exe`；默认应用包名为 `com.limelight`，可用 `--package` 覆盖。每次 ADB 调用最多等待 15 秒，可按 Ctrl+C 取消。输出为 JSON，成功退出码为 0，失败为 1，取消为 2。前台状态无法解析时为 null，不会推断成功。

`doctor` 检查 ADB 和 Sunshine 进程是否存在，不验证 Sunshine 的串流健康状态，也不连接设备。`launch` 只启动 Moonlight，尚未选择主机或游戏；`home` 只返回 Fire TV 主页，不能保证结束正在运行的串流。两者都不是完整的会话控制命令。

## 后续真实设备体验

1. 首次设置完成 Fire TV 的设备授权，保存设备和游戏配置。
2. 点击开始：检查设备，保存需要修改的电脑设置，准备串流环境，启动游戏与自写电视客户端。
3. 每一步读取状态并确认成功；失败时给出具体原因，允许重试或恢复。
4. 点击结束：停止本次会话，恢复本程序修改过的电脑设置。

电视开机、切换 HDMI 与游戏模式属于独立设备能力，按实际电视支持情况接入。

## 技术方案

Rust core 持有会话状态、超时、证据判断、清理与可用命令；TUI 和未来 TypeScript/Tauri 桌面版负责呈现和输入。Android TV 设备后端支持模拟器与 Fire TV；GStreamer 的 Rust 主机是后续工作。模拟后端与设备后端遵守同一套 effect/event 契约。

Jellyfin 方向按独立媒体来源考虑：浏览媒体库、取得播放资源与桌面串流是不同能力，未来共享会话控制与 UI。当前 `media_playback` 返回不支持，不会伪装成可播放。

## 仓库与许可证

公开源码仓库：[parz1/sofaLink](https://github.com/parz1/sofaLink)。项目目前处于开发实验阶段，尚未提供面向普通用户的安装包。许可证尚未选定；依赖与参考项目各自的许可证继续适用。本机配置、设备地址、日志、工具下载与 APK 签名密钥均不提交。

详见 [技术架构](docs/architecture.md) 和 [开发路线](docs/roadmap.md)。

## 参考资料

- [Sunshine 官方项目](https://github.com/LizardByte/Sunshine)
- [Moonlight Android 官方项目](https://github.com/moonlight-stream/moonlight-android)
- [Amazon Fire TV ADB 连接说明](https://www.developer.amazon.com/zh/docs/fire-tv/connecting-adb-to-device.html)
