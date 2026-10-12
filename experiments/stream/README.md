# GStreamer 1080p60 串流实验

真实 Fire TV 默认使用有限范围BT.709 SDR、SurfaceView硬件呈现、能力协商的High profile、无限GOP与一帧VBV。已完成同源编码对照、低延迟音频修复、用户“有声音，不闪”确认和异常恢复；模拟器goldfish配置保持独立。见 [编码对照](../../docs/encoder-flicker-ab-2026-10-11.md) 和 [Fire TV音频验收](../../docs/audio-mmap-recovery-2026-10-11.md)。

独立实现Windows主机与Fire TV客户端，不依赖Sunshine或Moonlight运行。音频采用WASAPI loopback、PCM传输、AAudio/AudioTrack输出，实际同步播放及电脑静音恢复已验证；物理音画偏差仍需关联声画素材测量。手柄回传和游戏库尚未实现。工程原则：**底层实现一切向着性能，GUI 一切向着直观。**

当前开发主线采用 [Rust core、状态模拟器和 TUI](../../docs/rust-core.md)。本目录的 Python/Java/C 媒体实验已被 Android TV 设备后端复用，默认入口可运行实际音视频。纯状态模拟器与设备模式分别标注；Android TV 软件解码的结果不代表 Fire TV 的音视频问题已解决。

正常桌面的显示路径诊断可在启动 TUI 前设置 `$env:LIVINGROOM_TEXTURE_VIDEO='1'`，改用 TextureView；设置为 `0` 或移除该变量恢复默认 SurfaceView。此选项用于实机对照，TextureView 实际更新可能低于 60 fps；core 仍依据实际播放反馈管理开始、恢复和电脑静音。

Fire TV正常策略为公共profile、无限GOP、一帧VBV和按需IDR恢复，保留零B帧/前瞻；模拟器仍显式Baseline/GOP15/VBV1。主机CLI保留独立编码参数；core/TUI对照必须使用有界诊断。客户端保留当前约60Hz显示模式，避免为码流分辨率改变HDMI音频路由。

默认仍需更多设备/游戏验证。`--diagnostics`记录实际编码格式、逐帧大小和关键帧，可用 `python experiments/stream/analyze-encoder.py <session.jsonl> --out <directory>`导出JSON/CSV；帧大小不能代替QP或像素质量分析。硬件AVC解码能力协商和会话校验/限流的IDR请求已实现，主机编码能力目前仅覆盖NVENC。

## 运行

本机工具链和 APK 已准备好，Fire TV 已安装 `sofaLink Stream Lab`。从项目根目录运行：

```powershell
# 使用 config.local.json 中的设备，启动桌面串流，60 秒后停止并退出客户端
./experiments/stream/run.ps1

# 持续运行，Ctrl+C 停止；遥控器 OK 隐藏统计，返回退出客户端
./experiments/stream/run.ps1 -Seconds 0

# 使用测试图，排除桌面捕获的影响
./experiments/stream/run.ps1 -TestPattern -Seconds 60

# 对照 UDP 传输
./experiments/stream/run.ps1 -Transport udp -Seconds 60

# 修改客户端后重新编译、安装、运行
./experiments/stream/run.ps1 -Build -Install -Seconds 60
```

新电脑先安装 Python 和 .NET 9 SDK，再运行 `./experiments/stream/setup.ps1`。脚本将官方 GStreamer 1.28.7 Python 包、OpenJDK 17、Android API 35 与 Build Tools 35 下载到被 Git 忽略的 `local/tools`，不修改系统 PATH。随后运行 `./experiments/stream/run.ps1 -Device 192.168.1.50 -Build -Install`，替换示例地址并在设备上完成首次 ADB 授权。可用 `-HostIp` 指定电脑网卡地址；默认按通往设备的路由自动选取。

视频端口为 5004，TCP 模式由客户端连接电脑；UDP 模式由电脑向客户端发送。统计通过 UDP 5005 回传。电脑防火墙若阻止相应入站连接，需要放行所用 Python 程序或对应端口。统计回传失败时可通过 `adb logcat -s StreamLab:I` 查看客户端日志。实验协议不提供配对和加密，使用范围为可信局域网。

脚本结束时关闭主机并强制退出本实验客户端，不操作 Sunshine 或 Moonlight。客户端请求应用运行期间的 1080p60 显示模式；退出后由 Fire OS 恢复原先显示模式。不会写入电视的永久显示配置。

## 实现与性能选择

```text
DXGI / D3D11 捕获 → videorate → D3D11 GPU 缩放与 NV12 转换
→ 最多一帧的原始图像队列 → NVENC H.264 → RTP
→ TCP（RFC 4571 长度分帧）或 UDP
→ 自写 RTP 组帧 → MediaCodec 硬件解码 → SurfaceView
```

