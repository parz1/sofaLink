# 串流颜色诊断：模拟器偏移与 Fire TV 闪动

后续已完成同源单变量矩阵与运动素材复验：周期性关键帧刷新导致的压缩重建误差在电脑软件解码端已能复现；无限GOP/一帧VBV候选接近消除该素材的静止背景波动。完整证据、Sunshine源码对照、能力策略和尚未通过的真机音画性能见 [编码闪动对照报告](encoder-flicker-ab-2026-10-11.md)。

此前实机结果：AFTKRT 正常桌面使用 SurfaceView、High profile、GOP120、4帧预算 VBV，用户确认“不闪了”。正常入口仍保留这组参数，候选尚未提升为默认。此前单改 BT.709 SDR 或换 TextureView 均未解决正常桌面闪动。模拟器的独立 goldfish 兼容配置保持 `2:4:7:1` 与原编码参数。下文为诊断过程的历史记录。

用户观察到红蓝绿漂移后，直接采集了 Steam 电脑原图、GPU 转换截图、电脑端 H.264 解码图与 Android 模拟器截图；随后使用固定 1920×1080 RGB / 灰阶图排除动画、缩放、统计遮挡和不同截图时间造成的比较偏差。

发现的问题在当前 `c2.goldfish.h264.decoder` 解码 → Surface → 模拟器显示路径：同样的 BT.709 矩阵编码内容，电脑解码颜色接近参考值，模拟器却出现红色衰减、绿色混入红色。结果与使用 BT.601 系数解释 BT.709 YUV 高度吻合。这里没有检查模拟器内部全部实现，不能进一步断言究竟是哪一个内部模块写错了。

## 数字证据

固定色块中心采样，RGB 为 0–255：

| 色块 | 原始图 | 电脑端解码 | 原配置模拟器 | 仅矩阵兼容后的模拟器 |
|---|---|---|---|---|
| 红 | 255,0,0 | 254,0,0 | 234,0,3 | 255,0,0 |
| 绿 | 0,255,0 | 0,251,0 | 21,255,10 | 0,254,1 |
| 蓝 | 0,0,255 | 0,0,253 | 3,0,246 | 1,0,255 |
| 青 | 0,255,255 | 0,251,253 | 22,255,254 | 0,254,255 |
| 洋红 | 255,0,255 | 253,0,252 | 236,0,247 | 255,0,255 |

新增可复现检查脚本重新启动客户端和原生 GStreamer 流，测量 8 个 RGB 色块和 16 个灰阶色块的中心，共 72 个通道值。模拟器兼容配置最大通道误差 **2 / 255**；原配置负对照最大误差 **22 / 255**，检查按预期失败。这个数字只描述测试点，不能当成全帧色差或物理屏幕校准结果。

原配置并非完全丢失颜色元数据：客户端输出曾报告 `color-standard=1` / `color-range=2`，即 BT.709 / limited。只把输出 caps 写成 `bt709` 仍存在颜色错误；直接用 `bt601` 虽修好饱和色块，却把灰阶 17 抬到约 34，不能作为最终修正。

最终兼容只改矩阵，保留源图 sRGB transfer 与 BT.709 primaries：GStreamer colorimetry 为 `2:4:7:1`（limited / BT.601 matrix / sRGB transfer / BT.709 primaries）。灰阶重新接近参考值。客户端对应输出实际值为 standard 86 / range 2 / transfer 2，完整记录在诊断反馈；不在普通操作界面暴露这些内部数字。

## 实现边界

- `host.py --color-profile sdr709` 为默认 SDR 路径，明确指定 `2:3:7:1`，与之前协商出的 BT.709 matrix / sRGB transfer / BT.709 primaries 相同。
- `host.py --color-profile goldfish-compat` 是本次专用 Android TV 模拟器的校准兼容路径。`crates/emulator` 目前明确选择 goldfish 解码器，因此只在这个后端传入该配置。
- 不把模拟器兼容配置应用到真实 Fire TV。真实硬件能力、矩阵处理和显示色彩还未完成实机验收。
- 保留 D3D11 捕获 / GPU 转换 / NVENC 路径，没有在生产视频链路增加 CPU 下载、逐像素软件修色或额外帧缓冲。颜色转换仍由原来同一个 GPU 转换节点完成。
- 主机日志记录请求的 profile / colorimetry；客户端记录实际输出 color standard / range / transfer，便于以后发现协商与显示不一致。

