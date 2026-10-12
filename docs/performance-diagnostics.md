# 逐帧测量与性能诊断

先测量，再调整媒体路径。主机用 GStreamer 的采集输出、编码提交/输出、RTP 首包输出边界；Android 用重组完成、提交 MediaCodec、取得解码输出、请求呈现目标、Surface 报告的呈现时间、回调送达时间。完整 trace 通过会话 ID + 32 位 RTP 时间戳关联，离线分析器采用 Rust。

## 运行

```powershell
# 正常串流，D 显示阶段 P95 和呈现间隔；轻量窗口测量一直开启
./start.ps1

# 完整逐帧 trace，手动开始/结束
./start.ps1 -Diagnostics

# 有界采样：第一次进入 Playing 后采 35 秒，异常照实记录，最后确认资源释放
cargo run --locked -p sofalink-tui -- --emulator --diagnostics --measure 35

# 仅用于对照：声音继续播放，但视频不等待声音时钟
cargo run --locked -p sofalink-tui -- --emulator --diagnostics --measure 35 --no-sync-video

# 同场景关闭完整 trace，估计观察开销；窗口指标仍然开启
cargo run --locked -p sofalink-tui -- --emulator --measure 35

# --test-source 可替换为测试图和测试音；仍使用实际编解码和声音输出
# 主机 JSONL 文件在 local/sessions；选定具体会话后导出
cargo run --locked -p sofalink-tui -- --report local/sessions/<session>.jsonl --out local/diagnostics/<name>
```

离线报告输出 `report.md`、逐帧结局 `frames.csv`、全帧接收 `receive.csv` 与全解码输出 `decoder.csv`。报告排除每个会话首 5 秒预热，保留 P50/P95/P99/最大值、长帧比例、匹配覆盖、无效记录、丢弃结局及 trace 队列丢弃计数。主机帧间隔和每秒音频统计包含预热，单独标注。取样默认保留音画同步；`--no-sync-video` 只能与有界完整诊断一起使用，不作为正常播放选项。完整 trace 会持续写盘，手动诊断结束后应停止会话。

## 共享状态与呈现

`Snapshot.performance` 是 core 状态契约的一部分，包含独立的主机/客户端测量、反馈时间、过期状态和诊断线索；TUI 和未来桌面版读取同一状态。性能线索不自行证明媒体健康，仍由 Surface/声音实际播放进度驱动会话状态。旧代次、重复序号、负数或非有限指标不能更新状态；结束/恢复时清空。

客户端窗口保留最多 120 个呈现样本，剔除超过 3 秒的旧反馈。帧率根据呈现时间戳间隔计算，不根据统计回调在某一秒集中送达的数量计算；最新呈现超过 2 秒仍会显示停止推进。主机和客户端性能反馈超过 3 秒不再作为当前数值呈现。

TUI 先显示“音频未启用低延迟”“音画同步等待较长”或阶段线索，详情显示排队/解码输出/同步 P95、呈现间隔和主机编码预算。提示是当前窗口的线索；长期归因和优化决策必须结合逐帧报告及控制变量实验。

## 时间口径

| 边界 | 包含 | 不应解释为 |
|---|---|---|
| 采集输出 → 编码提交 | GPU 转换、队列调度 | 游戏内部出帧或全部屏幕采集时间 |
| 编码提交 → 输出 | 编码及 GStreamer 调度 | 纯 NVENC 硬件耗时 |
| RTP 输出 → 客户端首包 | 主机后续发送排队、传输、接收调度 | 纯网络耗时 |
| 重组 → 提交 | 客户端队列和可用输入缓冲等待 | 解码硬件耗时 |
| 提交 → 解码输出 | MediaCodec 调度、解码和背压 | 纯硬件解码时间 |
| 解码输出 → Surface | 请求音画同步等待和平台呈现 | 可直接与同步等待相加的独立阶段 |
| Surface → 回调送达 | 平台回调通知延迟 | 用户看到画面的延迟 |

主机和 Android 单调时钟独立。诊断模式每 500ms 做四时间戳 UDP 探测，保留最近 5 秒中 RTT 最小的样本。报告给出偏移及半 RTT 不确定度，拒绝陈旧探测；该不确定度不包含额外漂移或平台时间戳误差。跨时钟结果是估计，不能直接相减原始时间。负的 Surface 与请求目标差也保留，不假设模拟器严格兑现请求时间。

