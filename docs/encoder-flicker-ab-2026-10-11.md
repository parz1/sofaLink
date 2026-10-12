# 编码闪动：同源单变量对照与候选策略

后续已定位音频MMAP退回慢路径的问题并完成Fire TV同步播放、用户“不闪”确认与异常恢复验收，无限GOP/一帧VBV已接为正常默认。见 [后续音频修复与实机验收](audio-mmap-recovery-2026-10-11.md)。以下保留当时的实验与候选阶段记录。

2026-10-11。当前NVENC配置下，周期性帧内刷新令静止内容的压缩重建结果发生变化。电脑软件解码同一码流时已经出现，不需要Fire TV、网络、SurfaceView或音频参与。只提高profile不能解决；扩大VBV减轻但未消除。只把GOP120改为无限GOP、保持一帧VBV，静止区域的时间波动接近消失。

这是对当前GPU、20Mbps、P4、1080p60与这些素材的因果证据。未记录实际QP，不能断言QP跳变；也未完成所有内容、设备和物理电视的验收。

## 方法与证据

`encoder-ab.py`：不可变输入 → 原生GStreamer/D3D11 NV12 → NVENC → 码流文件；停止编码后才离线软件解码、分析像素。NumPy不在生产路径，不计入编码时间。每组360帧、60fps，排除前120帧，分析后240帧。固定P4/ULL/CBR/20Mbps、零B帧/前瞻、BT.709 SDR、一帧原始队列。每组严格校验360帧编码和解码。

同一参考图生成平坦区域掩码，各组共享，不根据解码结果挑选区域。“平均波动”为每个像素240帧内max-min的RGB通道均值，单位为8位色值；不表示绝对色准、PSNR或物理亮度。

原图只抓取一次，SHA256 `ccde6c97e8b9fb3af7c5c7d942797f379817fe17a4af7dc0c4c06ca3365b8436`。原始目录 `local/encoder-ab/desktop-matrix`、`desktop-extra`、`moving-regression`。每组保留码流、逐帧字节数/PTS/关键帧/耗时、像素变化JSON；Git忽略原始大文件。

## 静态八组矩阵

相同profile/GOP/VBV的邻组构成单变量对照。VBV1=333kbit，VBV4=1333kbit，是编码码率控制容量，不等于增加四帧原始视频排队。掩码包含1,105,221个像素。

| Profile | GOP | VBV帧预算 | 平坦区域平均波动 | 波动P99 | 编码P95 ms |
|---|---:|---:|---:|---:|---:|
| Baseline | 15 | 1 | 1.684194 | 10 | 6.409 |
| Baseline | 15 | 4 | 0.399434 | 5 | 6.415 |
| Baseline | 120 | 1 | 2.672740 | 10 | 6.658 |
| Baseline | 120 | 4 | 0.648934 | 6 | 5.763 |
| High | 15 | 1 | 3.043374 | 11 | 6.839 |
| High | 15 | 4 | 0.431018 | 4 | 11.607 |
| High | 120 | 1 | 3.489688 | 12 | 6.320 |
| High | 120 | 4 | 0.623603 | 5 | 6.463 |

旧Baseline/GOP15/VBV1在关键帧转换时，全图相邻帧平均变化6.504，普通帧0.678，约9.6倍；60fps下每0.25秒重复。High/GOP120/VBV1的关键帧变化6.735，普通帧仅0.000076。拉长GOP减少发生频率，但刷新瞬间仍跳变；不能把长期max-min当成普通帧噪声。

额外单变量：High/GOP120/VBV1 → High/无限GOP/VBV1，平均波动 **3.489688 → 0.000001206**，P99=0、最大2色值；编码P95 **6.320 → 6.723ms**。预热后没有周期性关键帧。

另只改为Sunshine使用的quarter-resolution multipass：High/GOP120/VBV1波动仍3.268440，编码P95 7.488ms。当前证据不支持用更多编码计算解决周期闪动，候选保持P4/single-pass。

## 同一段运动素材复验

`make-ab-clip.py`将相同桌面与固定运动小窗原生合成360帧RGB片段，只生成一次。三组重播同文件，SHA256 `e55af72047d1a04222a0ed5bba7ea3c0b43b06c53a9b6c19e28f8ede9f26809e`。运动窗及边缘排除，剩956,776个静止像素；全图变化含正常运动，不用来评价闪动。

| 配置 | 静止背景平均波动 | 波动P99 | 编码P95 ms |
|---|---:|---:|---:|
| Baseline/GOP15/VBV1 | 2.158408 | 12 | 5.895 |
| High/GOP120/VBV1 | 1.987391 | 9 | 6.045 |
| High/无限GOP/VBV1 | 0.000055 | 0 | 6.623 |

后一对只改GOP，背景时间波动减少约99.997%。GOP15组关键帧处背景变化1.366、普通帧0.111，约12.3倍。无限GOP最大残余3色值，并非所有像素数学上完全相同。此段不能代替实际游戏、淡入淡出和场景切换的长期质量验收。

## Sunshine / Moonlight 对照

检查Sunshine固定提交 `88428ca4eedbc0392516061c2a0978f45a1f4b95`。

