# 显示呈现定位与 VSYNC 对齐

2026-10-12，Windows → Fire TV AFTKRT / Android 11。结论：此前约 42ms 不能全部解释为真实显示等待。MediaCodec.OnFrameRendered 提供的原始时间比同帧 SurfaceFlinger 呈现围栏晚约 33.7ms（约两个输出刷新周期）。这个偏差不是 Java 回调送达延迟；后者单独记录。不能将这个机型的 33.7ms 硬编码进通用统计或同步控制器。

## 同源单变量对照

视频源每轮重建同参数 `videotestsrc pattern=ball`，1080p60；这是一段确定性生成的动态参考图，并非录制文件。音频每轮播放同一个咖啡厅环境 WAV，由 Windows 实际播放并回环捕获，电视开始播放后静音电脑。WAV SHA256：`50603D891BF3CE1710EA5522CB44B60C83B018CD0E5AEDB5B5AAC1CD85B8550C`。

所有轮次保持 NVENC High/P4/ULL、20Mbps、无限 GOP、一帧 VBV、零 B 帧、相同解码器/SurfaceView、音画同步开启、AAudio fast 输出及相同缓冲。电视保持原有 3840×2160 / 59.94Hz HDMI 输出。A 为直接请求音频目标时刻，B 为 VSYNC 对齐。每轮播放约 35 秒，逐帧报告排除前 5 秒。顺序 A2→B2→B3→A3。

每轮在播放中执行一次相同 SurfaceFlinger `--latency` 采样，用 Surface 请求时间精确关联原始 trace。该环形记录约覆盖 2 秒，表中围栏百分位只代表该短窗口，不能冒充整个 35 秒或物理面板测量。

| 轮次 | 会话 | 围栏关联帧 | 请求→合成器 P50/P95 ms | 合成器−音频目标 P50/P95 ms | MediaCodec−合成器 P50 ms |
|---|---|---:|---:|---:|---:|
| A2 | 1791777558097392400-1-0 | 126 | 9.082 / 10.509 | +9.082 / +10.509 | 33.670 |
| B2 | 1791777602425698500-1-0 | 119 / 126 有效围栏 | 0.826 / 0.910 | −6.970 / +8.238 | 33.685 |
| B3 | 1791777646070327400-1-0 | 126 | 0.836 / 0.936 | +0.464 / +1.439 | 33.692 |
| A3 | 1791777702759355100-1-0 | 126 | 6.673 / 7.999 | +6.673 / +7.999 | 33.696 |
| 最终正常桌面 | 1791777791825398700-1-0 | 126 | 0.824 / 0.936 | +5.549 / +6.802 | 33.704 |

B2 的同步偏差绝对值 P95 为 8.272ms；四组窗口显示相位不同导致有符号偏差不同，符合就近刷新槽量化。最终桌面窗口最大偏差 7.193ms。未增加音频缓冲或解码帧队列。

全会话原始 MediaCodec−音频目标 P50/P95：A2 42.760/49.890ms，B2 33.153/41.346ms，B3 34.019/41.115ms，A3 41.050/49.670ms。音频积压 P95 四组均约 34.917ms。有效回调长帧比例分别为 0.76%、0.55%、0.55%、0.33%，不证明帧间隔得到整体改善；最终实际桌面为 1.27%，仍有接收/解码/呈现长帧需要后续定位。

原始采集→MediaCodec 时间 P50 在不同会话为 85.580、96.534、101.182、94.806ms，音频目标等待也随会话变化。本次证据支持减少显示请求后的等待，并不支持宣称整体端到端减少 42ms，也不支持宣称完整端到端性能已稳定改善。未用统一减常数的方法美化指标。

## 策略与通用边界

`PresentationClock` 仅依赖输出刷新率、公共 Choreographer VSYNC 相位和 Display.appVsyncOffset；不依赖 Fire TV 名称、分辨率或固定补偿毫秒值。解码输出仍立即交给已有同步路径，音频实际播放时钟继续产生唯一同步目标。将 Surface 请求时刻对齐到离该目标最近的 VSYNC，留刷新周期 5% 的小相位余量，避免浮点/围栏边界落入下个槽；不睡眠、不引入新渲染队列、不扩大缓冲。

相位量化最多半周期加 5% 余量；已经晚到的输出不能倒退时间。未知/非法刷新率、非法 app offset、过期相位回退到原始音频目标。输出模式变化与解码 flush 清掉旧相位，下一次系统 VSYNC 再建立。设备模式保持原样。测试覆盖 30/59.94/60/120/144Hz、未知模式、延迟观察、切换、flush 和不早于解码时刻。