Surface 时间来自 [OnFrameRenderedListener](https://developer.android.com/reference/android/media/MediaCodec.OnFrameRenderedListener)。回调允许明显迟到或分批送达；API34 起，在 codec 保持 executing 到通知发出时，已呈现的帧应得到回调。旧版本可能实际呈现却不产生回调。因此跨不同 Android 版本比较“回调缺失率”需要特别谨慎。它不是屏幕物理显示时间；采集输出到 Surface 也不是按键到屏幕延迟。各阶段分位数不能相加为端到端分位数。

## 原始记录

`host_frames.rows`：`[rtp, captureReadyNs|null, encoderSubmitNs, encoderOutputNs, rtpHandoffNs]`。

`client_frames.trace.rows`：`[rtp, firstRxNs, assembledNs, submittedNs, decodedNs, targetNs, presentedNs, callbackNs, queueDepth, inFlightDepth, audioBufferUs, outcome]`。

独立 `trace.arrivals` 记录全部完成重组的帧 `[rtp, firstRxNs, assembledNs]`，包括后来没有呈现回调的帧；`trace.outputs` 记录全部获得解码输出的帧 `[rtp, submittedNs, decodedNs, targetNs, releaseNs, requestedRender]`。诊断接收/解码节奏必须使用这些独立记录。不能用最终呈现帧的接收时间子集判断网络/接收卡顿，子集存在选择偏差。客户端累计收到/提交/输出/请求呈现/回调计数也分别记录。

结局 0 是观察到呈现回调，1 等待 IDR，2 队列溢出丢弃，3 超过播放目标主动丢弃，4 decoder flush，5 等待呈现回调的计时记录超过 240 条被淘汰。**5 不证明视频由应用主动丢弃**；未呈现或未获得回调都可能造成积累。`inFlightDepth` 也是计时记录数，不是 MediaCodec 硬件队列长度。停止时仍等待回调的记录可能不在 CSV 中，主机/客户端总行数之差不能直接当成网络丢包率。

客户端 trace 队列最多 240 帧、反馈任务最多 64 个；主机异步写入队列最多 240 帧。媒体探针不等待写盘。报告分别统计 frame trace 和反馈包队列丢弃，但 UDP 本身没有逐包确认，零队列丢弃仍不能证明诊断包全部到达。报告匹配率必须一起检查。

AAudio 记录实际 mode、buffer/burst/capacity，避免只因请求 LOW_LATENCY 就认定已获得低延迟路径。参见 [AAudio 官方说明](https://developer.android.com/ndk/guides/audio/aaudio/aaudio)。主机音频记录每秒队列年龄/发送阻塞 P95；这些秒级分位数的分布不是全部音频包的全局分位数。

原生音频现在使用预分配的单生产者 / 单消费者 PCM 队列。`audioQueueLimitFrames` 是应用队列上限，不是设备实际缓冲量；`audioSourceUnderruns` / `audioSilenceFrames` 记录回调缺少 PCM 时的补零，独立于设备 `audioUnderruns`。`audioClockDrops` 记录回调时间线队列溢出。发生补零时，设备帧位置与收到 PCM 的实际播放位置分别映射，补零不推进播放证据。`videoSyncScheduledFrames` 是实际按音频时钟请求调度的累计帧数，`videoSyncClockMisses` 是开启同步却没有可用或合理时钟目标而立即呈现的累计帧数；配置开关为真不等于所有帧都同步成功。

音画映射通过一个不可变快照发布，避免读取到来自不同更新的媒体时间、设备时间或 RTP 偏移。映射同时校验观察时间和设备时间的新鲜度；输出时间戳失效时音频健康指标也会变旧。原生队列与补零映射检查：`./experiments/stream/check-audio-native.ps1`，仅在专用 Android TV 模拟器运行。与 Java 时钟、RTP 和逐帧测试一起执行。

窗口测量本身仍有 Java 分配和 Python/GStreamer 探针开销。本次只比较完整 trace 开关的增量开销，不能据此宣布测量零开销。当前基线见 [2026-10-11 实测](performance-baseline-2026-10-11.md)。

主机配置记录 `color_profile` / `colorimetry`；客户端实际解码格式记录 `videoColorStandard` / `videoColorRange` / `videoColorTransfer`。当前 goldfish 模拟器显示颜色存在已实测的矩阵兼容问题，模拟器后端采用专用配置；固定 RGB / 灰阶截图检查见 [颜色诊断](color-diagnostics-2026-10-11.md)。
