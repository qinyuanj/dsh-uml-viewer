# 0007 停止 Windows 原生支持：WSL 作为唯一交付平台

**Status**: accepted；"停止 Windows 原生支持"部分被 [ADR 0010](0010-native-windows-delivery-platform.md) 取代

增补（2026-10-27，维护者决定）：macOS（Apple Silicon 与 Intel）恢复为交付平台，与
WSL2 + Ubuntu 24.04 + WSLg 并列；下文「macOS 当前不承诺」一句由本段取代，其余条款不变。
平台差异仍只出现在 adapter 层，按编译目标一次选定：

| 能力 | macOS 实现 |
|---|---|
| 交付平台判定 | `platform_support()` 与 `ir` 接受 `target_os = "macos"`；安装器 `host()` 接受 `Darwin` |
| 进程创建身份 | `proc_pidinfo(PROC_PIDTBSDINFO)` 的启动时刻，按微秒十进制文本记录（Python `uml_posix.proc_identity`、Rust companion 与 viewer 同一口径）；zombie 视为已退出。仍是不透明文本、精确相等比较 |
| 进程环境 | 从 `std::env::vars_os` 构造显式 `envp`（没有 `/proc/self/environ`） |
| libc 常量 | `CLOCK_MONOTONIC`、`O_CREAT`、`O_TRUNC`、`POSIX_SPAWN_SETSID` 取 Darwin 头文件的值 |
| 渲染 | wgpu 原生 Metal；不设 WSLg 的 `GALLIUM_DRIVER` / `WGPU_BACKEND` 默认值 |
| 窗口 | AppKit 自带标题栏（`WindowMode::NativeDecorated`）；无 X11 / Wayland 探测 |
| 终端 attach | `osascript` 打开 Terminal.app 执行 attach；失败时照旧只报告 attach 命令 |

Linux 的 `/proc` 实现与 WSLg 路径不变。`bench` 的 CPU 与负载读数依赖 `/proc`，性能协议仍只在
WSLg 上测量。

规格 #106。本 ADR 取代 ADR 0005 中"保留 PowerShell 7 wrapper"的部分；ADR 0002 的
恢复行为、ADR 0003/0004 的 Herdr 版本与可执行文件选择规则继续有效，只把其中的
平台专用描述按本 ADR 解释。ADR 0006 的 Lua 表格式决定不在此实施：本轮仍读写 EDN。

## 决定

唯一交付平台为 **WSL2 + Ubuntu 24.04 LTS + WSLg**。不支持 Windows 原生运行，
也不单独交付或验收普通 Linux 发行版；Linux 执行路径由 WSL 承担验证。
macOS（Apple Silicon 与 Intel）当前不承诺：2026-10-21 维护者收窄 #106 范围，
原 macOS 子票 #115–#119 以 not planned 关闭，恢复时从这些票重开并另行决策。

理由：Windows 原生实现把进程管理、路径、文件锁与安装入口绑在 Win32 与 PowerShell 上，
维护成本集中在平台差异而不是产品能力；开发与运行统一到 WSL 后，只需维护一套 POSIX 适配层。LOVE 11.5 / LuaJIT、cimgui-love 1.90-1、Herdr 与三个
Python 质量工具都可复用，不需要重写核心。

## 平台边界

平台差异只允许出现在 adapter 层，且**一次选择、长期固定**。spec #106 之后只剩
POSIX 实现，facade 直接转发，不再按宿主分支：

| 能力 | 选择点 | POSIX 实现（唯一） | 已退役 |
|---|---|---|---|
| 交付平台判定 | `adapters/platform` | LOVE 报告 `Linux`（WSL）即受支持；其余宿主返回拒绝原因，companion preflight 与 `ir` 只消费该结果 | 各调用点自行比较宿主名 |
| 路径身份与目录枚举 | `adapters/platform_path` | `posix_path`（`realpath`） | `windows_path` |
| 文件 mtime | `adapters/platform_watch` | `posix_watch`（`statx` 纳秒） | `file_watch` |
| 进程边界 | `adapters/process` | `posix_process`（`posix_spawn`） | `windows_process` |
| OS 锁 | `adapters/lock` | `posix_lock`（`flock`） | `os_lock` |
| DPI | `adapters/dpi` | 合成器与 LOVE 帧缓冲缩放 | `user32` 查询 |
| 原子替换 | `adapters/atomic_file` | `rename` | `pwsh` `File.Move` |
| 终端 attach | `adapters/companion_lifecycle` | WSL 上经 interop 用 `wt.exe` 打开新窗口运行 `wsl.exe … herdr session attach`；无 `WSL_DISTRO_NAME`、找不到 `wt.exe` 或打开失败时只报告 attach 命令 | 原生 Windows 的 Windows Terminal 分支 |

