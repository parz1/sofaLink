# Monorepo 布局与依赖边界

当前采用一个 Cargo workspace、一个 Cargo.lock。项目只在 Windows 开发和验证。根目录负责统一入口、依赖版本和检查，不放业务实现。

```text
apps/
  tui/                     终端应用与呈现
  host/                    Windows 用户会话主机服务与局域网控制入口
crates/
  core/                    共享契约、状态机、Runtime 与 Backend 接口
  simulator/               模拟后端、场景与无界面验收
  emulator/                Android TV 设备后端、进程与反馈桥接
  input/                   输入回传协议、断线释放与 Windows 注入
experiments/
  legacy-control/          旧 C# 原型及其测试、solution
  stream/                  Python/Java/C 实机媒体实验
docs/                      架构、契约、路线和实验记录
scripts/
  check.ps1                依赖方向、格式、Clippy、测试和场景验收
Cargo.toml                 workspace 与共享配置
Cargo.lock                 锁定依赖
start.ps1                  默认 Rust TUI 入口
```

目录归属取决于责任：apps 放可交互产品入口；crates 放可复用 Rust 能力；experiments 放独立工具链的实验；每个模块的测试与模块一起保存。默认 Cargo member 为 apps/tui，完整检查必须使用 --workspace。

## 依赖方向

```text
apps/tui ───────→ crates/core
apps/host ──────→ crates/core / crates/emulator / crates/input
     └─────────→ crates/simulator ─────→ crates/core
     └─────────→ crates/emulator ──────→ crates/core
                       └─────────────→ crates/input ─────→ crates/core
```

core 不反向依赖应用或后端，当前唯一外部依赖为 serde。simulator 实现 core 的 Backend，仅产生模拟证据；emulator 实现同一接口，执行实际 ADB/主机操作并转换客户端反馈。TUI 应用负责组装；view 模块仅接收 Snapshot 和显示参数，不访问 backend。状态判定与可用操作仍只有 core 一份实现。

`scripts/check.ps1` 根据 cargo metadata 校验内部依赖方向及 core 的依赖限制。增加 workspace member 时，应同时明确它的依赖策略。随后运行格式、Clippy、全部测试与模拟场景验收。检查过程没有设备 I/O。

```powershell
./start.ps1
./start.ps1 -Simulate
./scripts/check.ps1
```

## 后续扩展

未来 desktop、Fire TV 产品客户端、Rust media engine 和生成的 TypeScript contracts 分别考虑 apps/desktop、apps/fire-tv、crates/media、packages/contracts。只有实际开始实现时才创建这些模块。当前没有 pnpm workspace、桌面壳、Jellyfin adapter 或空的占位 package。

未来桌面应用通过 core 的 Command/Snapshot/Runtime 共用状态逻辑。媒体服务接入提供目录和播放资源，与桌面串流使用不同后端；不会把媒体库规则塞进 TUI 或绑定到 core 中的某个服务实现。Jellyfin 服务本身仍可作为外部部署的软件，接入代码以后再确定归属。
