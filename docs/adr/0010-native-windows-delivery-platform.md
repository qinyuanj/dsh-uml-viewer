# 0010 原生 Windows 恢复为交付平台

**Status**: accepted

本 ADR 取代 [ADR 0007](0007-posix-platforms-drop-windows-native.md) 中"不支持 Windows
原生运行"的决定；ADR 0007 的 adapter 边界、POSIX 进程契约与 WSL/macOS 条款继续有效。
ADR 0003/0004 的 Herdr 版本下限与可执行文件选择规则适用于 Windows。

## 背景

有三类需求：在 Windows 机器上开发跨平台项目（WSL 已满足）；被测项目本身是 Windows
专属代码（Rust MSVC / `cfg(windows)`、调用 Win32 API 的 Python），测试只能在原生
Windows 进程中通过；部分开发者的 WSL 被策略禁用。后两类无法由 WSL 承担。

## 决定

- **交付平台**增加原生 Windows 11 与 Windows 10 22H2（build 19045）及以上，与
  WSL2 + Ubuntu 24.04 + WSLg、macOS 并列；更低版本在启动前以真实原因拒绝。
- 范围是整套：crapper、mutator、dryer 与 uml-viewer（viewer、IR、四个扫描器、companion）。
- 平台差异仍只在 adapter 层，按编译目标一次选定：

| 能力 | Windows 实现 |
|---|---|
| 进程创建身份 | `GetProcessTimes` 创建时刻，不透明文本，精确相等比较 |
| 进程边界 | `CreateProcessW` 字面命令行（按 MSVCRT 规则转义），显式环境块；Job Object 管理进程树 |
| OS 锁 | `LockFileEx` |
| 原子替换 | `MoveFileExW(MOVEFILE_REPLACE_EXISTING)` |
| 渲染 / 窗口 | wgpu 原生 DX12，系统标题栏 |
| companion | 原生 Herdr；Windows Terminal（`wt.exe`）打开 attach，失败时只报告 attach 命令 |

- **mutator 隔离**：Windows 没有 user namespace 与 bind mount。每个 worker 使用项目
  副本，进程树受 Job Object 管控，`HOME`/`USERPROFILE`/`TEMP`/`APPDATA` 重定向到 worker
  目录。隔离靠约定而非 OS 强制，**弱于 Linux**，文档如实说明。副本在 ReFS（Dev Drive）上
  用块克隆，在 NTFS 上整份复制；两者功能一致，只差速度。
- **入口**：安装与命令分派本来就由 Python 实现，`uml` 与 `get-uml-viewer` 只是 POSIX shell
  引导。Windows 增加同职责的 `uml.cmd` 与 `get-uml-viewer.cmd`，调用同一 Python 实现；
  该实现中 `/proc`、`posix_spawn` 等宿主差异按平台选定。不重新引入 PowerShell。
- **快照与 IR 跨平台字节一致**：路径分隔符 `/`、换行 LF、不含盘符与源码根，使混合平台
  团队的 overlay join 与提交保持稳定。

## 被否决的选项

- 只做 best-effort 移植：Windows 专属项目与禁用 WSL 的开发者需要可依赖的承诺。
- mutator 用 AppContainer 强制隔离：会让依赖网络、注册表或命名管道的测试失败；留作可选加固。
- Windows Sandbox / Windows 容器：Sandbox 同时只能一个实例，无法并行 worker；容器依赖过重。

## 验收

两种 Windows 用同一套功能标准，不比较耗时。Win11 证据来自维护者本机（10.0.26220）；
Win10 22H2 的实机验收另开 ticket，在其余阶段完成后进行。不设 GitHub Windows CI。
