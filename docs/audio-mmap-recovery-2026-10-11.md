# Fire TV 音频积压定位、修复与无限 GOP 验收

2026-10-11。此前约336ms并非主机音频发送队列，也不是单纯把PCM ring设大了：低延迟MMAP路径无法打开后，AAudio退回普通模式；普通HDMI混音路径自身约210ms，再叠加框架与应用排队。应用仍按这个真实播放头同步视频，进而导致视频输出积压与IDR恢复循环。不能通过关闭同步掩盖。

## 定位证据

相同咖啡厅环境声实际在Windows播放/捕获，编码固定High/无限GOP/一帧VBV，音画同步开启，先对照输出后端。原始记录在 `local/fire-tv-check/audio-path-*.log`、`*-flinger.txt`、`*-logcat.txt` 与 `audio-service-stale.txt`。

| 输出路径 | 实际表现 | 结论 |
|---|---|---|
| 原AAudio共享 | mode=NONE，积压约308–336ms，视频约15fps/启动可能超时 | 请求低延迟不等于取得低延迟 |
| AudioTrack小缓冲/低延迟请求 | 实际仍普通模式，积压约230–238ms，视频约55–57fps | 去掉AAudio退回路径的额外代价，但未解决慢HDMI输出 |
| AAudio独占请求 | MMAP打开失败，仍退回普通模式 | 仅改共享模式无效 |
| 指定当前HDMI设备 | 仍MMAP失败、积压约345ms | 仅改设备ID无效 |
| HW_AV_SYNC直通诊断 | 未获得音频播放进度，启动失败 | 实验代码已移除，不能当作修复 |
| 重启清理旧端点＋修复客户端 | 真正MMAP、LOW_LATENCY、192帧burst、384帧输出缓冲 | 约34ms积压，约59.94fps，保持同步 |
| 干净端点下再用原共享AAudio | LOW_LATENCY，约42ms积压、59.94fps | 共享AAudio本身不是336ms主因；独占MMAP在本机再少约8ms |

AudioPolicy/AudioFlinger明确记录低延迟请求与普通输出flags不匹配；普通混音路径延迟约210ms。AAudio服务中旧HDMI设备的MMAP端点一直保留，新设备MMAP打开失败。客户端当时每次启动都请求1080p60，而Fire TV当前为4K59.94；启动后反复出现HDMI → Speaker → HDMI路由切换和AAudio断开。指定新设备也不能清理旧系统端点。

用户授权后重启Fire TV；修复客户端不再改变已有约60Hz的显示模式。清理后取得新MMAP端点，随后多次开始/结束与进程异常恢复仍保持低延迟。这支持“HDMI切换使旧端点失效、继而退回慢路径”的根因链；没有追踪厂商HAL内部全部实现，也不能断言所有外部HDMI拔插问题都已修复。

主机音频队列每秒P95的分布P95为0.082ms，发送阻塞每秒P95的分布P95为0.108ms，排除了主机发送积压为这轮336ms的主要来源。它们不是全部音频包的全局P95。

## 修复

- 保留当前59.94/60Hz显示模式，不再为1080p码流强制切换HDMI分辨率。码流仍1080p60，由显示路径缩放。低刷新率时只考虑同分辨率的60Hz模式。
- 正常输出先请求AAudio低延迟独占模式，检查实际performance mode；系统允许共享低延迟也可使用。未取得低延迟时释放该流，回退到较小的AudioTrack，不继续把普通AAudio冒充低延迟。
- AudioTrack先尝试20ms容量，再仅在创建失败时使用平台最低容量；有效缓冲仍请求20ms。AAudio继续两burst输出缓冲与原20ms PCM ring，没有加大队列。
- 继续使用实际设备播放时间戳映射媒体PTS；没有关闭同步、伪造播放进度或更改电视全局音频配置。
- 反馈增加实际输出设备ID和共享模式；后端对照参数只允许有界Fire TV诊断采样。
- 高层状态仍由真实播放证据驱动。输出路径不可用时回退并报告实际模式；不能保证系统慢路径也满足同样延迟目标。

