# 电脑静音与音频捕获实测

2026-10-11：本机 Windows 的进程回环捕获在输出端点静音后仍能捕获声音。已接入 Rust core、Android TV 后端和 TUI，默认桌面 / Steam 大屏幕会话在实际音画播放确认后静音电脑，停止或异常时恢复原状态。

没有安装声音驱动，没有调用默认设备切换，也没有改动音量。只使用 Windows `IAudioEndpointVolume.SetMute` 修改当前默认输出的静音标志。媒体路径仍为 WASAPI2 进程回环 → PCM 传输 → 客户端 AAudio；明确设置 `loopback-silence-on-device-mute=false`。

## 捕获证据

同一应用播放 440 Hz 测试音，保持应用音量、端点音量和捕获配置不变，只切换端点静音。每段采样 3 秒，48 kHz / 双声道 / S16。

| 指标 | 未静音 | 已静音 |
|---|---:|---:|
| 采样数（双声道合计） | 286080 | 286080 |
| 有效声音采样数 | 284160 | 284160 |
| 归一化 RMS | 0.001821961 | 0.001821999 |
| 最大缓冲 RMS | 0.001844378 | 0.001844341 |

静音前后 RMS 差约 0.0021%，没有变为零或明显衰减。采样程序使用 include-process-tree 隔离测试音应用；实际模拟器会话使用 exclude-process-tree 排除模拟器，二者均使用 Windows 进程回环 API。

本机当前默认输出为既有的 SteelSeries Sonar - Gaming，音量 0.4，原状态未静音。控制测试比较了控制台、多媒体、通信三个默认角色，测试前后全部一致。

另试了普通设备回环：未静音阶段就未捕获到测试音，因此不能用这次结果判断普通设备回环的静音行为。当前结论限定为已验证的进程回环路径；其他音频驱动与独占输出仍需单独验证。

## 真实状态与恢复

core 只有在两份音画反馈均推进、达到健康条件后才发起静音。后端调用 Windows API 后读回实际静音值，成功才回报 `Muted`。TUI / 未来桌面版消费同一份 `host_audio` 状态，不把“已发送命令”当成“已经静音”。

异常或反馈过期进入 `Degraded` 时立即请求恢复；重建媒体连接也先恢复，重新确认播放后再静音。停止、启动取消、退出都走恢复与资源清理；恢复失败时保留恢复记录，界面报告错误，不能宣称已完成清理。旧会话、旧 generation 与旧操作序号的回调不能覆盖当前状态。

恢复的是会话接管时记录的端点及原静音值：原本未静音会恢复未静音，原本已静音仍保持静音。如果用户在会话中手动取消静音，恢复时保留当前未静音选择。音量和默认设备始终由用户控制；切换到其他默认端点后的持续静音追踪尚未实现。

`local/audio-mute.json` 在修改前持久保存端点、原静音值、拥有者 PID 和启动时间。独立守护进程在拥有者被强制终止后恢复，启动时也会检查遗留记录。PID 复用不会被误判为原拥有者。守护进程不在逐帧媒体路径中。控制脚本输出使用文件，避免守护进程继承管道导致 Rust 等待 EOF 卡住。

## 验收结果

- 捕获测试通过：静音前后测试音幅度保持，默认设备、音量和原静音状态恢复。
- Windows 控制测试通过：原本静音 / 未静音、拒绝其他会话释放、用户取消静音，以及强制结束拥有者后的自动恢复。
- 实际 1080p60 桌面串流验收通过：开始 → 播放 → 中断客户端 → 恢复 → 中断主机 → 恢复 → 正常结束 → 再启动并取消。96 份健康反馈、3 次实际静音确认，最终 `Idle`、资源释放、电脑声音 `Original`。
- 静音期间实际主机仍捕获非零 PCM，客户端音频播放计数继续推进；第一段日志到 449 个捕获缓冲时全部非零，客户端已播放 399354 帧。随后主动结束客户端产生的传输错误属于故障注入。
- 40 个 Rust 测试、格式、Clippy、依赖边界和 7 个状态模拟场景通过。

原始记录保存在本机忽略目录 `local/host-mute-check/result-process.json`、`session-verify.log`，本次媒体日志为 `local/sessions/1791687251684520900-1-*.jsonl`。这是本机模拟器与 API / PCM 证据；真实 Fire TV 的扬声器听音、物理电脑扬声器静音尚未验收。模拟器声音也从本机输出，因此静音电脑会同时让本机听不到模拟器。

## 使用与复现

```powershell
# 桌面 / 本地游戏：客户端播放后自动静音电脑
./start.ps1
# Steam 大屏幕使用同一套静音策略
./start.ps1 -SteamBigPicture
# 本机听模拟器声音：保留电脑原状态
./start.ps1 -KeepHostAudio

# 以下音频测试在没有活动串流时运行
powershell.exe -NoProfile -ExecutionPolicy Bypass -File experiments/stream/check-host-mute.ps1
powershell.exe -NoProfile -ExecutionPolicy Bypass -File scripts/check-host-mute-control.ps1

# 已准备好专用 AVD 后，验证实际桌面串流与恢复
$env:LIVINGROOM_VIRTUAL_AUDIO='0'
$env:LIVINGROOM_HOST_MUTE='1'
cargo run --locked -p sofalink-tui -- --emulator --verify
./scripts/check.ps1
```

只有拥有者已退出时才可手动恢复遗留记录：`./scripts/audio-mute.ps1 -Action Recover`。原端点不可用等恢复失败会保留记录，不能删除记录后假装恢复成功。
