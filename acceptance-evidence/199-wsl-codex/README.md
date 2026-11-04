# #199 WSL2 真实 Codex 端到端验收

日期：2026-11-04。spec #192 的 Codex CLI 提供方，在 WSL2 上用真实 Codex、真实 Herdr、
真实质量工具（公共安装入口装入的 crapper / mutator / dryer）运行。没有使用
`CODEX_BIN` 替身、fixture 或占位进程。

## 环境

| 项 | 值 |
|---|---|
| 平台 | `Linux 6.18.33.2-microsoft-standard-WSL2 #1 SMP PREEMPT_DYNAMIC x86_64`，Ubuntu 24.04.5 LTS，WSLg（`DISPLAY=:0`，D3D12 / GTX 1080 Ti） |
| Codex | `codex-cli 0.166.0`（`~/.nvm/versions/node/v24.22.1/bin/codex`，Node 24 `execve` 到原生 musl 二进制，runner 记录的 child 即 codex 本身）；`codex login status` exit 0；Codex 后端模型经用户网关（pane 显示 `Opus medium`） |
| Herdr | `herdr 0.9.5`，protocol 22；`herdr integration status` → `codex: current (v10)` |
| Codex hook | 用户自己的 `~/.codex/hooks.json` SessionStart → `herdr-agent-state.sh`，已在 `config.toml` 中持久信任；本次未改动（`codex-config-sha-before/after.txt` 相同） |
| Python / Rust | 3.12.3 / 1.99.1 |
| uml-viewer | worktree `spec192/199`，开发安装（`UML_VIEWER_TOOL_ROOT`），release 二进制 |
| 项目 | `/tmp/spec199-run1/A 项目`、`/tmp/spec199-run1/B 项目`：同一个 3 模块 Rust crate（`ledger`、`ledger.calc`、`ledger.calc.util`、`ledger.store`，各带单元测试），git 仓库，policy 带命名提案 `Saved` |

启动方式：`harness/launch.sh`，即在项目目录以 `UML_COMPANION_NO_ATTACH=1`
运行 `./uml --coding-agent codex`（restart 为 `./uml --restart`）。邮件触发方式与 #114 相同
（**半自动**）：`harness/*.edn` 经 `UML_VIEWER_TEST_DRIVER` 在真实 viewer 内等待触发文件，
然后真实选中类并点右键菜单项；之后的 `to-agent.edn` 写入与 Herdr wake 走产品自己的路径。
`harness/phase.sh` 放触发文件，等队列清空、pane 回到 idle，再保存 pane 文本。

## 结果矩阵

