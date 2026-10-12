# sofaLink Windows 主机服务

`./host.ps1` 运行当前用户会话内的 Rust 主机服务，首次向电视显示配对码。电视 Command 进入共享 core Runtime；工作线程复用现有 Windows/Android 媒体适配器，状态由 Snapshot 返回。

```powershell
cargo run --locked -p sofalink-host -- --root <项目根目录> --python <GStreamer-Python路径>
```

默认局域网 TCP 47990；--diagnostics 保存逐帧记录，--run-seconds 70 用于有界验收，--reset-pairing 显式重新配对。Ctrl+C 通过 core 结束会话并恢复电脑状态。自启动与安装程序待实现。

协议、状态、验收与限制见 [电视控制闭环](../../docs/tv-host-control-2026-10-12.md)。
