# 0014 评审后的决定（spec-203 @ 83d6076）

**Status**: accepted

2026-11-09 对 `spec-203`（tip `83d6076`）做了两次独立评审：标准评审与规格评审。
修复分支 `trev-review-fixes` 修掉了源码问题；本 ADR 记录那些**不是代码**的决定、
被接受但推迟的取舍，以及以删除功能作废的验收标准，使它们不再依赖对话记录。

## 决定

- **D16（评审 S4）**：#30 删除 egui 窗口后，#18 的第 5 条标准（“egui 窗口仍可单独
  运行”）与 #16/#17 中关于窗口的标准**失效**，不是失败——被删除的功能不再验收。
  #18 第 6 条（Linux 与 Windows 上各完成一次 crap → mutate → ir 刷新）仍未完成，
  #18 保持 open。作废标准必须写在这里，不能悄悄消失。
- **D18（评审 S8）**：质量失败门控只存在于宿主进程内存（`packages/host/src/gate.ts`），
  DSH 重启后不再记得；展示门控是按会话的，且仍要求同进程内一次成功的 `uml_ir`，
  而全新会话本来就没有质量历史需要遵守。接受“不持久化”，作为限制记录，不新增持久化接缝。
- **D19（评审 S7）**：`crates/dryer/src/original/` 中的 `#[cfg(test)]` 模块仍在 policy 与
  质量范围内（`uml-quality.json` 只排除 `tests.rs` / `*_tests.rs`）。它们是产品侧移植代码
  的验证，排除出去会让 CRAP 图对这部分说谎。不排除。
- **D20（评审 S6）**：`docs/quality-dispatch.md` 里“mutator 尚未移植 Go/Java/Clojure”
  的说法在 #14 之后已过时。文档跟随源码（每个声明的语言都会被读），同时保留诚实边界：
  这三种语言的运行期一致性仍是 **UNVERIFIED**（#14 open）。
- **只读会话拒绝面板（评审 S2）**：只读会话继续拒绝面板（`watch`）与 `uml_display`，
  因为两者都需要 `uml-viewer resident` 子进程，而只读策略不提供该能力（D3）。这比
  SPEC-203 的“拒绝四个写工具”更严格，但 resident 不写工作区（会话状态在插件 home），
  所以这是**有意的收缩**而非缺陷：不改行为，已在 `docs/plugin-development.md` 写明，
  不放松该拒绝。要改变它需先改本条。
- **D15（评审 S3）**：视图状态与讨论上下文保存在插件自己的
  `<DSH_HOME>/dsh-uml-viewer/sessions/<id>.json`，不写入 DSH 会话存储；理由、代价与
  “没有 `DSH_HOME` 就什么都不持久化”的边界见 [ADR 0013](0013-repository-migration.md)
  的同日附录。

## 接受但推迟（本轮不重构）

- **M2 进程组与捕获无上限**：`crates/quality-core/src/process.rs` 用 `process_group(0)`
  把被测进程组移出 uml-viewer 的组，`Group::drop` 只在正常返回时运行；DSH 终止
  uml-viewer（graceMs 2000 → kill）后该组可能存活。mutator 用 PDEATHSIG 隔离，crapper
  与 dryer 没有。捕获也按 `read_to_end` 无上限、无超时，这与原版 Python 一致（D6）。
  需要 Linux 运行期验证，记为后续工作。
- **M3 `Atomics.wait` 阻塞宿主线程**：`versions.ts` 的 `withStoreLock` 最多阻塞约 2 秒、
  `writeAtomic` 最多 50×30 ms。改成有界异步等待要求 `withStoreLock`/`writeAtomic`/
  `pinLocked` 及其所有 pin、drop、cleanup 调用点一并变成 async，不是“最小改动”，因此
  本轮不动，现有 versions 测试保持原样；后续若重做，应以异步锁 API 为目标。
- **M1b/c/d**：混合分隔符的 fixture 路径（`display.rs` 的 `format!("{base}/{path}")` 与
  `frame/src/source.rs` 的 `starts_with('/')`）、三处重复的 Windows verbatim 前缀剥离、
  用 `process.platform` 而不是注入的 platform：都记为后续工作，本轮不重构。
- **重复与分散**（评审的 Shotgun Surgery 一类）：新增一个 resident 动作要改 5 处
  （Rust `ACTIONS`、TS `SAVED_ACTIONS`、`parsePanelAction`、`viewer.ts` 的级联、
  协议文档）；SemVer 正则有 3 份且互不一致；`reasonOf` 与 FIFO 尾部逻辑重复；
  `state.ts` 的会话 id 归一化会碰撞（`a/b` 与 `a_b`、NTFS 大小写）。全部接受现状。
- **每会话门控映射没有清理点**：`QualityGate.forget`、`IrGate.forget`、
  `PanelPrompts.forget` 从未被调用，已删除，而不是接到一个不存在的“会话销毁”接缝上。
  这些映射只随插件进程结束（DSH 退出）消失；宿主若提供会话销毁事件，应在那里清理。
- **供应链加固**：`package-release.yml` 的 action 仍按 tag 而非 commit SHA 固定，而该
  工作流持有 `id-token: write`。这是遗留加固项，记录在
  [docs/releasing.md](../releasing.md#hardening-items-open-not-done)，本轮不重新固定。

## 被否决的方案

- **让只读会话也打开面板**：需要给 resident 一个只读能力，未被验证，并且会与 D3 的
  “只读即拒绝”语义分叉；否决，保持拒绝并写明理由。
- **把质量门控持久化**：需要新的宿主持久化接缝，而规格只要求“最近一次”质量结果，
  重启后的新会话没有历史需要遵守；否决，保留进程内存并记录（D18）。
- **把 `crates/dryer/src/original` 排除出质量范围**：会让 CRAP 图缺失移植代码的测量，
  正是“无数据不隐藏”规则要暴露的缺口；否决（D19）。
