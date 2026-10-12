# 虚拟音频输出与游戏串流

Steam Streaming Speakers 是 Steam 的虚拟音频驱动提供的 Windows 输出设备。Sunshine 可安装 / 复用该设备，用于主机静音和环绕声；它不是 Steam 大屏幕的专用音频 API，也不负责视频串流或游戏启动。

来源：[Sunshine 配置说明](https://docs.lizardbyte.dev/projects/sunshine/latest/md_docs_2configuration.html)、[Windows 音频实现](https://github.com/LizardByte/Sunshine/blob/master/src/platform/windows/audio.cpp)。本机已有设备，不安装或重分发 Steam 驱动。

## 会话行为

`./start.ps1 -SteamBigPicture -VirtualAudio` 启动大屏幕与 TUI；在 TUI 真正开始串流时，backend 获取音频路由租约，将 Windows Console / Multimedia 默认输出切到虚拟扬声器。普通本地游戏用 `./start.ps1 -VirtualAudio`，两者共用同一音频链路。游戏显式绑定其他输出设备时不会自动改绑，已经运行且不跟随默认输出变化的游戏需重新选择输出或在切换后启动。

通信角色不改动；默认立体声 48kHz。当前未完成环绕声传输，也没有本地游戏启动器 / 虚拟手柄支持。

停止、启动失败、媒体重建与正常退出时释放路由租约，按开始前记录恢复输出。若用户在会话中手动改过默认输出，则保留用户选择。恢复失败不报告资源已释放。

`local/audio-route.json` 在切换之前保存原设备、目标、拥有者 PID / 启动时间、会话标识。进程被强制结束时无法执行 finally；下一次获取租约会恢复死进程记录，也可运行 `./scripts/audio-route.ps1 -Action Recover`。有活会话时拒绝 Recover，不允许其他会话释放。恢复目标设备不可用时保留日志并报告失败。文件锁用于本仓库工具之间的互斥，不能阻止 Sunshine 或 Steam 自己同时更改系统输出；使用时只启动一个串流服务。

音频路由是 Windows 控制面；当前使用 PowerShell / C# COM 桥接，不处理 PCM、不进入实时回调。Rust core 保持独立，backend 管理租约。默认输出切换使用未公开的 IPolicyConfig，调用失败必须显式报告。路由成功仅证明默认设备已切换；Playing 仍由实际媒体进度确认。

## 模拟器与真实客户端

模拟器也通过这台 Windows 电脑播放声音。如果模拟器输出跟随默认设备切到虚拟扬声器，可能听不到电视端的声音。因此 `-VirtualAudio` 目前显式启用；听音验收前需要把模拟器应用的输出固定到物理耳机 / 扬声器。

当前模拟器 backend 继续使用 WASAPI **进程回环**，排除模拟器进程树以避免回采；它不等同于“只采虚拟端点”，仍会采其他应用声音。真实 Fire TV 不在主机上播放声音，可使用端点回环；主机 CLI 已支持 `--audio-device <WASAPI endpoint ID>`，与 `--audio-exclude-pid` 互斥。

本机 GStreamer WASAPI2 直接初始化 Steam 端点时返回 `0x887c001a`，尝试 F32 / S16 与普通 / 低延迟配置均未解决。原生 WASAPI 使用设备混音格式曾成功初始化，但重复探测也遇到 E_NOTIMPL，未作为启动前提；进程回环可捕获发送到虚拟扬声器的测试音。不能据此宣布驱动损坏，也不能将端点回环标为已通过。后续真实客户端需定位端点初始化兼容性或采用经过验证的进程回环。

## 验证

- `./scripts/audio-route.ps1 -Action Inspect`：查看默认输出、Steam 端点与租约。
- `./scripts/check-audio-route.ps1`：实际切换、错误拥有者拒绝、活租约恢复拒绝、用户改变输出保留、正常恢复、死进程恢复。
- `./experiments/stream/check-virtual-audio.ps1`：普通测试应用向虚拟默认设备播放 440Hz 音，进程回环捕获 382080 个双声道样本，峰值窗口 RMS 0.01306，通过；这证明无需 Steam 游戏即可进入音频路径，不等于真实游戏或电视听音验收。
- `./experiments/stream/check-virtual-audio.ps1 -Capture endpoint`：独立检查端点回环；当前机器失败，保留真实错误。
- `$env:LIVINGROOM_VIRTUAL_AUDIO='1'; ./target/debug/sofalink-tui.exe --emulator --verify`：实际媒体会话与强制中断 / 恢复 / 停止 / 启动取消验收。

2026-10-11 最终串流验收通过，97 条健康样本；真实客户端退出、主机退出后的恢复、正常停止和启动取消都返回 Idle。最终默认输出恢复到原来的 SteelSeries Sonar - Gaming，两种默认角色均与开始前相同，租约记录已删除。此次验收使用桌面系统声音与实际音画进度，不能代替 Steam / 本地游戏内容的听音验收。

验证过程中临时的格式调整已撤回，最终实现不改变设备格式。测试音 / 捕获记录在忽略目录 `local/virtual-audio-check`。切换与格式试验不是延迟前后对照，不声明性能提升。