电脑源图是 2560×1440，串流为 1920×1080。还有一类独立现象：缩放和 NV12 / H.264 4:2:0 会损失高频色度细节。本次 1 像素白红蓝条纹在电脑端解码和模拟器中都会混色；矩阵修正不会把 4:2:0 变成 4:4:4。小字彩边仍需进一步区分源图子像素文字渲染、缩放和色度采样。没有为了改善测试条纹就切换成未验证的高负担编码格式。

## 验证与复现

```powershell
# 先结束当前串流。仅使用 Livingroom_TV_API_36 / emulator-5580。
./experiments/stream/check-color.ps1
# 负对照：当前模拟器应检出错误并返回失败，保留 PNG / result.json。
./experiments/stream/check-color.ps1 -Profile sdr709

# 正常模拟器启动会自动使用已经校准的 goldfish-compat。
./start.ps1 -Diagnostics
```

检查脚本使用现有绑定启动原生 GStreamer CLI，在 5014 端口传送静态图；每次启动实际客户端、抓取 ADB PNG、检查色块后，在 finally 中结束自己的主机进程与客户端。结果位于忽略目录 `local/color-checks/<timestamp>/`。它不会安装软件或修改系统显示设置。

工作区 format / clippy / 36 项 Rust 测试 / 7 个状态模拟场景通过；新 APK 已构建、安装。颜色修正后用真实桌面 + 系统音频采样 35 秒，稳态约 **59.83fps**，Surface 帧间隔 P95 **18.097ms**，时钟缺失回退 / 设备音频欠载 / 音频重置均为 0，core 最终确认 Idle 与资源释放。该次采样期间还执行了截图与 PNG 编码，因此只作为实际功能和流畅度检查，不是严格的颜色修正性能 A/B。捕获输出 → Surface P50 133.889ms，不包含物理电视显示。

记录：`local/sessions/1791661958476860800-1-0.jsonl`，报告 `local/color-diagnostics/fixed-report/report.md`。截图包括：

- `local/color-diagnostics/host-native.png`：电脑原图。
- `local/color-diagnostics/client-before.png`：修正前真实桌面客户端截图，左上有统计遮挡。
- `local/color-diagnostics/host-fixed-native.png`、`client-fixed.png`：修正后真实桌面，截图时间与界面状态并非完全相同，不用于逐像素全帧误差计算。
- `local/color-diagnostics/reference.png`、`reference-decoded.png`、`chart-client.png`、`chart-compat-client.png`：固定颜色图的参考、电脑解码、原配置模拟器与兼容配置模拟器。
- `local/color-checks/20261010-195212/result.json`：修正后重复检查，最大误差 2。
- `local/color-checks/20261010-195218/result.json`：原配置负对照，最大误差 22。

Windows 系统时钟与当前任务日期存在偏差，因此自动目录时间为 10-10，文档以任务日期 2026-10-11 标记。

## 与 Sunshine 性能目标的关系

这说明设备适配与正确颜色处理是必须逐项验证的工程工作，不能只按语言或编码器名称判断质量。当前方案的视频像素路径同样由原生 GStreamer / GPU / NVENC 执行，Rust core 负责独立的会话状态；技术路线没有已知的语言层面理由阻止局域网 1080p60 达到同级性能。

但目前未进行同一真实 Fire TV、同一游戏、同一网络和码率的 Sunshine / Moonlight 对照，不能宣布已经不输。我们的低延迟音频实机验证、媒体 UDP 恢复、网络抖动控制与游戏手柄仍是关键未完成项。模拟器接近 60fps 不代表真实电视已经达到同样延迟和稳定性。