调用方（domain/application）不感知平台。进程创建身份对调用方是**不透明文本**，
只做精确相等比较：POSIX 用 `/proc/<pid>/stat` 的 starttime。
禁止引入时间窗口容差，也禁止把已退役平台的表示带进 POSIX 契约。

## POSIX 进程契约

- `posix_spawn` 传字面 argv，永不经过 shell，因此空格、Unicode 与 shell 元字符不会被重解析。
- 每次 spawn 都显式传环境：`envp = NULL` 在 glibc 下不是"继承"，子进程会得到空环境。
- 已退出但未被回收的僵尸不算存活；detached 子进程没有 waiter，这是设计而非泄漏。
- 超时只终止本次调用自己派生的子进程。
- Herdr 的 Unix socket 路径受 `sockaddr_un` 限制。对照 Herdr 0.9.5 实测：101
  字节可正常建 workspace，102 字节起 `workspace create` 失败且只报 closed-channel。
  生成的 session 名按 `XDG_CONFIG_HOME` 实算长度收缩；根本装不下时在启动前以
  真实原因拒绝，而不是让 server 报不透明错误。

## 安装与入口

公共入口是 POSIX shell + Python 3.11+，不要求 PowerShell，也不调用 Windows 可执行文件。
安装幂等，保留手写 policy、提案与有价值的用户状态；已有 ownership 或恢复记录时拒绝安装。
`./uml ir` 无窗口运行，输出字节稳定（同一 policy 多次运行结果一致）。

## 影响

- 既有 Windows 专用测试断言改为**声明平台**的 fixture，而不是依据宿主系统推断；
  行为契约不变时保留，被新平台取代时删除。
- 历史上 JVM 时代的 macOS 验收不作为当前证据。macOS 不写"待验收"，而是明确
  **当前不承诺**；源码中已无交付目标的 Darwin 专用分支随 #114 退役。
- Windows 本地仓库、安装副本与专用环境的最终清理需逐项清单与单独确认，
  不属于本 ADR 的自动结果。

Addendum (2026-10-22, #144 under [ADR 0008](0008-rust-egui-viewer-drop-lua.md)): the
runtime behind every POSIX contract in this ADR is now the single Rust binary.
The LOVE-specific wording in the capability table is historical; the literal-argv
spawn, explicit envp, `flock`, `rename`, `statx`, `/proc` starttime identity and
Herdr socket-length rules are unchanged, and the platform check is implemented in
Rust. LOVE/Lua and cimgui are no longer installed or required.

Addendum (2026-11-07, #201 / UV-5 under ADR 0010): native Windows is a delivery
platform from Windows 10 22H2 (build 19045), alongside WSL2/WSLg and macOS. The
historical “Windows native refused” entries above are superseded. The companion
chooses its Windows boundary at compile time: FILETIME identity, LockFileEx,
CreateProcessW and explicit environments. Native Herdr 0.9.5 uses an AF_UNIX socket
file under XDG_CONFIG_HOME, else APPDATA; the measured usable path budget is 243
bytes on build 26220 (244 fails at workspace create), while POSIX keeps 101.
PowerShell 5.1 remains Herdr's pane shell, with a literal runner submission and
Console.Write terminal reset; no PowerShell entry script is reintroduced.
Windows Terminal receives XDG_CONFIG_HOME and HERDR_CONFIG_PATH on its command
line because a new tab uses the terminal's own environment. Missing/failed attach
still only reports the command. The ownership, FIFO, identity verification and
1/2/4-second, at-most-three-retry policy remain common to all platforms.
