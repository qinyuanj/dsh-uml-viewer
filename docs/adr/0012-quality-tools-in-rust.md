# 0012 质量工具移植为 Rust

**Status**: accepted

## 背景

crapper、mutator 与 dryer 是三个独立的 Python 仓库，各自 `0.1.0`，由安装器逐项目
clone 并各建 `.venv`；mutator 运行时导入 crapper 的 `coverage`、`discover`、
`languages`、`runners`、`atomic`，却未声明这一依赖。`./uml` 入口与质量分发
（`uml_launch.py`、`uml_quality.py`、`uml_posix.py`、`uml_windows.py`）同样是 Python。
随 [ADR 0011](0011-dsh-plugin-delivery.md) 以单一发行版本交付，三种语言并存意味着
用户仍需 Python。

## 决定

- 三个工具移植为 Cargo workspace 中的 `crates/crapper`、`crates/mutator`、
  `crates/dryer`，编入同一个 `uml-viewer` 二进制作为子命令。共享的源文件发现、
  tree-sitter 语言表、覆盖率运行器与原子写放进 `crates/quality-core`；三个工具 crate
  都依赖它，彼此不相互依赖。
- 入口与质量分发一并移植进二进制；`scripts/` 下仅供开发的测试与检查可以保留 Python。
- **移植完成的判据**：在原 Python 测试与黄金样例上，`.metrics/crap.edn`、mutation
  快照与 `.metrics/dry.edn` 与 Python 版逐字节一致；CLI 参数（`-s`、`--exclude`、
  `--changed` 等）与退出码契约不变；语言集合不变（Clojure、Java、Go、TypeScript、
  Rust、Python、Lua）。
- mutator 的隔离照搬现有分平台语义（Linux user namespace + overlayfs；Windows 每个
  worker 一份副本加 Job Object；macOS 暂不支持），本次不扩展平台。
- Python 仓库不并入本仓库；移植时参照原仓库，对齐后归档为只读。
- `dsh-uml-viewer` `1.0.0` 在三个工具全部移植后才发布，不先做 Python venv 机制。

## 被否决的方案

- **以 subtree 并入 Python 源码**：移植后即删除，历史在归档仓库里依然可查。
- **三个独立可执行文件**：平台包里多出三个二进制，与"viewer 运行时是一个二进制"不符。
- **mutator crate 直接依赖 crapper crate**：dryer 也要用同一套发现与语言规则，共享
  crate 才能让三者的选择规则保持一致（ADR 0001 2026-11-01 附录）。
- **先以 Python 实现发布、再逐个替换**：要先做一套马上被丢弃的 Python 安装机制。

## 附录（2026-11-10）：原 Python 仓库不归档

- **决定**：qinyuanj/crapper、qinyuanj/mutator、qinyuanj/dryer 不归档为只读，上文
  “对齐后归档为只读”一条失效。三个仓库的 README 改为指向 dsh-uml-viewer：在 DSH 中用
  插件，DSH 之外仍可单独使用原工具。
- **理由**：dsh-uml-viewer 只能在 DSH 中运行，原工具是 DSH 之外唯一可用的形态；冻结的
  `get-uml-viewer` 也要 clone 这三个仓库。
- **维护边界**：原工具可以修 bug，新功能只进 dsh-uml-viewer；“与 Python 版逐字节一致”
  只是移植完成的判据，此后不再要求两边保持一致。
