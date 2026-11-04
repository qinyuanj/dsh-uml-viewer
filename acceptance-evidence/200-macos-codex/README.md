# #200 macOS 真实 Codex 端到端验收

日期：2026-11-04。spec #192 的 Codex CLI 提供方，在 macOS 上用真实 Codex、真实 Herdr、
真实质量工具（公共安装入口装入的 crapper / mutator / dryer）运行。没有使用
`CODEX_BIN` 替身、fixture 或占位进程。本记录与 WSL 的
[199-wsl-codex](../199-wsl-codex/README.md) 分开，彼此不能替代。

## 环境

| 项 | 值 |
|---|---|
| 平台 | macOS 27.0.1（26A434），Darwin 27.0.0 arm64（Apple M1 Max），渲染 Metal |
| Codex | `codex-cli 0.166.0`（`/opt/homebrew/bin/codex`，runner 记录的 child 即 codex 本身）；`codex login status` exit 0（ChatGPT 登录） |
| Herdr | `herdr 0.9.5`，protocol 22；`herdr integration status` → `codex: current (v10)` |
| Codex hook | 用户自己的 `~/.codex/hooks.json`：PreToolUse（rtk）与 SessionStart（`herdr-agent-state.sh`）；SessionStart 的信任见 run 0b |
| Python / Rust | 3.15.2 / 1.99.1（Homebrew rustup） |
| uml-viewer | 本仓库 main（`UML_VIEWER_TOOL_ROOT` 开发安装），release 二进制 |
| 项目 | `/tmp/spec200/A 项目`、`/tmp/spec200/B 项目`（规范根 `/private/tmp/...`）：同一个 Rust crate（`ledger`、`ledger.calc`、`ledger.calc.util`、`ledger.store`，6 个单元测试），git 仓库，policy 带命名提案 `Saved` |

启动方式：`harness/launch.sh`，在项目目录运行 `./uml --coding-agent codex`（restart 为
`./uml --restart`）；A 用 `UML_COMPANION_NO_ATTACH=1`，B 保留 Terminal.app attach。邮件
触发方式与 #199 相同（**半自动**）：`harness/*.edn` 经 `UML_VIEWER_TEST_DRIVER` 在真实
viewer 内等待触发文件，然后真实选中类并点右键菜单项（`harness/gen.sh` 生成）；之后的
`to-agent.edn` 写入与 Herdr wake 走产品自己的路径。`harness/phase.sh` 放触发文件，等队列
清空、pane 回到 idle，再保存 pane 文本。

## 结果矩阵