默认客户端与正常 Rust 后端已采用此策略。有界诊断可使用 `--presentation-mode timed` 回退、`--presentation-mode vsync` 对照，需 `--fire-tv ... --diagnostics --measure 35`；`--reference-video` 仅替换视频，继续捕获真实 Windows 音频。TUI 标识参考视频，正常业务状态和性能诊断仍进入同一个 core Snapshot。

试过根据 MediaCodec 时间自动提前约 42ms 的候选 B1，独立合成器显示画面实际领先音频约 25ms，因此弃用。该候选只留历史证据，不存在于生产策略。

## 上游依据

[Moonlight Android 固定版本呈现器](https://github.com/moonlight-stream/moonlight-android/blob/b48494cb96bff23d8886c4775cc4f39a1075495d/app/src/main/java/com/limelight/binding/video/MediaCodecDecoderRenderer.java#L948) 的 balanced 路径使用 Choreographer 并扣除 app VSYNC offset；低延迟路径及时释放最新帧并丢弃被替代输出。我们借鉴公共输出相位与避免陈旧队列，但仍保留本项目音频播放时钟同步，不照搬其无音频目标的立即呈现路径。

[Android MediaCodec 官方说明](https://developer.android.com/reference/android/media/MediaCodec#releaseOutputBuffer(int,%20long)) 指出 SurfaceView 按目标时间对应的刷新时机呈现，同一刷新槽的多帧只呈现最后一帧。因此请求任意纳秒时间可能落入下一槽；额外缓冲不是解决相位偏差的必要条件。

独立围栏列定义核对 [Android 11 FrameTracker 源码](https://github.com/LineageOS/android_frameworks_native/blob/lineage-18.1/services/surfaceflinger/FrameTracker.cpp#L217)：输出顺序为 desiredPresentTime、actualPresentTime、frameReadyTime；actualPresentTime 取 present fence signal time。本机通过精确匹配第一列与实际传入 Surface 的纳秒值验证关联。

Sunshine 是主机端，Android 输出呈现主要由 Moonlight 处理。已有主机零重排、无限 GOP、一帧 VBV 与按需 IDR 策略继续保持，相关固定版本来源见 [编码对照](encoder-flicker-ab-2026-10-11.md)。

## 复现与原始证据

`local/fire-tv-check/presentation-{a2,b2,b3,a3}-report/report.md` 保存同一逐帧报告；各会话原始 JSONL 在 `local/sessions/`，`presentation-*-surface.txt` 保存独立围栏，`presentation-*-fences.json/.csv` 保存精确关联结果。

分析：`D:/miniforge3/python.exe experiments/stream/analyze-presentation.py <session.jsonl> <surface-latency.txt> <output.json>`。先用 `adb shell dumpsys SurfaceFlinger --list` 找准确的 `SurfaceView - dev.livingroom...` 图层（不要选择 BackgroundSurface），再用 `--latency '<layer>'` 获取记录。

trace 在旧 12 列后追加 `surfaceRequestNs`，原音频目标与原始平台时间不修改。离线 `frames.csv` 追加 `surface_request_ns`，继续接受历史 12 列。共享性能提示改为“平台呈现通知与同步目标偏差（需合成器核对）”，避免将厂商报告差解释成真实卡顿。

围栏是合成器边界，仍不包含 HDMI 接收/电视图像处理/液晶响应，也未用高速相机确认物理音画偏差。

## 回归验收

Rust workspace 52 个测试通过；随后参考图标识改动的 TUI 12 个测试再次通过。客户端 RTP、逐帧/时钟、音频时钟及多刷新率呈现策略检查通过；APK 构建、签名并安装成功。正常实际桌面同步播放后完成 `--verify`：真实音视频、客户端中断、主机中断、恢复、结束、启动取消全部通过，97 个健康样本、3 次确认电脑静音。结束回到 Idle；电脑读回 mute=false、原音量 0.10000002、静音 lease=null，客户端退出，串流端口释放。

仍待优化的是会话间音频目标等待差异与少量长帧；本次没有通过关闭同步掩盖这些问题。Fire TV 上公共相位策略已验证，其他机型的真实输出围栏尚未回归，安全回退和多刷新率纯逻辑测试不能代替实机覆盖。