- Python 只控制 GStreamer 和记录指标，不遍历或复制原始图像像素。捕获、转换、缩放与编码使用 D3D11 显存路径，尚未通过 GPU 性能分析工具证明完全零拷贝。
- 固定 1920×1080、60 fps、H.264 High、20 Mbps；NVENC P4、超低延迟模式、零 B 帧、零前瞻、4帧预算 VBV、120帧 GOP。静态图连续截图证实 P4 比 P1 减少细节像素变化；新的编码组合通过正常桌面不闪动观察。主机参数 `--encoder-preset p1` 和 profile/GOP/VBV 参数保留对照入口。模拟器仍为 Baseline、15帧 GOP、1帧预算 VBV，避免把实机结果外推给软件解码。
- 编码前只允许最多一帧原始图像排队，可丢旧原始帧。编码后的预测帧不能随意丢弃，客户端发生损坏或积压时等待 IDR，并清理解码器状态。
- UDP 乱序最多等待 2 ms、最多保存 8 个包；无重传、FEC 或拥塞控制。TCP 用于可靠链路对照，可能在丢包时发生队头阻塞，不能据此推断公网或所有网络上的游戏性能。
- 自写 Java 客户端直接调用 Fire TV 的 MediaCodec 硬件解码器，不通过 WebView 或通用播放器。压缩数据组帧与入队仍有内存复制；没有在 Java 层搬运解码后的原始图像。
- UI 只显示连接、尺寸、实际帧率、客户端延迟和丢帧，OK 隐藏，返回退出。完整解码器信息保留在诊断日志。

## 延迟数据的边界

`local/latest-stream.jsonl` 保存主机和回传统计。运行结束自动汇总，也可运行 `python experiments/stream/summarize.py local/latest-stream.jsonl`。汇总保留卡顿样本，仅去除启动前三秒和最后一秒；按秒记录的 p50/p95 的中位数不等于所有帧合并后的 p50/p95。

| 指标 | 范围 |
| --- | --- |
| 主机编码耗时 | 编码器输入 pad 到编码输出 pad，包含编码器内部等待，不含捕获和缩放 |
| 接收后解码耗时 | 客户端首个 RTP 包到解码输出可用，包含后续组帧和排队 |
| 接收后呈现耗时 | 客户端首个 RTP 包到 MediaCodec 的 Surface 呈现时间，包含解码，不应与上一项相加 |
| 物理显示延迟 | 需要对照电脑屏幕和电视拍摄，当前尚未测量 |

本实验不会将两个设备的未同步时钟相减，也不会把客户端时间当成输入到电视的总延迟。没有真实游戏和手柄输入测试，也没有相同条件的 Sunshine 对照，不能宣称已超越 Sunshine。

可启动双屏计时板：

```powershell
dotnet run --project experiments/stream/latency-board
```

同时运行桌面串流，用手机高速拍摄电脑和电视，在同一照片中读出计时差。空格可让计时板闪白。差值是两个显示设备之间的相对显示延迟；刷新率和拍摄帧率限制测量精度，毫秒数字不表示精确到 1 ms。

## 验证

```powershell
./experiments/stream/check-client.ps1
./experiments/stream/build-client.ps1
dotnet build experiments/stream/latency-board
```

RTP 测试覆盖单 NAL、FU-A、STAP-A、序列号回绕、丢包后 IDR 恢复、主机 SSRC 重启、损坏数据与有限乱序。真机实验记录见 [实测记录](../../docs/stream-experiment-results.md)。

## 官方参考

- [GStreamer Windows 与 Python 发行版](https://gstreamer.freedesktop.org/download/)
- [D3D11 屏幕捕获](https://gstreamer.freedesktop.org/documentation/d3d11/d3d11screencapturesrc.html)
- [NVIDIA D3D11 H.264 编码器](https://gstreamer.freedesktop.org/documentation/nvcodec/nvd3d11h264enc.html)
- [RTP H.264 打包](https://gstreamer.freedesktop.org/documentation/rtp/rtph264pay.html)
- [RTP TCP 分帧](https://gstreamer.freedesktop.org/documentation/rtp/rtpstreampay.html)
- [Android MediaCodec](https://developer.android.com/reference/android/media/MediaCodec)
- [Android 低延迟解码](https://source.android.com/docs/core/media/low-latency-media)

MediaTek 的 `vdec-lowlatency` 参数名参考 [Moonlight 的公开设备适配说明](https://github.com/moonlight-stream/moonlight-android/blob/master/app/src/main/java/com/limelight/binding/video/MediaCodecHelper.java)。没有复制其解码器实现；客户端代码为本项目实现。工具二进制、调试签名密钥、设备地址与原始日志放在 Git 忽略的本地目录，不作为发布包再分发。
