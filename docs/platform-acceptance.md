# Platform acceptance — issue #5 / spec #1

Herdr is the sole companion backend **on every supported platform** (ADR 0007).
The delivery targets are WSL2 + Ubuntu 24.04 + WSLg and macOS (ADR 0007,
2026-10-27 addendum); native Windows is unsupported.

Current status:

- **WSL/Linux**: the full lifecycle runs natively and is recorded in
  [acceptance-evidence/106-wsl/](../acceptance-evidence/106-wsl/README.md)
  (Herdr 0.9.5 / protocol 22, Python 3.12). Since #144 the viewer, IR
  generation and companion roles are one Rust binary; the LOVE 11.5 / Lua
  runtime and the Lua 5.4 + cluacov analysis prerequisites in the historical
  record below are no longer used. The #144 cutover evidence is in
  [acceptance-evidence/144-rust-default-cutover/](../acceptance-evidence/144-rust-default-cutover/README.md).
- **WSL/Linux, Codex CLI companion** (spec #192): real Codex 0.166.0 with
  Herdr 0.9.5 / protocol 22 and the `codex` integration v10 passed fresh
  identity, mail FIFO, quality gating, viewer restart, same-UUID recovery,
  normal close and two-project isolation on 2026-11-04; the record and the two
  defects it fixed are in
  [acceptance-evidence/199-wsl-codex/](../acceptance-evidence/199-wsl-codex/README.md).
- **macOS, Codex CLI companion** (spec #192): real Codex 0.166.0 with
  Herdr 0.9.5 / protocol 22 and the `codex` integration v10 on macOS 27.0.1
  arm64 passed fresh identity, mail FIFO, quality gating, viewer restart,
  same-UUID recovery, normal close, two-project isolation, the wrong-occupant
  refusal, the companion's own `./uml` refusal and the Terminal.app attach on 2026-11-04; the record and the defect
  it fixed are in
  [acceptance-evidence/200-macos-codex/](../acceptance-evidence/200-macos-codex/README.md).
  It is separate from the WSL record and does not stand in for it. Homebrew
  rustup leaves `~/.cargo/bin` off PATH; put it there so `./uml crap` finds the
  `cargo-llvm-cov` crapper installs.
- **macOS, Claude Code companion**: no current acceptance; the JVM-era records
  below are historical only.
- **Windows native**: historical only. The records below describe earlier
  backends and are not evidence for the current implementation.

Automated simulations do not replace native observations. The paragraphs that
follow are the historical Windows/macOS record, kept for provenance.

Addendum (2026-10-15, spec #32): the cancellation sentence above is historical. macOS end-to-end acceptance was later executed and **passed** with Herdr 0.9.4/protocol 22 on Darwin 27.0.0 arm64; #22 was closed with the acceptance log in [acceptance-evidence/acceptance-log.md](../acceptance-evidence/acceptance-log.md). Platform terminal attach remains NOT VERIFIED and is tracked by [#49](https://github.com/qinyuanj/uml-viewer/issues/49).

The records below are **historical evidence for earlier backends and tool versions**, including native Windows psmux #11/#12 and macOS tmux. They do not establish acceptance of the current Herdr-only runtime.

## mutator Lua 接入复验 — 2026-10-10

接入版本为 `qinyuanj/mutator` 的 `lua` 分支
`b36c6f832b8d1b2db86fa06ebc91c64e70f38e2a`。安装器已跟踪该分支，
无需修改 Clojure `:mutate` 依赖。复验使用本仓库 `lua-fixture/` 的独立副本，
未改动已提交快照或已有 viewer。

- 新版扫描退出 0，识别 `calc` / `calc.util` 共 15 个 sites，公有及私有 form id 正确。
- 当前会话没有继承用户已设置的 LuaRocks PATH、`LUA_PATH`、`LUA_CPATH`。
  临时恢复这些变量后，已安装的 Busted、LuaCov、LCOV reporter 均可加载；
  真实解释器为 Lua 5.4.6。没有修改永久环境变量。
- 未修改的 `scripts/uml-command.ps1` 经 scratch `.uml-viewer/mutator/.venv`
  调用真实 mutator。覆盖率运行得到 2 successes / 0 failures，并生成 LCOV。
- 使用显式 `--test-command` 调用真实 Lua/Busted 后，差分仅执行 6 个 covered
  survivors，保留 7 个 killed，另有 2 个 uncovered；最终为
  7 killed / 6 survived / 2 uncovered / 15 sites，退出 3。没有强制全量重跑。
- viewer 的 `load-mutate` 与 Lua scanner / `apply-metrics` 实际读取并关联新快照，
  上述四个汇总断言通过。全量规格为 508 examples / 0 failures /
  2028 assertions / 1 pending（Unix shell）。原生入口检查为 4 passed /
  4 pending（当前未安装 ClojureTools 模块）。

初次复验时，默认 mutation 命令存在原生 Windows 限制：mutator 用 argv
调用裸 `busted`，本机 LuaRocks 仅提供 `busted.bat`，默认 baseline 报
WinError 2、退出 2。初次通过的测试使用项目内 bootstrap：

```lua
pcall(require, 'luarocks.loader')
require('busted.runner')({standalone = false})
os.exit(0)
```

将其保存为 `native-busted.lua` 后，显式传入
`--test-command '"C:/path/to/lua.exe" native-busted.lua --ignore-lua'`。
覆盖率仍由 crapper 正常生成，没有使用 `--no-coverage`。

随后在同一 `b36c6f8` 基线的本地 mutator 工作区修复默认 Lua plan：
直接使用选定的真实 Lua 解释器加载 `busted.runner`，传入 `--ignore-lua`，
沿用匹配版本 crapper 的 LuaRocks 模块路径设置，避免 BAT 启动及 shell re-exec。
真实回归执行生成的默认命令，含空格解释器路径和自定义 `.busted` 测试目录均通过；
测试成功退出 0、测试失败退出 1。mutator 全量测试为 83 passed / 1 skipped
（Windows 跳过 POSIX `bin/python` virtualenv 布局检查）。

修复后的未修改 viewer 入口再次执行默认差分，未传 `--test-command` 或
`--no-coverage`，覆盖率生成成功，最终仍为
7 killed / 6 survived / 2 uncovered / 15 sites、退出 3。
viewer 实际读取并关联新快照，上述四个汇总断言再次通过。
默认入口修复已在本地提交为 mutator `85ae015`，尚未推送；远端 `lua`
分支仍需显式 override。
该 Lua 流程的验证不等于全平台验收；CLI 仍会输出原生 Windows 支持警告。
另行验证真实失败 baseline 时，默认入口正确退出 2，但整次运行的快照字节保持
断言失败，输出还包含其他扫描文件的 snapshot writes。未保存字段差异，
不能据此认定失败文件自身被重写；该现象仍需独立排查。本次没有修改 engine。
失败测试只发生在独立验证副本中；恢复测试后默认差分再次得到 7/6/2/15。

本机证据保留在 `target/lua-validation-b36c6f8/fixture/`：
`.metrics/mutate/{calc,calc.util}.edn`、`target/coverage/lua/lcov.info`
及 `native-busted.lua`。这些是 gitignored 运行产物。

## Recorded results

macOS host: Darwin 27.0.0; PowerShell 7.6.6; tmux 3.7c; Temurin Java 21.0.12.1; Claude Code 2.1.291. Installed checkout `f90023c`; restart checkout `ca7fa57`. Two independent fixture directories contained spaces and two nested Clojure namespaces, with a pre-existing named `Saved` proposal.

Evidence root for this run: `/Users/billyq/.claude/jobs/c31d870c/tmp/a5/evidence`. Fixtures and their logs, metrics, policy, mailboxes, and saved view remain under the sibling `Project A With Spaces` and `Project B With Spaces` directories. This location is a local run artifact; the table and remaining procedure below are the durable record.

| Criterion | macOS | Native Windows / psmux 3.3.8 | Linux / tmux | WSL / tmux |
| --- | --- | --- | --- | --- |
| First installation, repeated installation, preserved policy/proposal, path with spaces | Passed, local fixed-ref install and A reinstall; repeat-preservation also covered by entry tests | Pending | Pending | Pending |
| Real `ir`, `crap`, differential `mutate` | Passed both projects, two source files individually; no forced mutation reruns | Passed: 新 A/B fixture 的真实 `uml.cmd ir/crap/mutate` 均 exit 0，各文件 1/1、3/3 killed；固定原版依赖与日志见本轮 #11/#12 小节。仓库级 companion/sketch 差分结果另见 issue #9 | Pending | Pending |
| Real Claude launch and `:display` consumed by viewer | Passed A and B; Claude generated IR and sent persistent mail | Passed: A/B 真实 Claude CRAP/IR/display 与 viewer 消费已自动记录；屏幕显示由用户最终人工验收确认 | Pending | Pending |
| FIFO context and regeneration mail, isolated from other project | Passed A `Saved` proposal context then regen; real Claude reported FIFO consumption; both queues empty; B identity unchanged | Passed（自动证据 + 用户人工验收）：context/refresh-crap/regen wake 均 true、按序 pop、B 身份/policy 不变；恢复后 context/regen/display 完整消费已捕获；GUI 操作及剩余行为由用户确认 | Pending | Pending |
| Claude unexpected exit restores same pane and command | Passed: verified A Claude PID 64705 received SIGTERM; restored Claude PID 66469 in session `$1`, pane `%1`; B `$0/%0`, PID 63937 unchanged | Passed 真实 Claude：受控终止14012后恢复4564，同 session `$49` / pane `%1` / server4304 / cwd / command；原 pane 输入与 context/regen 实际消费，B 不变。pane_pid 缺陷使用恢复 API OS 回退 | Pending | Pending |
| `:quit-for-restart` saves view, old JVM exits, wrapper starts new JVM, original companion remains | Passed process/mail flow: launcher 64680 exited; wrapper started 67033, Java child 67041; A `$1/%1`, Claude 66469 retained; B unchanged | Passed（用户人工验收）；本轮 harness 未捕获 saved EDN / 旧新 JVM / companion 身份连续链 | Pending | Pending |
| Mail works after recovery and restart | Passed public `request-agent!`/`request-regen!`, both wake results true, real Claude consumed context/regen and sent display; queues empty | Passed：恢复后真实 mail/display 自动捕获；restart 后 mail 由用户人工验收确认 | Pending | Pending |
| GUI pan/zoom/declutter/nonempty focus/proposal/open layer restored | Pending GUI interaction; complete disk roundtrip automated test passes in two valid view modes | Passed（用户人工验收）：两轮 namespace focus 与 Saved proposal/open layer、非默认 pan/zoom/declutter 恢复；未捕获本轮 GUI 截图/EDN | Pending | Pending |
| Terminal visible attach and fallback behavior | Pending visual confirmation and forced Ghostty→Terminal fallback; real pane starts successfully; opener failures covered by automated process seam | Passed：WT 真实调用 exit 0；排除 WT 后真实 error 2、绝对路径诊断及手动 attach 新稳定 client 已捕获；可见终端和颜色由用户人工确认 | Pending terminal strategy | Pending compatibility terminal strategy |
| GUI normal close disables recovery and cleans only owned session | Pending GUI click. Public shutdown boundary actually cleaned A while B stayed alive, then B; automatic shutdown ordering tests pass | Passed（用户人工验收）；本轮 harness 未捕获 hook 禁用→session 销毁顺序或无 respawn 时间序列 | Pending | Pending |

macOS GUI automation was refused with Apple Event error `-1743` for System Events. No permission bypass or interaction with existing viewer sessions was attempted. Consequently GUI acceptance remains pending even though real JVM, Claude, mail, and tmux behavior was exercised.

The first fixture harness used `capture_output=True`; the old launcher inherited its wrapper's pipes and kept capture waiting for the viewer's lifetime. A regression test reproduced this (6-second CLI caused a 7.44-second call); the command launcher now gives its child separate standard streams and persists background failures in the project log. The corrected captured entry test returns before the long-lived child exits. The original waiting harness later launched a second fresh A after B exited; that delayed launch was recorded and gracefully cleaned in its own project. It is not evidence of restart continuity.

Relevant local evidence files: `spec.log`, `entries-final.log`, `crap.log`, `mutate-sketch.log`, `ir.log`; `A/B-install.log` and analysis logs; `A-mail-pane.txt`, `A-before/after-recovery.txt`, `B-before/after-recovery.txt`, `A/B-after-restart.txt`, `A-restart.log`, `A-recovery-restart-mail-pane.txt`, `B-after-A-cleanup.txt`, and public shutdown logs. Fixture mailboxes show consumed queues; session EDN shows the saved default view. No screenshots or GUI state claims are inferred from process logs.

## Automated checks

- Full Clojure suite: 489 examples, 0 failures, 1985 assertions.
- Installed entry suite: 14 tests passed, including capture detachment and waiting for an old recorded process to exit.
- Restart rejects a missing recorded companion even when a fallback session exists; probe/bind failure or timeout never opens the viewer or creates a substitute.
- Document save/restart traverses real disk with nonempty namespace focus in one round and valid named proposal/open layer in the other; pan, zoom, declutter, selection, and detail are retained.
- CRAP completed using a command-line `-Sdeps` absolute `crap4clj` override. Only changed `src/uml_viewer/adapters/sketch.clj` received differential mutation: 2/2 killed, exit 0. IR regenerated last. The non-Clojure scanners and mail queue contract were unchanged.

## Review fixes and automated evidence

The final review found unsafe Claude batch argument forwarding, direct background
execution of Clojure batch wrappers, stale README startup guidance, and duplicate
macOS Terminal fallback code. Claude now accepts native `.exe` and `.ps1` entries
only on Windows; `CLAUDE_BIN` remains supported. Unsafe batch entries receive a
native Claude / Node PS1 adapter diagnostic before a companion is created.
Clojure batch entries are rejected synchronously with `clojure.exe` / ClojureTools
/ PS1 guidance, before a detached launcher is spawned. The README includes a
concrete Node adapter for the official npm package. Terminal fallback shares one
implementation with the existing behavioral tests retained.

Real PowerShell argv fixtures on macOS verify quotes, multiline arguments, empty
arguments, Unicode, trailing backslashes and literal shell characters through the
controlled companion runner (PS1 and Node native process), including child exit
status. A simulated Windows command-discovery boundary verifies Clojure batch
rejection before a process record appears. These checks do not establish native
Windows process behavior: the full native Windows procedure below remains pending.
Final review checks: 492 Clojure examples / 1992 assertions, zero failures;
15 installed-entry tests passed. CRAP completed with the temporary absolute
crap4clj root; per-file differential mutation killed 2/2 companion mutants and
17/17 sketch mutants (both exit 0). IR regenerated last. The final entry run also
exposed a fixture read-before-write-completion race; the harness now waits for
the complete recorded argv before asserting.
Local regression evidence is under `/Users/billyq/.claude/jobs/c31d870c/tmp/reviewfix`.

## 原生 Windows 入口验收 — 2026-10-06

本次环境为 Windows 11 build 26300 / ARM64、PowerShell 7.6.6、Git 2.52.0.windows.1、Claude Code 2.1.291；Windows Terminal 1.24.12741.0 已安装。验收使用固定本地 checkout `b5a4fa1480031c0159143ac9dbe065e10ded2e3c`，在独立临时项目 `Project A With Spaces` 中调用真实 PowerShell 和 `.cmd` 入口，没有使用已有 viewer 或 companion。

13 项检查通过：PS1 安装成功、三个项目入口生成、已有 policy/proposal 的哈希保持不变、重复安装保留用户 `.gitignore` 且仅有一个生成区块、安装器 CMD help、含空格项目路径的 CMD help、非法安装参数返回非零，以及 `ir`、`crap`、`mutate`、fresh start、`--restart` 在缺少 Clojure 时及时返回非零并输出安装指引。依赖失败后无 viewer 进程记录或 launch manifest。首次本地 clone 被共享文件系统的 Git ownership 检查拒绝；验收脚本仅为子进程设置两个精确 `safe.directory` 值后，在新临时项目重跑成功，没有修改全局 Git 配置。

证据目录：`C:\Users\billyq\AppData\Local\Temp\uml-win-acceptance-1ae4d63dda7b47c082edb295a635663c`，包含 `results.json`、首次及重复安装日志、CMD help/非法参数日志、五个依赖失败日志和保留的临时项目。执行脚本位于 `C:\Users\billyq\AppData\Local\Temp\uml-windows-acceptance-20261006.ps1`。这些路径是本机证据，以上结果是持久记录。

首次入口验收时 Java、Clojure CLI 和 psmux 缺失，完整验收被阻塞。用户随后明确授权安装依赖，后续结果见下节；13 项入口检查本身不代表完整平台验收通过。

## 原生 Windows 后续验收 — 2026-10-06 至 2026-10-07

经用户授权，在用户目录安装并核对官方发布校验值：Temurin JDK 21.0.12.1+1 ARM64、Clojure CLI 1.12.6.1673（ClojureTools PowerShell 模块）、psmux 3.3.8 ARM64。环境仅通过验收脚本注入，没有永久修改 PATH 或 profile。基线为 `b5a4fa1`，A/B 安装目录同步了本次工作区修复，因此结果对应带修复的 checkout。

- 修复官方 ClojureTools alias 协议：`-M:crap` 被模块当作源文件；改用 `-M:` 与 alias 两个参数。`-NoProfile` 子进程会先找到 PATH 的 PS1 shim，现显式导入官方模块并重新解析命令。真实 CMD → PS7 → JVM 回归六项通过，覆盖模块、PATH shim、无模块外部 PS1、含空格 cwd/argv、字面 shell 字符、stdout/stderr 和退出码 23。
- 修复 psmux pane target：`=session:%1` 返回 `can't find window: %1`；使用经 session/server 身份核对的原生 `%1`。旧格式记录拒绝使用并给出重建指引。独立真实 session 的创建、身份探测、绑定、recovery hook 设置/撤销及清理通过。
- 修复 mailbox spec 的 Windows 路径断言，使用 canonical path；该组 37 examples / 141 assertions、零失败。
- A/B 的真实 `uml.cmd crap` 与 `uml.cmd ir` 返回 0，两个 namespace 的覆盖率均为 100%。原版 clj-mutate 配置默认调用 `clj`，验收 fixture 改用显式 coverage/test command 与 `--test-roots spec` 后 baseline 通过，但 worker 的 `Files/createSymbolicLink` 因 Windows 缺少权限失败，退出 1。曾临时在外部依赖加入复制 fallback，A/B 各得到 3/3 和 1/1 killed；这些是实验结果，**不算固定版本 mutation 验收通过**。外部依赖已恢复原版，实验补丁单独保存在 evidence。不是 uncovered-mutant 退出 3。
- B 的真实 Claude 启动并发送 `:display`；viewer 消费后 mailbox 为 `{:next-id 2 :queue []}`。首次 context/regen wake 的即时 capture 校验偶发报告 `psmux wake effect was not observed`，但真实 Claude 随后按 FIFO 消费了两条命令；在队列清空后再验一次，两次 wake 均成功、队列清空且新的 display 被消费。该误报已于 2026-10-07 修复（issue #7）：wake 校验改为 2 秒窗口、100 毫秒间隔的有限轮询观察（`companion.clj` 中 `wake!` 的 `{:timeout-ms :interval-ms}` options，默认 2000/100），超时仍报错且 mail 保留在队列。受控 spec 证据：`windows_companion_spec.clj` 新增两个 it，分别证明 capture-pane 前几次不可见、之后才可见的渲染延迟不再误报，以及始终无效果时仍以 `:failure` 抛错；本机全量 spec 496 examples / 1943 assertions，18 项失败均为基线既有环境问题（路径分隔符、缺 Node 等），与该修复无关。修复后的真实 Claude wake 复验仍待进行。
- B 的受控 Claude 退出触发了 respawn，但恢复后 psmux 的 `#{pane_pid}` 为空，harness 以 `For input string: ""` 失败。后来手动诊断 respawn 已改变测试状态，故不能据此证明原始 hook 完整恢复成功；恢复项保持 Pending。该问题由 issue #8 跟踪并已在 2026-10-07 的 recovery 验收（见下节）中定位根因并完成 fixture 级恢复验证。
- B 通过 `:quit-for-restart` mailbox 正常退出 viewer；再次核对 session/server/pane 和 owner-start 后，公开 `shutdown-children!` 清理成功，companion 记录为 `{}`。这是公开接口清理，不能代替 GUI close 验收。

本次仓库质量检查亦未全绿：CRAP 退出 1，配置的本地 `crap4clj` 路径 `\\Mac\Home\Desktop\dev\clojure\crap4clj` 不存在；companion/sketch 分别执行差分 mutation，均因 coverage 无法生成退出 1，未执行 mutants，既有快照保留。相关 Windows/sketch/mailbox spec 为 96 examples / 15 failures / 367 assertions；单独 Windows companion + mailbox 为 30 examples / 1 failure / 76 assertions，唯一失败是缺少 Node。全 coverage suite 为 494 examples / 19 failures / 1939 assertions，退出 19；其他失败包括 Unix/macOS fixture、Windows 路径分隔符和换行假设。失败运行的 LCOV 不能算质量检查通过。IR 检查退出 0，输出 `target/companion-sketch-check.edn`；没有改动受保护的 `examples/uml-viewer.edn`。

完整 A/B 并行隔离、wrapper 重启和原 Claude 身份保持、GUI pan/zoom/focus/proposal 恢复、GUI close，以及 Windows Terminal 有无两种情况仍为 Pending。不能声称原生 Windows 或全平台完整验收通过。

本机证据目录：`C:\Users\billyq\AppData\Local\Temp\uml-win-full-20261006-222855\evidence`。包含 B observe、mail-sent/mail-observed/mail-pane、before-recovery、quit-sent，以及 `mutation-experimental-copy.patch`；fixture 的 policy、metrics、mailbox、session EDN 和 viewer log 保留在相邻 A/B 项目。实验 mutation 快照不应被当作原版工具的验收证据。

## 原生 Windows 质量检查修正 — 2026-10-07

针对上一节记录的质量检查失败（issue #10），在独立 worktree（分支 `issue-10-quality`，基线 `spec-6-windows-acceptance` c317a9d）修正，合并 issue #7/#8（a19e1ab）后在合并树上真实重跑，环境同上节的便携 Temurin JDK 21.0.12.1+1 与 Clojure CLI 1.12.6.1673。

- `:crap` alias 由不存在的本地路径 `../clojure/crap4clj` 改为按 git SHA 固定 `io.github.unclebob/crap4clj`（e90be2e），与 `:mutate` 的固定方式一致。crap4clj 经 `sh -c "clj -M:cov --lcov"`、clj-mutate 经直接进程启动调用 `clj`，而官方 Windows Clojure CLI 只有 PowerShell 模块；按要求声明 prerequisite：在 `C:\Users\billyq\.local\bin` 放置转发到 ClojureTools 模块的 `clj`（sh 脚本）与 `clj.exe`（.NET Framework csc 编译的转发器，源码随证据保存），README 已记录该前置条件。
- spec 修正：macOS/Linux fixture 规格的 `with-redefs` 补 `sketch/windows? (fn [] false)`（与 windows_companion_spec 既有 stub 风格一致）；路径断言改为与 `existing-file` 的正斜杠归一一致；stderr 换行断言改用 `System/lineSeparator`；真实 `/bin/sh` 进程规格在无 Unix shell 时、真实 Node 原生 argv 规格在检测不到 Node 时以 speclj `pending` 声明缺失 prerequisite，不再失败。
- 全 spec 套件 `clj -M:spec`：501 examples / 0 failures / 2007 assertions / 2 pending（Unix shell、Node），退出 0。
- 全 coverage `clj -M:cov`（随 CRAP 内部运行）：501 examples / 0 failures，退出 0，LCOV 写入 `target/coverage/lcov.info`，ALL FILES 93.34% 行 / 96.46% 形式。
- CRAP `clj -M:crap`：退出 0；固定版本重写 `.metrics/crap.edn`（单行格式），973 条目（含 #8 新增 3 条），相对旧快照 8 条数值变化，全部位于本分支与 #7/#8 触及的 sketch/companion。
- 差分 mutation：`clj -M:mutate src/uml_viewer/adapters/companion.clj` 与 `.../sketch.clj` 的 coverage 生成均成功、baseline 均 PASS（18.5s），随后 worker 目录 `Files/createSymbolicLink` 因 Windows 缺少符号链接特权（Developer Mode 未启用、非管理员）抛 FileSystemException，退出 1，mutants 未执行，既有快照保留。companion 运行另报告 11 个未覆盖行 mutation（其中 M037–M050 位于 #8 新增的 `windows-recovered!`）。这是 #9 的符号链接权限阻塞，不算 mutation 验收通过，也未改动外部依赖；#8 新增的差分 surface 同样未执行 mutants。
- IR 检查 `clj -M:ir examples/uml-viewer.policy.edn target/ir-check.edn`：退出 0，内容与受保护的 `examples/uml-viewer.edn` 一致（仅行尾符差异），未改动该文件。

证据目录：`C:\Users\billyq\AppData\Local\Temp\uml-win-quality-20261007`，含 spec-suite、coverage、crap、两个 mutate、ir-check 日志及 clj shim 源码（`merged-*` 为合并后结果）。上一节的完整 A/B、GUI、Windows Terminal 等 Pending 项不受影响。

## 原生 Windows 差分 mutation 验收 — 2026-10-07（issue #9）

针对上一节记录的符号链接阻塞，在管理员提权 shell 下以原版固定依赖（`io.github.unclebob/clj-mutate` c8c0eed，未改动）完成仓库级差分 mutation，基线为 `spec-6-windows-acceptance` 9dbd655，环境同上节的便携 Temurin JDK 21.0.12.1+1 与 Clojure CLI 1.12.6.1673。

- 提权验证：`net session` 通过；Temp 下 `New-Item -ItemType SymbolicLink` 创建成功（验后已清理）。子进程 JVM 继承 SeCreateSymbolicLinkPrivilege。
- 新发现并记录的前置条件：提权只解决特权问题。仓库位于 UNC 共享（`\\Mac\Home\Desktop\dev\uml-viewer`）时，worker 在 `target\mutation-workers\run-*\worker-*` 下创建符号链接仍抛 FileSystemException（"函数不正确"）退出 1，mutants 未执行——共享文件系统不支持重解析点，与权限无关。README 已补充：提权 shell 或 Developer Mode 二选一，且 checkout 必须在支持符号链接的本地文件系统（NTFS）上。
- 实际执行方式：将仓库（不含 `.git`/`target`/`.cpcache`）镜像到本地 NTFS 临时目录 `C:\Users\billyq\AppData\Local\Temp\uml-mut-run` 后运行，快照按相对路径与源码 SHA-256 记录，写回仓库 `.metrics/mutate/`（diff 确认仅 companion/sketch 两个快照变化）。
- `clj -M:mutate src/uml_viewer/adapters/companion.clj`：coverage 生成成功，baseline PASS（18.2s）；113 站点 / 78 覆盖 / 12 未覆盖，执行 78 mutations，**78/78 killed（100.0%）**，退出 3。12 个未覆盖行 mutation（M014–M017 defrecord、M050–M052、M059–M063 `windows-recovered!`）属 coverage gap，按仓库语义保留快照、不强制重跑、不记失败；#8 新增差分 surface（69 个新形式 mutation）全部真实执行并 killed。
- `clj -M:mutate src/uml_viewer/adapters/sketch.clj`：baseline PASS（16.4s）；223 站点 / 8 覆盖 / 0 未覆盖，执行 8 mutations，**8/8 killed（100.0%）**，退出 0。
- 实验性 copy fallback 结果未计入；本次为原版工具真实执行，符号链接路径未打补丁。

证据目录：`C:\Users\billyq\AppData\Local\Temp\uml-win-mutation-20261007`（companion/sketch 运行日志及 UNC 共享失败日志 `mut-companion-unc-share-fail.log`）；完整本地运行目录保留在 `C:\Users\billyq\AppData\Local\Temp\uml-mut-run`。表格中 A/B 两项目程序级 `uml.cmd mutate` 未在本轮重跑，保持 Pending。

## 原生 Windows recovery 验收 — 2026-10-07（issue #8，fixture 级）

环境同上（Windows 11 build 26300 ARM64、PowerShell 7.6.6、固定 psmux 3.3.8 `66cf613 2026-08-18`）。使用全新 fixture 与独立 psmux session，没有触碰任何真实 Claude 或既有 psmux session；fixture 为可长期运行的假 Claude（`fake-claude.ps1`，逐行消费 stdin 并写 receipt），通过真实 adapter 的受控 encoded runner（`windows-command!` manifest）启动，因此 respawn 恢复的是与生产完全相同的 runner 命令路径。本轮证据为 fixture 级：真实 Claude 的退出恢复仍按验收程序第 7 步 Pending。

**根因结论：psmux 3.3.8 元数据缺陷**，不是恢复时序也不是进程身份不符。证据链：新 session 中 `#{pane_pid}` 正确返回 fixture PID（如 4100，父进程为 psmux server）；受控终止后 respawn 真实发生（新 pwsh 5636/3692 作为同一 server 的子进程存在，`pane_dead=0`）；但 `#{pane_pid}` 在随后 15 秒以上、数十次轮询中始终为空，`#{pane_current_command}` 同时损坏（显示主机名 `BILLYPARALLEL` 而非 `pwsh`）。时序假设被持续为空否决；身份不符假设被 OS 进程证据否决。

**规避方案**（已实现在 `companion.clj` 并注释）：`windows-pane-pid!` 对 `#{pane_pid}` 做有界轮询，空元数据永不直接解析，持续为空抛出带 pane 状态（`:pane-dead`/`:pane-command`）的可诊断错误；`windows-recovered!` 在元数据缺失时回退到 OS 证据——枚举已验证 server PID 的直接子进程，要求有且仅有一个携带原始 encoded command token（token 绑定 cwd manifest 与 argv）的新进程，零个报 `:no-recovered-process`，多个报 `:ambiguous-recovered-processes`。注意 psmux respawn 的实际进程会被包一层 `pwsh -NoLogo -Command`，token 子串匹配对此宽容。不升级固定 psmux 版本（维护者决策，超出本修复范围）。

**真实恢复验证结果（ACCEPTANCE-PASS，fixture 级）**：受控终止已验证身份的 fixture（pid 14020，server 3728 子进程）后 pane-died hook 触发 respawn；`windows-recovered!` 在 15 秒 pane_pid 轮询持续为空后回退 OS 证据，确认**同一已验证 server 下**携带原始 encoded command token（即同 cwd manifest 与 argv）的**唯一新进程** pid 3692，`pane_dead=0`。OS 回退本身不能证明 pane 绑定（pane 元数据正是损坏的部分）；pane 归属的证据来自下一步：向 pane `%1` 发送字面 mail `MAIL8-AFTER-RECOVERY`，被 respawn 后的 fixture 真实消费并写入 receipt——只有位于该 pane 的进程才能收到该输入。随后撤销 hook、kill session，`has-session` 返回 1，无残留。

**闭环说明**：原始的 `For input string: ""` 失败发生在已删除的外部验收 harness（不在本仓库），它直接解析 `#{pane_pid}` 的空输出。本仓库现在以 `windows-recovered!` 作为恢复验证 API（`companion.clj`，带 `windows-pane-pid!`/`windows-server-children!` 探针）提供规避方案；外部 harness 应改用它而不是自行读取并解析 `#{pane_pid}`。

本机证据目录：`C:\Users\billyq\AppData\Local\Temp\uml-issue8\evidence-20261007-105856`，含 `psmux-version.txt`、`before-recovery.txt`（原始身份与命令）、`hook-armed.txt`、`quit-sent.txt`、`after-recovery.txt`（OS 回退证据）、`pane-pid-after-respawn.txt`（缺陷证据）、`identity-verdict.txt`、`mail-after-recovery.txt`、`session-cleanup.txt`。该行为 fixture 级恢复证据；表格中 “Mail works after recovery and restart” 行的 restart 部分仍 Pending。

## 原生 Windows A/B 现场验收 — 2026-10-07（issues #11/#12，自动证据与用户人工确认）

使用独立 fixture `C:\tmp\ab-20261007\Project A With Spaces` 与 `Project B With Spaces`，没有把已有用户项目作为测试资源。安装后同步到固定 checkout `07fe5dc8bc4659d4825dd9470d93c7bfe71800d2`；psmux 仍固定 3.3.8。证据与 harness 位于 `C:\Users\billyq\AppData\Local\Temp\uml-win-ab-gui-20261007`。

- 两项目真实 `uml.cmd ir`、`crap`、两个文件分别 differential `mutate`、最后 `ir` 均 exit 0。各项目 7 examples / 0 failures / 7 assertions，CRAP coverage 100%；alpha 1/1、beta 3/3 mutants killed。fixture `deps.edn` 使用原版固定 clj-mutate c8c0eed；本轮核对 gitlibs checkout 无修改。证据为 `A/B-{ir-1,spec,crap,mutate-alpha,mutate-beta,ir-2}.log`，未强制重跑 mutation。
- 07fe5dc 本轮 baseline 的 viewer mail next-id 为 A3/B4。2026-10-07T12:40:40Z 观察到 A4/B13 且 queue 均空，真实 Claude pane 明确报告已按已核实工具环境执行 CRAP/IR 并发送新 display。A JVM1964、Claude6532、server1304；B JVM12704、Claude12772、server3356；进程创建时间、父子树、session/pane/server 与 owner-start 已核实，A `$47/%1`。这是实际进程/mail 消费证据，不能替代图形显示的目视确认。证据为 `A/B-resume-current.txt`、`A/B-resume-current-pane.txt`、`A-resume-identity.edn`。
- A 的公开 `request-agent!` / `request-regen!` 依次发送 `:context`（Saved proposal）id1、`:refresh-crap` id2、`:regen` id3，三个 `:woke?` 均 true。真实 Claude capture 显示按顺序 pop，明确切换 Saved context；12:43:01Z 仅 id3 留在队列。此时 B 原 session/pane/Claude/JVM 身份与 policy SHA-256 `AF15284BBA7F21FC184D3B61C1E35D6225DC7DD1FD69855FC01058BEE6C60601` 不变，B mail 亦不变。最后检查 A agent queue 已空、viewer next-id4→6 且 queue 空，但现场已经退出，未捕获最后工具结果；不据此宣称全部 FIFO 工具或 GUI proposal 点击通过。证据为 `A-fifo-*-sent.edn`、`A-fifo-progress*-pane.txt`、`A-fifo-progress2.txt`、`B-fifo-control.txt`。
- 对 A 已创建 companion 调用真实 `open-window!`：WT 存在时返回 `:windows-terminal`（exit 0）；仅从 harness 子进程 PATH 排除 WindowsApps 并确认找不到 wt.exe 后，真实启动失败 error 2，返回 `:none`，输出带绝对 psmux 路径与 session-qualified target 的 PowerShell attach 命令。没有修改全局 PATH 或卸载 WT。证据为 `A-WT-present.edn`、`A-WT-absent.edn`。准备执行原样诊断 attach 时 companion 已成为 `{}`，因此实际 fallback attach 和可见窗口确认保持 Pending。
- 2026-10-07T12:45:00Z 至 12:45:01Z 只读核实 A/B 旧 launcher/JVM/server/Claude 均已退出，两份 companion 为 `{}`，两项目 `session.edn` 均不存在。此轮执行者没有发送 quit、终止进程或调用 cleanup；用户随后明确确认关闭了窗口，因此退出来源已确认是用户操作。viewer log 无关闭顺序证据。没有自动 fresh 重建，未触碰既有 psmux5356。不能据此证明 GUI close、hook 先禁用、无 respawn 或 A 关闭时 B 仍可用。证据为 `A-unexpected-A-close.txt`、`B-after-A-close.txt` 及两项目 `uml-viewer-log.txt`。
- 颜色问题只读白名单发现工具父环境 `NO_COLOR=1`，TERM/COLORTERM/FORCE_COLOR/CLICOLOR/CLICOLOR_FORCE 未设置。这只是 inherited 单色候选原因；未在真实 Claude pane 内确认环境，也未用目视验证色彩。普通 capture 文本没有 ANSI 不构成单色证据，未修改用户 profile 或全局颜色变量。

首轮现场退出时，真实 Claude recovery、恢复后 mail、`:quit-for-restart` → `uml.cmd --restart` 的新 JVM 与原 companion 身份保持、GUI 非默认 pan/zoom/declutter/focus、proposal 展开层恢复、GUI close 及 fallback 实际 attach 均 Blocked/Pending。用户随后明确授权重开 fixture 完成剩余验收，续跑事实如下；issues #11/#12 尚未完整通过，全平台仍 Pending。

### 用户最终人工验收确认 — 2026-10-07

用户在剩余验收流程中明确确认“都验收了，没问题”，作为 issues #11/#12 剩余 GUI 状态、两轮 namespace focus 与命名 proposal 展开层恢复、公开 wrapper restart、重启后 mail、GUI close、终端和颜色行为的人工验收结果。对应表格以 Passed（用户人工验收）限定；已有自动捕获的恢复/mail/attach 证据单独保留。

人工确认没有提供新的具体 PID、saved session EDN、GUI 截图或 hook 事件顺序。因此本轮 harness 未捕获的 `:quit-for-restart` 保存→旧 JVM 退出→wrapper 新 JVM→原 companion PID 保持链，以及 GUI close 禁 hook→销毁自身 session→无 respawn 时间序列，均不能表述为自动观测结果。第一轮退出本身也不作为该顺序的证据。收到最终确认后停止追加自动重启、GUI 请求及窗口操作，没有重开或清理用户窗口。原生 Windows #11/#12 的验收结论不代替 macOS/Linux/WSL 验收，全平台声明保持 Pending。

### 授权重开后的真实恢复与 fallback — 2026-10-07

- 在确认两个 fixture 的旧资源均已退出、安装仍为 07fe5dc 后，13:13:54Z 起通过真实 `uml.cmd` 依次重开 B/A（均 exit 0），新 display baseline A6/B13，真实 Claude 新 display 消费后 A7/B14。新 A JVM1560 / Claude14012 / server4304 / session `$49` / pane `%1`；B JVM13532 / Claude11384 / server1180。没有重跑已通过的 mutation 或仓库质量门。
- 精确核对 A server/runner 命令 token、父子关系及真实 Claude PID14012 的创建时间后，仅终止该 fixture Claude。原 recovery hook 实际 respawn；`windows-recovered!` 确认同 server4304 下携带原 encoded command token 的唯一新 runner12756。真实进程树为 runner12756 → pwsh10908 → Claude4564，原 session/cwd/command 保持，pane_pid 空缺仍由恢复 API 的 OS 证据回退处理。随后向原 session 的 pane 发送指令，并通过公开 API 发送 context Saved id4/regen id5，wake 均 true；恢复后的真实 Claude 明确报告处理两条 mail、执行 IR 并发送 display，A next-id7→8、两队列为空。这同时提供恢复进程实际位于原 pane 的输入消费证据。13:20:43Z B 原 JVM/Claude/server、policy hash 及 mail 不变。证据为 `A-continuation-before-recovery.json`、`A-continuation-termination.json`、`A-continuation-recovered.edn`、`A-post-recovery-{context,regen}.edn`、`A-post-recovery-mail.json`、`A-post-recovery-complete-pane.txt`、`A-post-recovery-mail-complete.txt`、`B-post-recovery-mail-control.txt`。
- 对新 A session 真实排除 wt.exe，再得到 error2 与绝对路径手动 attach 诊断。原样在新 PowerShell 执行诊断命令，psmux list-clients 出现原 `/dev/pts/20` 之外的新稳定 client `/dev/pts/276`；OS 核对 psmux client980 为 pwsh8180 的子进程、command target 为原 A session。初始 harness 把 activity 时间变化误当新 client，已纠正，以稳定 tty 和真实子进程保存最终证据，不重复打开窗口。证据为 `A-continuation-WT-absent.edn`、`A-continuation-manual-attach.json`。可见窗口与色彩仍待用户目视确认。
- 仅在 fixture wrapper 启动子进程撤销父环境 NO_COLOR；未设全局 FORCE_COLOR 或修改用户 profile。恢复后真实 Claude 将实际工具白名单写入 `.uml-viewer/color-whitelist.json`：NO_COLOR 空、TERM `xterm-256color`、COLORTERM `truecolor`、FORCE_COLOR null。该结果确认新进程不再继承禁色标记，但屏幕色彩仍需目视。GUI 状态和两轮 wrapper restart 尚待用户设置，未用 EDN 修改代替 GUI 操作。

## Historical native acceptance procedure (pre-Herdr backends)

The procedure below belongs to the earlier tmux/psmux acceptance records.
Its backend versions, terminal strategies, recovery hooks and all-platform
gate are historical; do not use it as the current Herdr procedure. Current
support, lifecycle requirements, local delivery status and deferred checks are
recorded in [the Herdr contract](herdr-backend-contract.md); Windows native
observations and the seven remaining gaps are recorded in
[the #30 baseline](windows-herdr-30-baseline.md) and
[observations](windows-herdr-30-observations.md). macOS #22 was
cancelled, not passed. WSL companion launch **is** supported (spec #106); its
record is [the #106 evidence](../acceptance-evidence/106-wsl/README.md). The
steps below still describe the retired tmux/psmux procedure; use the Herdr
contract as the current one.

Use two new projects under a short unique temporary root, for example `a5-20261006/Project A With Spaces` and `Project B With Spaces`. Never use an existing project's viewer or companion as a test resource. Record versions and fixed checkout SHA, timestamps, exit codes, project log, policy hashes, mail EDN, saved-view EDN, JVM PID/parent, and exact session/pane/Claude identities. Do not dump credentials, the environment, or unrelated sessions.

1. Install prerequisites yourself: PowerShell 7, Git, Clojure CLI, Java, Claude, and the native backend. On Windows use exactly psmux 3.3.8; run `psmux.exe -V` and retain both provenance lines. On macOS/Linux/WSL use `tmux -V`. Record `pwsh --version`, `java -version`, `clojure -Sdescribe`, and `claude --version`.
2. Create a small Clojure project with two nested namespace files and corresponding tests. Give `deps.edn` genuine `:crap`, `:mutate`, and coverage aliases; configure local roots for that host. Create a namespace-nesting policy with an existing named proposal. Copy the same fixture into A and B. Record the initial policy hash.
3. Install a fixed branch/tag with `UML_VIEWER_REPO_URL` and `UML_VIEWER_REF` set for that invocation. Unix: invoke `pwsh -NoProfile -File /path/to/scripts/get-uml-viewer.ps1 --install-only` from each project. Windows: invoke `pwsh -NoProfile -File C:\path\scripts\get-uml-viewer.ps1 --install-only`. Repeat installation and verify policy/proposal and user configuration are preserved.
4. Unix: run `./uml ir`, `./uml crap`, then `./uml mutate src/demo/core/alpha.clj` and `./uml mutate src/demo/core/beta.clj`, one file per invocation, then `./uml ir`. Windows: run `.\uml.ps1` (or `pwsh -File .\uml.ps1`) with the same arguments; no `.cmd` shim is installed. Save stdout/stderr/exit status. An uncovered-mutant exit 3 is a coverage gap: retain its snapshot and continue to IR; do not rerun that file or force full mutation.
5. Launch B, then A through their wrappers. Wait to a finite deadline for each real Claude to send `:display`, for the queue to be consumed, and for the diagram to appear. Capture only those test panes. Confirm project cwd, recorded session/pane, and real Claude process. If inherited `CLAUDECODE` prevents nesting, remove only that marker for these explicitly new test processes; do not change global settings.
6. On A, click a named proposal to send context, then request refresh/regen. Observe FIFO queue consumption, actual tool execution, and `:display`. Verify B's session/pane/Claude identity and policy remain unchanged. Do not concurrently rewrite a mailbox being consumed by Claude.
7. Obtain A's exact owned pane PID and verify the real Claude process before interruption. On Unix, `kill -TERM <verified-test-Claude-PID>` is sufficient; do not use SIGKILL. On Windows, use the controlled test-process termination available on that host. Observe new real Claude PID in the same pane with the original cwd and launch command, then send mail and verify real consumption. Never terminate another process or use `kill-server`.
8. Set nondefault pan/zoom/declutter and a nonempty namespace focus via GUI. Ask the test companion to enqueue `:quit-for-restart` (ordinary FIFO queue format). Confirm saved session EDN and old JVM exit. Run `./uml --restart` or `.\uml.ps1 --restart`; never pass restart directly to the JVM. Record new JVM PID and unchanged companion session/pane/Claude PID. Verify displayed state and working mail. Repeat with the named proposal and its expanded layer when focus and proposal cannot be simultaneously valid.
9. Close A's viewer using its GUI close control. Confirm its recovery hook was disabled before only A's session was destroyed, no respawn occurred, and B remains usable. Then close B. If GUI is unavailable, record pending and clean only confirmed owned test resources with the existing graceful save/quit and public cleanup boundaries; do not describe that as GUI close acceptance.
10. Platform terminal checks: macOS confirms Ghostty attachment and the fallback to Terminal.app when Ghostty is unavailable; Linux confirms its documented attach/terminal strategy; Windows runs once with Windows Terminal available and once without it, confirming the diagnostic manual attach command can attach to the already-created psmux companion. Repeat Windows failure/timeout binding against its fixed psmux baseline. WSL runs its Linux backend and terminal compatibility checks separately and cannot satisfy native Windows acceptance.

Only change the platform cells to passed when the corresponding real evidence is available. Until every native platform has completed the full procedure, keep the all-platform claim pending.
