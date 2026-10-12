# Fire TV 静音、延迟、同步与恢复验证

后续已修复音频MMAP退回慢路径的问题，并验证无限GOP/一帧VBV的同步播放、物理“不闪”观察与进程异常恢复。最新数据见 [音频修复与无限GOP验收](audio-mmap-recovery-2026-10-11.md)，下文保留此前验证记录。

2026-10-11，自动探测本机以太网的直连 /27 子网，发现一个开放的 ADB 端口。连接后确认 Amazon AFTKRT、Android 11，已有授权；设备地址只保存在忽略目录与本机配置中。安装当前自写客户端，未改动 Sunshine / Moonlight。

设备后端现支持明确选择真实 Fire TV：按 Windows 路由取得电脑地址，校验 Amazon / AFT 型号，使用正常 SDR709 与自动硬件解码器选择。模拟器继续使用固定 AVD、goldfish 解码器与专用颜色配置。两种设备共用 Rust core 的状态、静音确认与恢复契约。输入通道绑定选定电脑网卡，保留每会话密钥验证；无需改变默认声音设备。

## 现场听音

先用直接生成的连续测试音排除 Windows 捕获问题，用户确认电视能听到。随后使用 Windows SoundPlayer 播放柔和的合成环境噪声，经实际 WASAPI2 进程回环捕获与 PCM 传输，读取 Windows 静音标志为 true；用户现场确认“电视有声，电脑安静”。环境噪声为合成房间声底，不是真实咖啡厅录音。

电脑音量在测试期间由用户从 0.4 调为 0.2，程序没有改变音量；恢复保留用户最新音量。默认输出始终为现有的 SteelSeries Sonar - Gaming，未安装虚拟扬声器，也未切换三个默认角色。最后恢复的是原静音标志，而不是强制设置为有声。

## 问题与修复

开启同步的首次真机测试约 27fps，采集输出到 Surface 的估计延迟中位约 465ms。主机编码与网络首包较快，但视频在等待音频媒体位置，随后产生解码背压和丢帧。

发现两个相互影响的问题：Windows 音频媒体时间未充分对齐管线主时钟；Fire TV 启动 / HDMI 音频重新打开期间，旧 PCM 在 TCP socket 中积压，播放器按正常速率播放，无法追上实时声音。

改动：

- WASAPI2 使用 `provide-clock=false` 与 `slave-method=re-timestamp`，按共同的 GStreamer 主时钟标记音频。新增主机捕获运行时间 / 音频 PTS 与客户端实际音频时钟诊断。
- 客户端读取完整 PCM 包后，如果 socket 里仍有超过约 40ms 的字节积压，丢弃旧包直到回到有界范围；下一份不连续媒体位置触发音频缓冲与时钟重置。丢弃的数据不计为实际播放，诊断记录 `audioNetworkDrops`。
- AAudio 的断开可能发生在启动、flush 或写入时。现在这些位置统一进入重开输出与重建时钟逻辑；旧缓冲不会继续拖延视频。

保持音频作为视频调度时钟。曾测试在客户端等待目标后立即释放视频帧，结果更差，已移除该实验路径。关闭同步只用于有界诊断对照，默认仍开启同步。