本次重启是清除已存在的系统残留状态，不是产品每次开播的步骤。现有系统状态无法由应用API彻底清理；程序对外部HDMI异常的兜底仍有平台能力边界。

最后的共享路径对照使用 `--audio-backend aaudio`，除输出共享模式外保留修复后的显示/同步/编码设置，日志 `local/fire-tv-check/audio-clean-shared-control.log`。它进一步区分了共享模式的额外等待与旧MMAP端点导致的严重退回。

## Sunshine / Moonlight 如何处理

Sunshine将按客户端packetDuration采集的PCM交给独立线程编码Opus，使用有界队列。Moonlight Android使用AudioTrack，先尝试小缓冲低延迟，考虑原生采样率与音效限制；待播放音频积压超过40ms时不继续写入旧数据。moonlight-common-c独立收包/解码，队列满时清理旧包，并处理启动前的积压。其方向是限制旧音频、选择可工作的输出路径，而非不断积累。

参考：[Sunshine audio.cpp，固定提交](https://github.com/LizardByte/Sunshine/blob/88428ca4eedbc0392516061c2a0978f45a1f4b95/src/audio.cpp)、[Moonlight Android输出](https://github.com/moonlight-stream/moonlight-android/blob/b48494cb96bff23d8886c4775cc4f39a1075495d/app/src/main/java/com/limelight/binding/audio/AndroidAudioRenderer.java)、[Moonlight收包与解码](https://github.com/moonlight-stream/moonlight-common-c/blob/master/src/AudioStream.c)。Android官方建议使用[低延迟模式、独占MMAP和回调](https://developer.android.com/games/sdk/oboe/low-latency-audio)，并强调请求可能未被满足。

我们当前仍是有界PCM/TCP，不是完整Opus/UDP/FEC传输；本轮证据不需要更换传输即可解决主要设备输出积压。

## Fire TV 实测验收

先用旧High/GOP120/VBV4验证干净音频，再用High/无限GOP/VBV1连续测量120秒。无限GOP会话 `1791693511984408300-1-0`，报告 `local/fire-tv-check/audio-clean-infinite-report/report.md`。

| 指标 | P50 | P95 |
|---|---:|---:|
| 实际播放头对应音频积压 | 34.396ms | 34.896ms |
| 编码提交到输出 | 6.085ms | 8.387ms |
| 客户端排队 | 0.845ms | 2.304ms |
| 解码输出等待 | 8.685ms | 13.164ms |
| 音画同步请求等待 | 52.653ms | 56.613ms |
| 采集输出到Surface估计 | 113.091ms | 120.796ms |

排除前5秒后匹配6817个呈现帧；滚动约59.94fps，完整有效呈现窗口58.43fps；>25ms间隔52/6816（0.76%）。音频mode=LOW_LATENCY，独占MMAP，同一设备端点；输出8ms、应用ring20ms，设备xruns=0、最终source starvation=0。启动/恢复累计4次音频重置、2个旧网络PCM包、51个等待IDR/队列丢帧，随后计数稳定。统计有24条非法时间记录及175个缺失呈现记录，原报告保留，不把滚动60fps说成零丢帧。

用户现场明确确认“有声音，不闪”。音画同步开启不等于物理音画偏差已精确测量；环境声也不是关联声画标记。Surface实际呈现比目标P50偏晚42.4ms，仍需后续专门测量与优化；当前时间边界不包含电视物理显示/扬声器延迟，不声称端到端65ms或完美唇音同步。

随后将能力协商的High/无限GOP/一帧VBV接入正常Fire TV入口，模拟器仍Baseline/GOP15/VBV1。正常配置 `--verify`通过：94个实际音画健康样本、3次电脑静音确认，覆盖客户端强制退出、主机终止、恢复、正常结束与启动取消，日志 `local/fire-tv-check/audio-infinite-verify.log`。无需再重启电视，各次恢复重新取得正常输出。

Rust格式/Clippy/测试/依赖边界/7个状态模拟场景、Java RTP与时钟检查、Android原生audio ring并发/静音映射检查及原生按需IDR检查通过。会话已清理，电脑恢复原mute=false、volume=0.2、lease=null；未安装虚拟输出或改变默认设备。
