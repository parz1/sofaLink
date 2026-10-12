# sofalink-emulator

Windows Android TV 设备后端，实现 core 的 Backend。只操作 `emulator-5580` 上的 `Livingroom_TV_API_36`，不读取真实电视配置。

worker 负责 ADB、媒体主机子进程、启动、恢复和清理；UI 线程只提交 effects 并非阻塞读取 events。媒体反馈采用一个最新值槽位，避免排队旧反馈；控制结果单独传递。会话使用实例 nonce + core session/generation，同时检查主机和客户端的 session ID、反馈 sequence、指标合法性与接收时间。Windows 独占文件句柄防止同时运行两个设备会话。

HostReady 来自主机已进入 GStreamer PLAYING 的报告，ClientReady 需要 Android 启动结果与进程确认。它们不能证明播放；core 仍要求连续 Surface 呈现和音频设备时间戳播放帧数推进。清理确认包含所拥有的主机进程退出和客户端 pid 消失，失败时保留资源与重试入口。取消先使旧 token 失效，再由 worker 清理已启动的资源。

目前复用 `experiments/stream/host.py` 控制原生 GStreamer。捕获、转换、NVENC 编码和客户端解码仍使用原生路径；这不是 Rust 媒体引擎迁移完成。主机为 TCP RTP 视频与 PCM 音频，Android 客户端使用 MediaCodec 和 AAudio。模拟器显式使用 `c2.goldfish.h264.decoder`；真实 Fire TV 默认硬件选择不变。

模拟器 profile 使用音频时间戳调度视频，并将编码帧队列限制为 16，以容纳虚拟音频输出缓冲；真实 Fire TV 默认队列为 4。持续音画输出、恢复后的调度与控制流程已验证，实际音画偏差及物理延迟仍需单独验收。

配置来自 `local/emulator-runtime.json`：安装/启动脚本记录当前 Python 路径与模拟器进程 ID。`LIVINGROOM_PYTHON` 可覆盖 Python 解释器。桌面模式排除模拟器进程树的声音，防止回采；测试模式使用测试图与测试音。日志在 `local/sessions/`，均被 Git 忽略。

设备时间预算：启动 20 秒、反馈 2 秒、恢复 20 秒、清理 8 秒，由 core 执行。硬件性能、持续播放、同步与物理延迟仍需实机验收。