| 场景 | 结果 | 证据 |
|---|---|---|
| fresh：hook 未信任 | 符合契约：Codex 停在 “Hooks need review”，45 s 后状态栏 `Herdr reported no Codex CLI session within 45 s, likely an untrusted Codex hook … start refused; this start's Herdr session was released`，ownership `{}`，session 删除，viewer 照常可关闭 | `run0-untrusted-hook/driver/gui-after-timeout.edn`、`after-timeout.png` |
| fresh：身份与首次展示 | 通过：启动后 9 s 内写入 `:coding-agent :codex`、`:codex-session 01a1a691-…` 与显式 resume 命令；policy 与 `Saved` 保留，`./uml ir` exit 0 后 `:display` 被消费（viewer 窗口被遮挡时同样消费）；B 同样通过 | `run1-A-*/ownership-fresh.edn`、`pane-launch.txt`；`run1-B-*/ownership-fresh.edn` |
| 邮件 FIFO | 通过：同一次 wake 送达 1（`:refresh-crap`）、2（`:refresh-mutate`），按 1→2 处理，各一个 `:display`（to-viewer next-id 2→4） | `run1-A-*/to-agent-1.edn`、`pane-1.txt` |
| 质量成功 | 通过：整树 crap 与差分 mutate 都 exit 0；记录 version 1、operation、规范项目根、实际 exit 一致且 `validResult true` 后才 IR 与 `:display` | `run1-A-*/pane-1.txt`、`pane-4.txt` |
| 质量失败门控 | 通过：`average_divides` 期望值改错后 `:refresh-crap` → crapper exit 1、`validResult false` → 无 IR、无 `:display`，源码 sha 不变 | `run1-A-*/pane-2.txt`、`broken-sha.txt` |
| 失败后紧随的 `:omit`：修复前 | **失败**（缺陷 1）：Codex 把 `:omit` 当作“policy change”先跑 crap，再被同一失败门控拦下，policy 加了 `:omit [:store]` 却没有 IR / `:display` | `run1-A-*/pane-2.txt`、`pane-5.txt` |
| 失败后紧随的 `:omit`：修复后 | 通过：同一场景下 crap 失败被拦，紧随的 `:omit store` 不跑质量、IR exit 0、一个 `:display`。run 2（第一版措辞，to-viewer 10→11）与 run 3（评审后的最终措辞，12→13）都通过 | `run2-A-omit-after-failed-refresh/pane-f1.txt`、`run3-A-final-rules/pane-f1.txt` |
| companion 不自行启动生命周期 | 通过：Codex child 的环境带 `UML_COMPANION_ROOT=<规范项目根>`；带该标记在 A 内运行裸 `./uml`、在 B 内运行 `./uml --restart`，都在任何资源前 exit 1（`Refusing to run the viewer lifecycle inside the companion …`），viewer 与 Herdr session 数不变，B 的 ownership 仍为 `{}` | `run3-A-final-rules/companion-root-marker.txt`、`stray-launch-refused.txt` |
| 缺失记录门控 | 通过：`uml-quality.json` 写成截断 JSON 后 `:refresh-mutate` → dispatch exit 1、没有发布记录 → 无 IR/display，后续邮件照常被弹出 | `run1-A-*/pane-5.txt` |
| 不一致记录（operation / 根 / exit 不符） | 跳过：与 #199 相同，真实会话无法自然产生，由 `test_posix_entries.py` 与 `test_posix_codex_companion.py` 覆盖 | — |
| viewer restart | 通过：`:quit-for-restart` → 旧 viewer 2 s 内退出、`session.edn` 保存 → `./uml --restart`（未带 `--coding-agent`）→ 新 viewer；Codex PID 56780 与 UUID `01a1a691-28cc-7553-9b74-e49800e45829` 不变，restart 后的邮件 5 完成 crap → IR → `:display` | `run1-A-*/codex-before-restart.txt`、`restart.log`、`pane-4.txt` |
| 受控异常退出 | 通过：核对 child 56780 的父进程为本项目 runner 56779 后 `kill -TERM`（12:45:06Z）；exit 143，约 1 s 后 `:attempts 1`，新 child 61267 的 argv 为 `codex resume … -C <项目> … 01a1a691-28cc-7553-9b74-e49800e45829 <恢复提示>`（无 `--last`/`--all`）；run 1 当场用 `herdr agent get` 读到同一 UUID（未存文件），run 3 重做并存档：恢复前后 `agent_session` 逐字相同；宕机期间经右键发出的邮件 8 按 FIFO 排空（一个 `:display`），恢复后的 wake（邮件 9）照常消费 | `run1-A-*/recovery-after-kill.edn`、`recovery-child.txt`、`recovery-after-drain.edn`、`pane-recovery.txt`、`pane-7.txt`；`run3-A-final-rules/agent-session-*-{kill,recovery}.txt`、`recovery-after-kill.edn` |
| 正常关闭不恢复 | 通过：A viewer 正常关闭 → journal `:cancelled "Viewer closed; recovery revoked"`，Codex 与 controller 退出，没有 `runner-2`，session 删除，ownership `{}`；B 里 Codex `/quit` → `:normal "Companion exited normally"`，`:attempts 0`，不重启；之后关闭 B viewer，ownership `{}`、session 删除、attach 进程退出 | `run1-A-*/recovery-after-close.edn`、`run1-B-*/recovery-after-quit.edn`、`recovery-after-close.txt` |
| 多项目隔离 | 通过：B（`01a1a696-…`，Codex PID 60259）与 A 同时运行；A 的恢复、`/new` 与关闭期间 B 的 ownership sha、runner child 与 `agent_session` 不变，B 的邮件 b1 独立完成；用户的 `default` Herdr session 与另一个 Codex app-server（PID 7165）未被触碰 | `run1-B-*/baseline-before-A-recovery.txt`、`after-A-close.txt` |
| 错误 occupant | 通过（与 #199 相同的边界）：A pane 里 `/new` 后，Herdr 在首条 prompt 前仍报告旧 UUID，第一次 wake（邮件 10）投给新对话并被处理，随后上报 `01a1a698-…`；之后的 wake（邮件 11）被拒：`No Codex CLI agent occupies the owned Herdr pane with its saved session`，邮件 11 留在队列 | `run1-A-*/agent-session-after-*.txt`、`gui-9.edn`（状态栏拒绝文本；worker 的 event 文件已被后续启动清理，未保存） |
| 终端 attach（ADR 0007 macOS） | 通过：B 的 fresh 经 `osascript` 打开 Terminal.app 窗口（窗口数 2→3，标题 `herdr session attach uml-viewer-B----66a70b62-e16e1896`）；关闭 viewer 后 attach 进程退出 | `run1-B-*/attach-process.txt`、`terminal-windows-*.txt` |
| 进程身份（ADR 0007 macOS） | 通过：runner、controller 与 viewer 记录的启动时刻为 `proc_pidinfo` 微秒文本（如 `1793795891720305`），恢复按 PID+启动时刻核对 | `recovery-*.edn` |