参考：[GStreamer colorimetry](https://gstreamer.freedesktop.org/documentation/video/video-color.html)、[D3D11 转换](https://gstreamer.freedesktop.org/documentation/d3d11/d3d11convert.html)、[Android 色彩字段](https://developer.android.com/reference/android/media/MediaFormat#KEY_COLOR_STANDARD)。

## 真实 Fire TV 的时间闪动调查

设备为 `AFTKRT`，解码器为 `OMX.MTK.VIDEO.DECODER.AVC`。本轮问题是整块背景/颜色闪动，不能与前面模拟器的静态矩阵偏差混为一谈。

同一固定图、1080p60、20Mbps、Baseline、GOP15、一帧 VBV、零 B 帧和前瞻，仅比较 NVENC P1 / P4。对 8 张截图计算各像素 RGB 通道跨截图的最大跨度，而非图像文件大小。

| 固定输入 | P1 最大通道跨度 | P4 最大通道跨度 | P1 / P4 细线区域平均跨度 |
|---|---:|---:|---:|
| PNG 直接输入 | 18 | 5 | 见对应 temporal.json |
| 桌面显示固定图，2560×1440 捕获并缩为 1080p | 27 | 8 | 1.593 / 0.227 |

编码码流在电脑上软件解码也存在变化：桌面输入 30 帧 P1 最大跨度 27，P4 为 19，细线区域平均跨度 2.050 → 0.584。截图与电脑离线帧的采样时刻不同，不能逐帧相减或混合比较。P4 因而作为减少压缩细节变化的默认预设；保留 `--encoder-preset p1` 对照。这个改善不等于消除了电视实际闪动。

用户的物理观察：P4 桌面仍闪；隐藏统计浮层仍闪；固定串流彩条的大块颜色仍闪。新增客户端本地 `colorChart` 对照，不经过捕获、网络、解码器、视频 Surface 或定时重绘；用户确认本地 RGB 图不闪。因此继续调查视频解码输出/呈现路径，不能把全部问题归因于捕获、细线量化或统计浮层。

Fire TV 数字截图红色中心为 `[233,0,3]`，电脑离线解码为 `[254,0,0]`；灰阶中心没有同等偏差。数字截图静态色块检查最大误差仍为 22，检查如实失败。MediaCodec 报告标准/范围/传递为 `1/2/2`，SurfaceFlinger 视频层为 `DEVICE` 合成、dataspace `0x10810000`、无 HDR metadata；Windows 捕获协商为 2560×1440 BGRA sRGB。没有证据证明 HDR 切换，也不能仅凭截图把模拟器补偿配置用于物理电视。试验显式设置解码色彩字段和 BT.709 视频 transfer 均未消除截图矩阵偏差，未采用该改动。

P4 的 90 秒桌面/环境声/音画同步采样：滚动呈现约 60fps，完整有效回调窗口 58.16fps；编码提交到输出 P50/P95 为 3.408/5.811ms，捕获输出到 Surface 估计 P50/P95 为 133.206/165.283ms。先前 P1 采样为 124.526/141.313ms；不同窗口与桌面内容，不能据此声称同场景性能没有回退。结束已确认 Idle、电脑 mute=false、音量 0.2、无静音租约。

诊断入口：

```powershell
./experiments/stream/check-color.ps1 -Profile sdr709 -Device 192.0.2.23 -Preset p4 -Captures 8
# 加 -DesktopCapture 验证捕获/缩放；-TextureVideo 对照 GPU View 呈现；-HoldSeconds 60 留出物理观察时间。
# 离线分析要求 Pillow/NumPy，完全不进入媒体热路径：
python experiments/stream/analyze-color.py local/color-checks/<目录>
```

静态测试先确认当前 session 持续呈现 >=120 帧，再截图；保留实际编码码流和客户端日志。已有原始目录：`20261011-031828`（PNG/P1）、`20261011-031936`（PNG/P4）、`20261011-032120`（桌面/P1）、`20261011-032200`（桌面/P4）。P4 同步报告：`local/fire-tv-check/p4-report/report.md`。本地 RGB 截图：`local/fire-tv-check/native-rgb.png`。TextureView 仍是诊断选项，不能以解码器的 Surface 回调时间代替 TextureView 合成后的物理显示时间。

TextureView 的首轮 Surface 时间戳统计约 5.7fps，但该窗口解码/回调计数约 60fps，不能将其解释为实际 View 更新速率。新增 TextureView 更新计数、最后更新时间、renderer 标识和独立 codecRendered 计数；设备健康反馈使用实际 View 更新进度。重测实际 TextureView 更新约 48fps，目前未采用为默认呈现。诊断 APK 本地 RGB 菜单键、编译与 Java RTP/逐帧时钟/音频时钟检查通过。

用户错过首轮 TextureView 后，重新持续 90 秒播放相同编码测试图，确认“这次不闪了”。解码器、码流和低延迟选项保持原样，只改变 SurfaceView 视频层到 TextureView/GPU View 呈现。物理观察因而指向解码后的呈现/视频图层合成路径，而非需要换成软件解码；仍不能证明具体厂商内部实现的缺陷。真实 TextureView 更新率约 48–51fps，与 codecRendered 约60fps 分别记录，性能目标尚未满足。原始目录 `20261011-033459`。

随后保留 SurfaceView 和同一硬件解码器，仅将编码流的传递特性改为 BT.709 SDR（`2:3:5:1`），用户对固定图确认“这次不闪了”。标准/范围/传递回读为 `1/2/3`。原始目录 `20261011-033650`。该结果仅适用于此轮固定图，后续正常桌面仍闪，无法据此确认根因或修复成功。主机 `sdr709` 改为该标准 SDR 配置，客户端 Fire TV 模式也显式声明 BT.709/limited/SDR；goldfish-compat 保持 `2:4:7:1`。矩阵截图误差仍为22，完整物理颜色精度尚未测量。

正常 core/TUI 桌面串流 60 秒复测（同步开启，实际 Windows 环境声捕获）：滚动最后反馈 59.996fps、SurfaceView、色彩 `1/2/3`、音频健康、RTP 丢包/坏帧为0。完整有效呈现窗口 58.72fps，3298 个主机匹配呈现帧，>25ms 间隔 26/3297；捕获输出到 Surface 估计 P50/P95 为118.078/134.957ms，编码提交到输出 3.425/5.263ms。该时间不包含物理电视显示，也不能单凭不同时长/内容的对照声称提速。报告 `local/fire-tv-check/sdr-fixed-report/report.md`。启动阶段出现短暂 Degraded，状态随后恢复 Playing 并确认电脑静音；60秒结束确认为 Idle/原静音恢复。另发现原8秒媒体主机等待偶尔先于原生插件冷启动结束，改为15秒上限，并仍等待真实 host_ready；core 的20秒启动超时保持原值。

最终真实 Fire TV `--verify` 通过：115 个实际音画健康样本、4 次电脑静音确认；覆盖客户端强制结束、媒体主机终止、两者的恢复、正常停止和启动取消。日志 `local/fire-tv-check/sdr-fixed-verify.log`。结束确认电脑 mute=false、volume=0.2、lease=null，PC 视频/音频端口关闭，Fire TV 的本项目客户端停止。未安装虚拟音频设备或更改默认输出。

最终 `scripts/check.ps1` 的格式、Clippy、workspace 测试、依赖边界和7个状态模拟场景通过；Java RTP/逐帧时钟/音频时钟检查通过。goldfish 模拟器用 P4 连续3张截图检查仍通过，色块最大误差2，原始目录 `20261011-034259`。由于用户错过正常桌面短时复测，随后通过交互式 TUI 重新开始实际串流，保留播放等待物理观察，再由 TUI 正常结束。

正常桌面交互会话 `1791690238050408500-1-0` 得到用户明确反馈“还有闪动”，修复未通过。通过 TUI 停止并退出后电脑静音恢复，volume=0.2、lease=null。新增 `LIVINGROOM_TEXTURE_VIDEO=1` 仅供正常 core/TUI 会话的显示路径对照，默认仍为 SurfaceView；真实更新帧率继续取 TextureView 更新计数，不能冒用解码回调帧率。新对照会话 `1791690497033859400-1-0` 保留实际桌面、Windows 环境声捕获、音画同步与状态驱动静音。

该正常桌面 TextureView 对照得到用户反馈“仍然闪动”，固定图的无闪动不能推广；尚不能把根因归为 SurfaceView。停止后原静音状态恢复。下一轮 `1791690577094818200-1-0` 使用 SurfaceView、High profile、GOP120、4帧预算的 VBV，保留20Mbps、P4、零B帧/前瞻和一帧原始队列，临时诊断入口为 `LIVINGROOM_ENCODER_QUALITY_AB=1`。这是编码质量波动的组合排查，不能由单轮推断某一个参数的因果。较长 GOP 增加损坏参考帧后的恢复等待，High profile 可能增加解码开销，必须同时观察性能。

用户对这轮正常桌面明确确认“不闪了”。随后将该组合接为 host.py 与真实 Fire TV 的默认；模拟器后端显式保留 baseline/GOP15/VBV1。移除已无意义的临时编码 A/B 环境变量，host CLI 保留各参数的独立对照入口。未切换 TextureView 或软件解码。持续窗口的滚动呈现约60fps，说明无需以约50fps的 TextureView 路径换取此次稳定。

默认配置逐帧35秒复测 `1791690694674708300-1-0`：排除前5秒预热后匹配1892个呈现帧，稳态有效呈现58.82fps，>25ms间隔13/1891（0.69%），间隔P95 16.894ms。捕获输出到Surface估计P50/P95为116.637/133.327ms；编码提交到输出5.363/8.629ms，解码输出等待8.505/13.415ms。相比此前仍闪的基线118.078/134.957ms，未见这一采样中总延迟明显上升，但时长、内容与系统负载不同，不能称为严格提速对照。此边界不包括电视物理显示或输入延迟。报告 `local/fire-tv-check/desktop-stable-report/report.md`。音频仍有设备xruns5、重置10、旧网络PCM包40，不能据此宣称音频所有性能问题已解决。结束core确认Idle，mute=false、volume=0.2、lease=null。

新默认配置实机异常恢复验证通过：96个实际音画健康样本、4次电脑静音确认，覆盖客户端强制结束、主机终止、自动恢复、正常结束与启动取消。日志 `local/fire-tv-check/desktop-stable-verify.log`。工作区格式、Clippy、Rust测试、依赖边界和7个状态模拟场景均通过。色彩图诊断脚本同步支持 profile/GOP/VBV 参数，默认按设备选择真实 Fire TV 新配置或模拟器旧配置，保留历史配置复现入口；PowerShell语法检查通过。

## 通用性与根因边界

目前最值得验证的假设是编码质量周期性起伏。旧配置在60fps下每0.25秒一次帧内刷新，VBV只有20Mbps/60约333kbit；帧内编码压力与码率控制可能使压缩误差随帧变化。但实际 QP 尚未记录，不能断言发生了 QP 跳变，也不能把 High/GOP/VBV 组合通过解释成某一参数单独修复。TextureView 对实际桌面仍闪，削弱了仅由 SurfaceView 视频层造成的解释；没有坏帧/丢包也不能排除解码器内部行为。

NVIDIA 将一帧 VBV 列为低延迟场景的推荐起点，同时推荐较长/无限GOP、intra refresh和强制IDR等恢复机制；因此一帧VBV并非通用错误。更合理的方向是把关键帧突发、编码质量和恢复策略一起设计，而不是不断扩大缓冲。

资料：[NVENC 编码指南](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.1/nvenc-video-encoder-api-prog-guide/index.html)、[GStreamer NVENC 属性](https://gstreamer.freedesktop.org/documentation/nvcodec/nvd3d11h264enc.html)。当前GStreamer元素公开了GOP/VBV等参数，但其公开属性表没有完整的intra-refresh/lowDelayKeyFrameScale控制；不能假设这些能力已接入当前后端。

已补通用诊断：`--diagnostics` 记录实际协商H.264 caps、每帧索引/PTS/压缩字节数/关键帧标记，单独使用 `encoder_frames` 事件，旧 `host_frames` 时间格式保持不变。仅取压缩缓冲元数据，240项有界队列由后台线程批量写日志；满队列丢诊断并记录，不等待媒体线程。普通运行不开启这些记录。`analyze-encoder.py` 离线导出JSON/CSV、关键帧间隔与I/P尺寸比例，同时报告缺失/重复/丢弃；这些指标不能代替视觉质量或QP测量。

下一步根因实验应固定同一段内容，分别比较只改profile、只改GOP、只改VBV与旧/新组合；关联关键帧位置和静止区域像素变化。电脑端解码同一码流也变，则问题在编码或更早；电脑稳定而客户端数字帧变，继续定位解码；两端数字帧稳定而物理画面闪，再定位显示合成/电视处理。动态游戏需用参考帧比较，不能直接把正常画面运动计为闪动。

通用产品策略仍待实现：查询主机编码与客户端profile/level/分辨率/帧率能力后协商；以码率/帧率和延迟预算计算VBV，而非按电视型号写死；丢参考帧时请求IDR，支持时评估渐进刷新；使用帧尺寸、解码/呈现时间和恢复指标验证策略。当前High/GOP120/VBV4只是这轮验证的默认起点，不能宣称所有GPU、码率、内容和电视已通过。

诊断烟雾测试：原生GPU/GStreamer/NVENC向本机UDP运行5秒，记录286个编码帧、3个关键帧，实际间隔120，协商High/BT.709；无缺失、重复、非法记录或trace丢弃。这只验证诊断工作，不证明闪动根因。

新增记录的首轮实机验证 `1791691187120504100-1-0` 未通过完整音画启动：视频正常推进，编码记录829帧/7关键帧/间隔120，无记录缺失或丢弃；Windows捕获为非零音频，但音频连接出现timed out，客户端已播放音频帧为0。core按启动超时清理并恢复电脑状态，不能把这轮记为完整音画成功。时间报告仍可分析视频，与编码记录的新事件兼容。

相同配置重试的10秒实机音画采样完成并回到Idle，旧逐帧时间报告与新的编码JSON/CSV均生成：`local/fire-tv-check/encoder-trace-retry-report`。首轮音频超时仍保留，成功重试不能抹去其存在。