时钟配置的含义见 [GStreamer AudioBaseSrc](https://gstreamer.freedesktop.org/documentation/audio/gstaudiobasesrc.html)。平台的 Surface 定时呈现可能保留缓冲并阻塞后续帧，见 [Android MediaCodec](https://developer.android.com/reference/android/media/MediaCodec)；这与本次背压证据相符，但不能把所有平台迟到都归因于这一机制。

## 延迟与同步测量

真实桌面 + 实际捕获的环境声音，1920×1080、目标 60fps、NVENC H.264、TCP、AAudio，完整逐帧诊断开启。每段前 5 秒不计入稳态统计。

| 指标 | 修复前 | 限制音频积压并对齐时间后 |
|---|---:|---:|
| 稳态有效 Surface 呈现率 | 27.06fps | 56.73fps |
| 采集输出 → Surface，估计 P50 | 464.90ms | 124.53ms |
| 同上，P95 | 731.54ms | 141.31ms |
| 同上，P99 | 748.47ms | 141.80ms |
| 视频解码输出等待 P50 | 301.68ms | 10.52ms |
| 视频呈现间隔 P95 | 283.35ms | 17.32ms |
| Surface 相对音频目标，P50 | 晚 45.76ms | 晚 42.12ms |
| 同上，P95 | 晚 312.39ms | 晚 51.59ms |

修复后的滚动反馈多次约 59–60fps；表内 56.73fps 包含稳态窗口中实际取得呈现回调的结果及长帧，不能用局部 60fps 替代。该段 1756 帧匹配到主机，帧间隔 >25ms 为 51/1755（2.91%）。后续补齐 AAudio 启动 / flush 断开恢复，不改变正常播放的媒体路径。

跨设备时钟为四时间戳探测，半 RTT 不确定度 P50 0.692ms / P95 0.780ms；偏移估计仍受网络不对称影响。采集输出 → Surface 不包括游戏内部、采集前等待、HDMI 与电视物理显示，不是按键到电视的总延迟。

Fire TV 实际 AAudio LOW_LATENCY，设备缓冲 384 帧 / 8ms；PCM 逻辑缓冲约 42ms。主机音频每秒发送阻塞 P95 的中位很小，但该段出现设备 xruns、时钟重置和旧包丢弃，仍需继续改善启动与持续稳定性。8ms 的设备缓冲不能代表整条声音链路只有 8ms。

视频仍比音频调度目标晚约 42ms 中位、52ms P95。这是实际播放时钟与 Surface 报告的对齐误差；物理扬声器 / 显示屏的音画偏差未用摄像或麦克风测量。这次完成了听音验证并定位、降低了积压，没有宣称音画同步和游戏延迟已经优化完成。

原始证据：`local/fire-tv-check/sync-first/report.md`、`bounded-report/report.md`，修复后该段媒体日志 `local/sessions/1791688126424141900-1-0.jsonl`。现场听音的环境噪声会话为 `local/fire-tv-check/ambience-stable-session.log`；各诊断设置和测试信号变化均保留在本机日志，表格是工程前后对照，不是完全控制其他变量的基准测试。

## 使用

最终实际验收通过：103 份健康播放反馈、5 次实际静音确认，覆盖开始、主动中断客户端、恢复、主动终止拥有的媒体主机、恢复、正常结束、再次启动并取消。最终 `Idle`，主机 / 客户端资源均已释放，电脑声音状态 `Original`，静音恢复记录已清除。首次验收暴露的 AAudio 命令断开已修复，重跑结果保存在 `local/fire-tv-check/final-verify.log`。

最终检查：40 个 Rust 测试、7 个状态模拟场景、Clippy / 格式 / 依赖边界，以及 Java RTP、逐帧时钟和音频时钟测试通过。结束后无本次媒体端口监听、无本项目 Fire TV 客户端进程、无静音租约；电脑未静音，用户最新音量 0.2 保留。

```powershell
./start.ps1 -FireTv
./start.ps1 -FireTv -Device <Fire-TV-IP> -SteamBigPicture

# 已连接、授权且安装当前 APK 后：实际中断、恢复与结束验收
cargo run --locked -p sofalink-tui -- --fire-tv <Fire-TV-IP> --diagnostics --verify
# 保留同步的逐帧采样
cargo run --locked -p sofalink-tui -- --fire-tv <Fire-TV-IP> --diagnostics --measure 35
```

ADB 目标由参数明确指定，后端先校验设备身份；清理只结束本项目客户端与拥有的媒体主机。恢复失败保留错误状态及静音恢复记录，不能把已发送恢复命令当成恢复成功。
