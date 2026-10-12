# Android TV 模拟器

用于验证客户端的安装、遥控操作、连接、音画协议与生命周期。它运行 Android TV，不模拟 Fire OS、电视芯片和 HDMI；性能验收仍在真实 Fire TV 上完成。Rust TUI 已通过设备后端控制实际音视频会话，纯状态模拟器继续独立保留。

## 本机配置

- SDK：`%LOCALAPPDATA%/Android/Sdk`，使用现有 Android Emulator 和 WHPX 加速。
- AVD：`Livingroom_TV_API_36`，官方 Android TV API 36、x86_64。
- 显示：1920×1080、60 Hz，配置 8 核、4 GB RAM、4 GB 数据分区。
- ADB：`emulator-5580`，脚本核对 AVD 名称后才安装或停止，不选择局域网电视。
- APK：`local/stream-build/streamlab.apk`，包含 ARM 和 x86_64 原生音频库。

在仓库根目录运行：

```powershell
./scripts/tv-emulator.ps1
./scripts/tv-emulator.ps1 -Action Status
./experiments/stream/build-client.ps1
./scripts/tv-emulator.ps1 -Action Install
./scripts/tv-emulator.ps1 -Action Stop

# 默认桌面捕获 + WASAPI 系统声音；Enter 开始/结束
./start.ps1
# Steam 大屏幕导航与实际键鼠回传
./start.ps1 -SteamBigPicture
# 可重复的测试图 + 测试音，使用实际编码与播放
./start.ps1 -TestSource
# 仅验证 core 状态，不执行设备 I/O
./start.ps1 -Simulate
# 自动验收：播放、中断实际客户端、恢复、停止、启动取消
cargo run --locked -p sofalink-tui -- --emulator --test-source --verify
```

Start 等待系统启动完成；首次启动可能较慢。Install 安装并打开 sofaLink Stream Lab，使用 TCP 连接 `10.0.2.2:5004`。`10.0.2.2` 是模拟器访问 Windows 主机的地址，不写入真实电视配置。未启动媒体主机时，客户端会等待音画。键盘方向键和 Enter 用于遥控操作；Android Back 可退出客户端。

启动/安装脚本在 `local/emulator-runtime.json` 保存 Python 路径和当前模拟器进程 ID。设备模式复用现有 GStreamer 主机，需要与本地 GStreamer Python 包匹配的解释器（本机为 Python 3.13）。`LIVINGROOM_PYTHON` 可覆盖解释器。系统声音捕获排除模拟器进程树，避免把客户端播放的声音再次回采；使用 GStreamer 的 [WASAPI process loopback](https://gstreamer.freedesktop.org/documentation/wasapi2/wasapi2src.html)，需要 Windows build 20348 或更新。

模拟器日志放在被 Git 忽略的 `local/tv-emulator.log` 和 `local/tv-emulator-error.log`。默认保存 AVD 快照，Stop 正常关闭。脚本没有清空数据的操作。

已确认系统启动、1920×1080/60 Hz 显示、APK 安装、返回 TV 主页及重新打开客户端。最终设备 profile 使用 `c2.goldfish.h264.decoder`：先等解码器就绪再接收，队列溢出后等待 H.264 IDR 恢复，不反复 flush 丢弃已解码帧。参考 [Android MediaCodec 的流边界要求](https://developer.android.com/reference/android/media/MediaCodec)。Fire TV 的默认硬件选择保持独立。

模拟器的音频输出缓冲约 150–180 ms，原先 4 个编码帧的队列不足以支持按音频时钟等待视频呈现。最终 profile 使用最多 16 个编码帧的有界队列并启用音频时间戳调度，反馈记录 `audioVideoSyncEnabled=true`；恢复后的持续播放也已通过。Fire TV 默认队列仍为 4，不使用模拟器配置。当前已验证调度流程，实际音画偏差和物理延迟仍需独立实测。

实际播放验收已通过：测试图/测试音及桌面/WASAPI 路径均取得持续音画播放帧数；实际终止客户端或主机触发异常，TUI 主操作重建连接并重新确认音画，结束后确认主机退出和客户端进程消失。另验证启动期间取消和第二个会话拒绝。设备指标来自 Surface 呈现回调与音频设备时间戳，不能由连接成功或累计收到字节代替；音视频同步和物理延迟仍需独立实测。

## 其他 Windows 电脑

安装官方 Android SDK Command-line Tools、Emulator 和 Platform Tools，并启用虚拟化及 WHPX。用 SDK 的 sdkmanager 安装 `system-images;android-36;android-tv;x86_64`，再用 avdmanager 创建同名 TV 设备：

```powershell
# 先配置所使用的 JDK；替换 sdkmanager/avdmanager 路径为本机位置
sdkmanager --install 'emulator' 'platform-tools' 'system-images;android-36;android-tv;x86_64'
'no' | avdmanager create avd --name Livingroom_TV_API_36 --package 'system-images;android-36;android-tv;x86_64' --device tv_1080p
```

自定义 SDK 目录可通过脚本的 `-Sdk` 参数指定。APK 构建依赖的本地工具链见 [串流实验](../experiments/stream/README.md)。

[官方模拟器启动与安装说明](https://developer.android.com/studio/run/emulator-commandline) · [官方模拟器网络说明](https://developer.android.com/studio/run/emulator-networking)