| 场景 | 结果 | 证据 |
|---|---|---|
| fresh：修复前 | **失败**（缺陷 1）：git 项目首次启动停在 Codex "Trust this folder?"，`agent_status: blocked`，没有 SessionStart 与 rollout；45 s 后按契约释放本次 Herdr session，ownership `{}` | 本文“缺陷 1” |
| fresh：身份与首次展示 | 通过：runner 启动后 8 s 内 Herdr 报告 `agent_session {agent codex, source herdr:codex, kind id}`，写入 `:coding-agent :codex`、`:codex-session`、显式 `resume` 恢复命令；Codex 保留 policy 与 `Saved` 提案，`./uml ir` exit 0 后写 `:display`，viewer 消费 | `run*/ownership-fresh.edn`、`runner-fresh.edn`、`pane-launch.txt` |
| 邮件 FIFO | 通过：同一次 wake 前后送达 `mail-1`、`mail-2`，按 1→2 处理，各一个 `:display`（to-viewer next-id 2→4）；恢复后排空的 9→10 同样按序 | `run1/to-agent-1.edn`、`pane-1.txt`；`run3/pane-recovery.txt` |
| 质量成功 | 通过：`:refresh-crap` 整树 crap exit 0、`:refresh-mutate` 差分 exit 0，记录 version 1、operation、规范项目根、实际 exit 一致且 `validResult true` 后才 IR 与 `:display` | `run1/pane-1.txt` |
| 质量失败门控 | 通过：把 `calc` 测试期望值改错后 `:refresh-crap` → crapper exit 1、`validResult false` → 无 IR、无 `:display`，Codex 报告失败断言且不改源码（sha 校验）；紧随其后的 `:omit store` 仍被消费：policy 只加 `:omit [:store]`，IR exit 0，一个 `:display` | `run1/pane-2.txt`、`broken-sha.txt` |
| 缺失 / 过期记录 | 通过：`uml-quality.json` 写成截断 JSON 后 `:refresh-mutate` → dispatch exit 1、没有发布记录 → 无 IR/display；紧随其后的 `:omit ledger` 正常 IR + `:display` | `run2/pane-3.txt`、`phase-3.out` |
| 不一致记录（operation / 根 / exit 不符） | 跳过：dispatch 每次先删旧记录，真实会话无法自然产生；与 #114 相同，由 `test_posix_entries.py` 与 `test_posix_codex_companion.py` 覆盖。Codex 每次都逐字段核对了记录（pane 文本） | — |
| viewer restart | 通过：`:quit-for-restart` → 旧 viewer 退出、`session.edn` 保存 → `./uml --restart`（未带 `--coding-agent`，沿用记录的 Codex）→ 新 viewer；Codex PID 927732（start 1843184）不变，Herdr `agent_session` 与 `:codex-session` 都是 `01a1a56b-a890-7b80-9fcf-68e415e39e72`；restart 后的邮件 8 完成 crap → IR → `:display` | `run2/codex-before-restart.txt`、`restart.log`、`pane-4.txt` |
| 受控异常退出：修复前 | **失败**（缺陷 2）：SIGTERM 后 exit 143、1 s 内提交第 1 次恢复，但 runner 命令被 zsh 拒绝（exit 127），journal `:failed "Runner acknowledgement missing; exit cause unknown"`，邮件 9 保留在队列 | `run2/recovery-failed.edn`、`pane-recovery-failed.txt` |
| 受控异常退出：修复后 | 通过：核对 runner 记录的 child PID 942320 / start 1920731 及其父进程为本项目 runner 后 `kill -TERM`（07:32:43.59Z）；exit 143，约 1.1 s 后 `:attempts 1`，新 child 946341 在提交后 0.89 s 启动，argv 为 `codex resume … -C <项目> … 01a1a577-7e09-7240-b1e4-755d8a322304 <恢复提示>`（无 `--last`/`--all`）；`:session-check` 以同一 UUID 清除；宕机期间经右键发出的邮件 10 与此前遗留的 9 按 FIFO 排空（两个 `:display`）；恢复后的 wake（邮件 11）照常消费 | `run3/recovery-*.edn`、`pane-recovery.txt`、`pane-6.txt` |
| 旧 `agent_session` 是否跨进程残留 | 不残留：Codex 退出后 `herdr agent get` 立即返回 `agent_not_found`，所以恢复核对只能被新进程的报告通过 | `run2` 恢复期间读取 |
| 正常关闭不恢复 | 通过：A viewer 正常关闭 → journal `:cancelled "Viewer closed; recovery revoked"`，Codex 与 controller 退出，没有 `runner-2`，Herdr session 删除，ownership `{}`；另在 B 里 Codex `/quit` → runner exit 0 → journal `:normal "Companion exited normally"`，不重启；之后关闭 B viewer 清理 | `run3/recovery-after-close.edn`、`run3-project-B/recovery-after-quit.edn` |
| 多项目隔离 | 通过：B（`01a1a577-7d03-…`，Codex PID 942269）与 A 同时运行；A 的恢复、邮件与关闭期间 B 的 ownership sha、Codex PID、`agent_session` 不变，B 的邮件 b1 独立完成；用户的 `default` Herdr session 与另一个 Codex 进程（PID 556315）全程存活、未被触碰 | `run3-project-B/` |
| 错误 occupant | 通过（带边界）：在 A 的 pane 里 `/new`。新对话直到第一条 prompt 才触发 SessionStart，所以紧接着的 wake（邮件 12）仍投给新对话并被处理；新对话随即上报 UUID `01a1a57b-1530-…`。之后的 wake（邮件 13）被拒：`Companion wake failed: No Codex CLI agent occupies the owned Herdr pane with its saved session`，没有投递 prompt，邮件 13 保留 | `run3/agent-session-after-new.txt`、`companion-worker-*.event-*.edn` |
| companion 不自行启动生命周期 | 修复前**失败**（缺陷 3）：Codex 的探测命令里的裸 `./uml` 在 `/tmp/spec199-run1` 拉起了缺省 Claude 的 viewer + companion；修复后带 runner 标记重放同一命令与项目内裸 `./uml`，都在任何资源前 exit 1 拒绝 | `run2/stray-launch-command.txt`、`stray-restart.log`、`stray-launch-refused.txt` |

SessionStart 时效：修复后的三次 fresh 都在 9 s 内上报（预算 45 s）；恢复的 SessionStart 在提交后数秒内以同一 UUID 到达。

## 本轮修复（TDD，红→绿）

