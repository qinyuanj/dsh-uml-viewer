# 0020 测试分层、统一依赖图与 CI 门禁收束

**Status**: accepted

本 ADR 替代 [ADR 0019](0019-ci-tiered-routing.md)，在保持单仓、单一 `uml-viewer` 原生二进制（ADR 0010/0011/0013）及自举质量契约（`uml-quality.json`）的前提下，深化重构本仓的测试分层、依赖图裁剪与 CI 门禁架构。操作手册见 [docs/ci-routing.md](../ci-routing.md)。

## 背景

ADR 0019 落地后，路径路由阻断了纯文档提交对 Windows runner 的占用，但 2026-11-26 实测基线（SHA `f1ee6d9972fbab8ba52012d61780af7878314a51`）显示以下结构性瓶颈：

1. **外部 7 语言重型测试三路重复执行**：`mutator::go_java_clojure`（单次 573–885s）、`mutator::python_ts` 与 `uml-viewer::{crap_languages, crap_runner_smoke}` 在 `windows-mutate`（含 `cargo-llvm-cov`）、`windows-quality` 与 `windows-check` 三个通道中重复运行，消耗约 50 分钟/PR 的 Windows 计费时间。
2. **自举覆盖率归因**：`crates/quality-core/src/runner.rs` 按单 crate 目录调用 `cargo llvm-cov`，外部工具链测试剥离后对 `quality-core` 零退化，仅需补齐 `mutator` 5 处密闭 seam 测试即可使 `-p mutator` 覆盖率采集提速 86%。
3. **测试二进制膨胀与过度串行化**：`edn`、`ir`、`mutator` 存在 17 个独立 `[[test]]` 二进制；同时 `ci.yml` 在 `fixtures/crap/{java,clojure}` 树内直接运行 `mvn`/`clj` 导致污点与 `--test-threads=1` 串行化。
4. **Windows 编译关键路径**：`Swatinem/rust-cache@v2` 热恢复仅 0.9–2.7s、增量构建 8–17s；引入 `cargo nextest archive` 反增 110–165s 串行屏障且不覆盖 doctest/`llvm-cov`/`panel-e2e`，`sccache` 无法缓存 MSVC 链接与 C `build.rs`。
5. **路径路由与本地内循环割裂**：`.cargo/config.toml` 曾落入 `unknown`，`.metrics/**` 曾跳过 `panel-e2e`（#153）；`scripts/check.py` 缺少与 CI 同源的受影响裁剪。

## 决定

- **密闭测试与外部工具链契约彻底隔离**：`mutator` 与 `uml-viewer` 声明 `autotests = false`、`[[test]] name = "integration"`（默认参与 `cargo test`、`cargo nextest` 与 `cargo llvm-cov`）以及 `[[test]] name = "toolchain_contract"`（`required-features = ["toolchain-contract"]`）。该机制零改动兼容 `mutator::rust_suite` 的 `--ignore-default-filter` 反窄化契约，CI `windows-fast` 强制执行 `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`。
- **集成测试二进制收束与 `nextest` 资源组限流**：`edn`、`ir`、`mutator` 统一收束至 `tests/main.rs`；新增 `.config/nextest.toml` 定义 `jvm-toolchain`（`max-threads = 1`）与 `heavy-toolchain`（`max-threads = 2`），取消 `--test-threads=1` 并移除 `ci.yml` 中污染 `fixtures/` 的树内 `mvn`/`clj` 预处理步骤。
- **统一依赖图求解器（同源本地与 CI）**：`scripts/ci-changes.py` 使用标准库 `tomllib`/`json` 实时解析 `crates/*/Cargo.toml` 与 `packages/*/package.json`，维护角色感知的包级 `rdeps`（显式裁剪 `crapper`/`dryer`/`mutator` 到 `panel-e2e` 的非运行期链接边，并覆盖 `.metrics/**`、`examples/uml-viewer.policy.edn`、`.cargo/config.toml` 与 `.config/nextest.toml`）。`scripts/check.py` 复用同一求解器提供 `--affected` 与 `--toolchain-contract`，裸跑 `scripts/check.py` 保持全仓密闭 + release 构建，`.cargo/config.toml` D17 约束保留。
- **Presubmit 直接触达与 Post-submit/Nightly 兜底**：PR 仅在直接修改工具链适配器或打包升级脚本时触发对应的 `toolchain-crapper` / `toolchain-mutator` / `upgrade_e2e`；非纯文档 `main` push 与 `schedule` 全量执行。保留 `Swatinem/rust-cache@v2`（`shared-key: windows-rust`），不引入 `nextest archive` 或 `sccache`。
- **单一确定性汇总门禁与回归告警**：`ci.yml` 新增 `ci-gate`（`if: always()`）作为分支保护的唯一必检项；`post-submit-alert` 在 `main` push 或 Nightly 失败时自动创建或更新带 `bug`、`ci` 标签的回归 Issue。

## 后果

- 常规 Rust PR 的 Windows 计费分钟数下降约 68%–89%，`windows-mutate` 与 `windows-hermetic` 均在 3 分钟级完成。
- 本地 `python -I scripts/check.py --affected` 按 `rdeps` 闭包精准裁剪测试范围，纯文档变更 0s 跳过 Rust 构建。
- `scripts/test_ci_changes.py` 对 `git ls-files` 全路径执行零 `unknown` 断言与 `rdeps` 单调性守卫，任何新增未归类路径立即触发全量兜底并在快速守卫中报错。