## 本轮修复（TDD，红→绿）

1. **`:omit` 被失败的质量门控连带拦下**（`crates/companion/src/prompt.rs`）。常驻规则第 2 步
   “Run ./uml crap …”挂在“After every later source or policy change”之下，`:omit` 也改 policy，
   Codex（GPT-6.1-Sol）于是先跑 crap 再对 `:omit` 套用结果门控；fixture 测试失败时 omit 永远
   不显示。WSL 的同一场景由另一个模型处理，没有暴露。规则现限定质量刷新只用于源码变更或质量
   刷新，并明确 `:omit` 与提案只改 policy：不跑质量、不套门控，IR exit 0 即可 `:display`。
   代码评审指出第一版只限定了第 2 步、步骤 3–5 仍挂在“After every later source or policy
   change”之下，最终措辞改为“Steps 2-4 are for a source change or a quality refresh only; a
   policy-only change (:omit, a proposal) skips to step 5”。新测试
   `a_policy_only_request_runs_no_result_gate` 先红后绿（按空白归一化比对，不钉换行）；Claude
   argv 指纹随规则更新。规则为所有提供方共享，所以 Claude companion 的提示同样改变。
   用真实 Codex 重放同一场景：第一版（run 2）与最终措辞（run 3）都通过。

## 观察（未改动）

- **hook 信任没有经用户终端持久化**（原因未查明；README 的处理说明据此改写）：用户在自己终端审核后，`config.toml` 的 sha 未变，第二次
  fresh 仍超时。抓 pane 看到 companion 自己的 Codex 停在 “Hooks need review”，列表里只有
  SessionStart 一项待审核；经用户授权在该对话框按 `t` 信任后，`config.toml` 新增
  `hooks.state."…hooks.json:session_start:0:0"` 的 `trusted_hash`（sha 前后见
  `codex-config-sha-*.txt`）。之后的 fresh 都在 9 s 内取得身份。
- **Homebrew rustup 不把 `~/.cargo/bin` 放进 PATH**：crapper 自动安装的 `cargo-llvm-cov` 落在那里，
  `./uml crap` 因而 exit 1（`No usable coverage for rust`，`validResult false`）。把
  `~/.cargo/bin` 加进 PATH 后 crap / mutate 都 exit 0；harness 的 `launch.sh` 这样做。
- **被遮挡的 viewer 不推进 test driver**：macOS 不给被遮挡窗口绘帧，driver 在 `ui()` 里推进，
  因此 harness 每步前用 `front.sh` 置前窗口。产品路径不受影响：fresh 的 `:display` 在窗口
  被遮挡时照样被消费。
- pane 文本里 Codex 读到的用户私人记忆（`~/.codex/memories`）片段已替换为
  `[redacted: personal Codex memory excerpt]`，其余文本原样保留。
- **系统截图被 TCC 拒绝**（终端无屏幕录制权限），截图全部来自 viewer 内的 driver。
- **启动回合里 Codex 偏离“只改有意改的键”**：run 2 在 policy 加了 `:order [:calc :store]`，run 3
  在文件头加了一行注释；都不是新包、没删键、提案保留，run 1 没有这样做。这是模型对常驻规则的
  偏离，规则已写明，不另改产品；与 Claude 的业务语义差异记录在此。
- B 的 phase b1 等待 402 s 才结算：邮件 40 s 内处理完，但 pane 被 Terminal.app attach 时
  `agent_status` 没回到 `idle`，属 harness 的判定条件，与产品无关。

## 已知限制

- `/new` 后的第一次 wake 投给新对话，与 #199 及契约 §5 相同。

## 未覆盖

- 人工右键与真实窗口关闭按钮：右键经 driver，关闭用 SIGTERM（与窗口管理器退出请求同路径）。
- 只验证了 Codex 0.166.0 与 Herdr 0.9.5；门槛不变。