1. **Codex 首启停在目录信任屏**（`crates/companion/src/agent.rs`）：fresh 与 resume 的 argv
   都加 `-c projects={"<项目根>"={trust_level="trusted"}}`。这是 Codex 的单次调用覆盖，不写入
   `config.toml`（前后 sha256 相同）；只信任本 companion 的项目根，不扩大到其他进程。
   内联表写法在路径含空格、中文和 `.` 时实测成立（`"` 由 TOML 转义，单元测试覆盖）；项目根是 git 子目录时也生效（探针实测）。
2. **恢复前的终端复位从未生效**（`crates/companion/src/herdr.rs`）：复位序列以原始 ESC 字节
   经 `pane run` 键入 shell，行编辑器把 ESC 当按键吞掉，只剩可见字符被打印。Claude 退出时会自己
   复位终端，所以从未暴露；SIGTERM 的 Codex 留下 kitty 键盘标志，随后的 `ctrl+c` 以 CSI-u
   （`ESC[99;5u`）送达，zsh 行首残留 `9;5u`，runner 命令 exit 127。现改为 `printf '\033[…'`，
   由 printf 自己展开。修复前后都在独立探针 Herdr session 里复现与核对（`MARKER-RAW` 127，
   `MARKER-FIXED` 正常执行）。
3. **companion 自己拉起了另一个 viewer + companion**（缺陷 3，`crates/companion/src/preflight.rs`、
   `recovery.rs`、`posix.rs`、`prompt.rs`、`scripts/uml_launch.py`）。Codex 在 run 2 启动回合
   （2026-11-04T07:18:14Z，rollout 第 46 行，`run2/stray-launch-command.txt`）一边处理遗留的邮件 5
   （`:refresh-mutate`，当时 `uml-quality.json` 已损坏），一边探查 `./uml mutate` 怎么用，执行了
   `cd .uml-viewer && … ./uml-viewer/../../../uml 2>/dev/null; cd .. && ./uml mutate --help`。
   `uml-viewer` 是指向工具 worktree 的符号链接，内核按物理路径解析到主 checkout 的 `uml`，该入口用
   `cd "$(dirname "$0")"` 的逻辑路径把项目根算成 `/tmp/spec199-run1`；不带子命令就是 fresh 生命周期，
   继承来的 `UML_VIEWER_TOOL_ROOT` 让它拉起了一个**缺省 Claude** 的 viewer + companion
   （`run2/stray-restart.log`）。Codex 随后自己发 `:quit-for-restart`，剩下的 Claude companion 由验收者
   经 `--restart` 加正常关闭清理，A/B 项目未受影响。
   根因不在某个模型：常驻共享规则对 companion 说"Prefer ./uml (fresh start: spawn this companion…)"，
   无人值守的 bypass 让探测命令直接执行，而生命周期入口不知道自己正在 companion 里运行。修复与
   provider 无关：runner 给 agent child 设置 `UML_COMPANION_ROOT=<规范项目根>`；`launch` preflight
   在 fresh 或 restart 写入任何资源前，只要见到该标记就拒绝 fresh，restart 只放行该标记自己的项目
   （`:quit-for-restart` 之后的合法 restart）；launcher 不把标记传给它启动的 viewer；共享规则改为
   "never run bare ./uml (a fresh start) or any ./uml lifecycle entry for another directory"。
   修复后在同一 fixture 里带标记重放这条命令与项目内裸 `./uml`：都以
   `Refusing to run the viewer lifecycle inside the companion …` exit 1，没有 viewer 启动
   （`run2/stray-launch-refused.txt`）。

## 观察（未改动）

- 用户全局 Codex 配置里的 hook / 规则拦下了 `rm -f .uml-viewer/quality-result.json`；Codex 改为
  `mv` 到 `quality-result.prev.json` 再核对新记录，门控仍成立。
- 失败的 fresh 会留下该次 session 的 `*-runner.edn*` 文件；worker 的 `command-*` / `event-*`
  文件在 viewer 退出后也保留。两者都不影响下一次启动。
- run 1 的 viewer 因 driver 选错类（`calc.util` 在组件内，不在顶层）以 exit 1 退出；这条路径同样
  走了关闭：journal `:cancelled`、Codex 退出、session 删除、邮件 5 留在队列，下一次 fresh 的 Codex
  在启动回合里主动处理了它。

## 未覆盖

- 人工右键、真实窗口关闭按钮与 Windows Terminal attach（`UML_COMPANION_NO_ATTACH=1`）。
  关闭用 SIGTERM，与窗口管理器退出请求走同一路径。
- hook 未信任时的表现只按契约推断（身份等待超时），没有改用户配置去构造。
- 只验证了 Codex 0.166.0；更早版本没有证据，preflight 门槛定为 0.166.0。
