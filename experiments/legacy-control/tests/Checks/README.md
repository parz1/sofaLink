# 适配器验证

在仓库根目录运行 `dotnet run --project experiments/legacy-control/tests/Checks`。此程序将自身作为模拟 ADB 子进程，验证正常启动、返回主页、未授权、连接失败、缺少应用和未知前台状态。不会连接真实设备。