| 项目 | Sunshine NVENC路径 | 我们的候选 |
|---|---|---|
| 编码节奏 | 无限GOP、零B帧、关闭前瞻、CBR/ULL | 同样原则，GStreamer gop-size=-1 |
| VBV | 硬件支持时码率/帧率计算一帧，默认增幅0 | 一帧预算，20Mbps/60=333kbit |
| 恢复 | 客户端请求IDR，主机强制刷新 | 源IP/会话/序号校验，去重、500ms限流，force-key-unit |
| Profile | 普通AVC High，另有444模式 | 查询客户端能力，选择公共High或Baseline |
| 编码代价 | 默认P1、quarter multipass，另有AQ等选项 | 此轮固定P4/single-pass，未盲目跟随默认 |
| 刷新突发 | lowDelayKeyFrameScale；按客户端和硬件能力启用intra refresh | 当前元素未暴露这两项，未实现渐进刷新 |

源码：[编码初始化](https://github.com/LizardByte/Sunshine/blob/88428ca4eedbc0392516061c2a0978f45a1f4b95/src/nvenc/nvenc_base.cpp)、[默认配置](https://github.com/LizardByte/Sunshine/blob/88428ca4eedbc0392516061c2a0978f45a1f4b95/src/nvenc/nvenc_config.h)、[IDR控制消息](https://github.com/LizardByte/Sunshine/blob/88428ca4eedbc0392516061c2a0978f45a1f4b95/src/stream.cpp)、[Moonlight参考帧损坏后的恢复](https://github.com/moonlight-stream/moonlight-common-c/blob/master/src/VideoDepacketizer.c)。[NVIDIA指南](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.1/nvenc-video-encoder-api-prog-guide/index.html)同样建议把低延迟VBV和刷新策略共同设计。

这说明其路线适合稳定的低延迟串流，不能倒推用户安装的Sunshine没有其他卡顿/颜色问题。我们尚未拥有其完整传输、纠错、参考帧管理与设备适配能力。

## 通用实现与验收边界

Rust `core::EncodingPolicy`是纯能力策略：输入编码/解码能力、码率/帧率，要求按需IDR能力，选公共profile和一帧预算，不按电视型号写死。主机目前仅NVENC，编码能力仍由后端声明；AMD/QSV需另做实际探测与验证。

客户端只读Provider受android.permission.DUMP保护，报告硬件AVC profile/level/尺寸/帧率。Fire TV实际报告OMX.MTK.VIDEO.DECODER.AVC支持High/Baseline 1080p60，后端传递选定解码器名。丢参考帧/队列溢出后请求IDR，恢复IDR到达时丢弃旧排队帧并一次flush旧解码输出；没有增大生产队列。

原生本机 `check-idr.py`通过：初始关键帧1，合法请求产生58/106号关键帧；旧会话、非法序号、重放、限流内请求不产生额外刷新。Java RTP恢复/时钟检查、Rust测试、格式、Clippy、依赖边界及7个状态模拟场景通过。它证明控制/编码链路，不表示物理电视损坏恢复已验收。

**尚未提升为正常默认。** 正常Fire TV仍High/GOP120/VBV4；候选只通过有界诊断 `--encoder-gop -1 --encoder-vbv 1`启用。模拟器保留此前独立验证的Baseline/GOP15/VBV1。

真实电视完整音画复测出现另一阻碍：AAudio实际mode=NONE，播放头对应积压约336ms，视频同步等待约310ms，解码输出积压与IDR请求循环，呈现约15–28fps。旧High/GOP120/VBV4也复现约24fps；关闭同步的候选对照约55.5fps、无排队丢帧。因此不能把问题归因于无限GOP，也不能把关闭同步当产品修复。应用音频ring约32ms，设备播放积压是不同指标，HAL/路由根因待定位。没有改变电视全局音频设置或堆队列隐藏问题。

会话已结束，core回到Idle，电脑mute=false、volume=0.2、lease=null。完整音画性能、实际游戏、物理闪动与真机异常恢复仍待验收。

## 复现

需要配置GStreamer的Python；离线NumPy/Pillow目前在local/tools/encoder-analysis。每次使用新目录，不覆盖结果；RGB片段约2.24GB。

```powershell
D:/miniforge3/python.exe experiments/stream/encoder-ab.py local/encoder-ab/desktop.png --out local/encoder-ab/repeat-matrix
D:/miniforge3/python.exe experiments/stream/encoder-ab.py local/encoder-ab/desktop.png --extra-only --out local/encoder-ab/repeat-extra
D:/miniforge3/python.exe experiments/stream/make-ab-clip.py local/encoder-ab/desktop.png local/encoder-ab/repeat-moving.rgb
D:/miniforge3/python.exe experiments/stream/encoder-ab.py local/encoder-ab/desktop.png --dynamic-clip local/encoder-ab/moving.rgb --out local/encoder-ab/repeat-motion
D:/miniforge3/python.exe experiments/stream/check-idr.py
cargo run --locked -- --fire-tv <IPv4> --diagnostics --measure 35 --encoder-gop -1 --encoder-vbv 1
```

`--measure`即使中途Degraded也会完成有界采样/清理；退出码0不代表性能通过，必须读取真实状态与逐帧数据。
