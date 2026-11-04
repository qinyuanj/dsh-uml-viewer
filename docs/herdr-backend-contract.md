# Herdr SessionBackend 契约 — spec #13 / issue #14 产出

本文档记录 Herdr 的能力证据和生命周期契约。**平台以 ADR 0007 为准：交付目标为
WSL2 + Ubuntu 24.04 + WSLg 与 macOS；Windows 原生不受支持。**
文中 Java/JVM 与旧 Windows/macOS 验收记录是历史证据，不代表当前实现。当前实现是
`crates/companion` 的 Rust runner/controller（#144 删除了 LOVE/Lua 实现），进程创建、
等待、身份与 OS 单例 lock 由 POSIX adapter 提供（平台能力表见 ADR 0007；Windows 专用实现
在 spec #106 退役），ownership、预算、FIFO 与质量门控契约保持不变。当前身份文件与重启规则
见 §9.1。#23 退役后 Herdr 是唯一 companion 后端；`--backend herdr` 保留为兼容选项。

WSL 原生证据：Herdr 0.9.5 / protocol 22 下的完整生命周期（preflight、隔离 server、
workspace/pane、runner、恢复注册、验证清理）与公共入口端到端流程；
见 `acceptance-evidence/106-wsl/`。macOS 端到端验收是 spec #32 的 Herdr 0.9.4
历史记录（见 §8），不构成当前支持。能力实验与端到端验收
分开记录；未验证项不按官网声明或命令退出码 0 推断。旧 ownership 迁移见 §9。

证据来源：
- WSL 端到端验收（当前实现）：spec #106，Herdr 0.9.5 / protocol 22，
  `acceptance-evidence/106-wsl/`
- macOS 端到端验收（历史）：#32 / #44 验收日志（2026-10-15，Herdr 0.9.4 /
  protocol 22，Darwin 27.0.0 arm64，acceptance-evidence/acceptance-log.md）
- 历史 checkpoint：Windows #14（2026-10-10，0.9.3）与 macOS #14（同日同版本）

## 1. 版本预检契约

**接受 Herdr >= 0.9.3 且 socket protocol 22；当前实现有端到端证据的版本是
0.9.5（WSL，spec #106）；0.9.4 是 macOS 历史记录（spec #32），0.9.3 是早先
checkpoint。**
其他更高版本只按协议门槛放行，未经验证（见 [ADR 0003](adr/0003-herdr-version-floor.md)）：

- wrapper 与 JVM 共用只读预检，创建 detached Viewer、终端或 companion 前执行
  `herdr --version`（必须为纯数字 `herdr X.Y.Z` 且 >= 0.9.3；预发布或带后缀
  版本拒绝）及 `herdr api schema --json`
  （bundled schema 的顶层 `protocol` 必须为 22），且平台为 WSL/Linux。
  PATH 上每个 `herdr` 都逐一核对，选出 selected Herdr（兼容者中版本最高，
  同版本取 PATH 靠前者），本次启动的所有 Herdr 调用都用其绝对路径
  （见 [ADR 0004](adr/0004-herdr-executable-selection.md)）。
  schema 是随 CLI 分发的本地数据；预检禁止为了协议核对创建 server。
  #17 创建已拥有的 server 后还须核对实际 endpoint 兼容性。
- 官方来源：[Herdr releases](https://github.com/herdrdev/herdr/releases)
  与 https://herdr.dev；安装后仍必须核对版本。
  低于 0.9.3、缺失依赖或协议不为 22（包括更高协议）均失败，给出安装指引；
  不自动下载、升级或静默回退；无兼容候选时列出每个候选的路径与拒绝原因。
- coding-agent 提供方（#194）：只有 viewer 生命周期入口（`./uml`、
  `./uml --restart`）接受 `--coding-agent claude|codex`，缺省 claude；
  `ir`、`scene`、`crap`、`mutate`、`dry` 拒绝该选项，非法值或重复选项拒绝。
  fresh 的选择经 viewer 与 worker 请求（`:coding-agent`）写入 ownership；
  restart 沿用 ownership 记录的提供方，显式选择与其冲突时在 preflight
  拒绝，早于等待旧 viewer 退出，不替换、不复制 companion。Codex 另需：
  `CODEX_BIN`（按字面路径执行，不经 shell；缺省为 PATH 上的 `codex`）存在且
  `codex --version` 为 `codex-cli X.Y.Z` >= 0.166.0（候选门槛，以真实验收为准）、
  `codex login status` 退出 0、selected Herdr >= 0.9.5（protocol 22），且
  `<selected herdr> integration status` 的 `codex:` 行为 `current`（该命令只有
  文本输出）。任一不满足都在创建 viewer、ownership 或 Herdr 资源前以真实
  原因拒绝；不安装、不登录、不改全局 Codex 配置，也不退回 Claude。Claude
  项目的 0.9.3 下限不变。Codex fresh（#195）以 preflight 核实过的可执行文件
  启动 Stable 交互式 TUI（ADR 0009），fresh 与 recovery argv 都写这一路径。
- fresh 默认 Herdr；`--backend herdr` 兼容，tmux/psmux 明确拒绝。
  restart、wake、close 必须具有显式 `:backend :herdr` ownership，身份核实后
  才操作；legacy、无 backend、未知或不完整记录保留并拒绝接管。
  WSL/Linux 可启动；Windows 原生在创建资源前拒绝（ADR 0007），
  旧 tmux 路径已退役。
  Herdr 创建项目独占的命名 server；预检本身仍不创建任何资源。

## 2. 身份与 ownership 契约

Herdr 的 pane ID 是 **session 局部** 的：两个命名 session 各自都有
`w1:p1`。因此持久化记录必须包含完整寻址链，后续操作按记录路由，
禁止按当前 OS 或默认后端重新猜测：

```clojure
{:backend :herdr
 :owner-role :companion
 :session <命名 session，如 uml-viewer-<project-hash>-<uuid>>
 :socket <session socket 绝对路径>
 :workspace <如 "w1">
 :pane <如 "w1:p1">
 :terminal-id <"term_...">
 :server-pid <独占 server 的进程 id>
 :server-start <server 进程启动时间>
 :shell-pid <pane shell pid>
 :owner-start <shell 进程启动时间，防 PID 复用>
 :cwd <规范化项目根目录>
 :command <原始命令向量>}
```

- coding-agent 提供方（#193）：Claude 记录保持上面的形状，另带
  `:recovery-command` 与 `:claude-session`，**不写** `:coding-agent`；缺少
  `:coding-agent` 的记录只读解释为 Claude，不迁移、不改写。非 Claude 提供方
  写 `:coding-agent :codex`，会话放在自己的 `:codex-session`（UUID），恢复
  命令只由已记录的会话 id 生成。其他提供方值、或已提交但没有可验证会话的
  Codex 记录一律拒绝并保留。提供方、会话字段与 fresh/recovery argv 的映射
  只在 `crates/companion/src/agent.rs`。
- Codex 会话身份（#195）：runner 确认启动后，`Herdr::create` 在提交 ownership
  前轮询 snapshot，至多 45 s，等待占据本 pane/terminal 的 agent 上报
  `agent_session`。只接受 occupant `agent` 为 `codex`，且 `agent_session` 为
  `{:agent "codex" :source "herdr:codex" :kind "id"}`、`value` 为 UUID；随后写入
  `:codex-session` 与显式 `codex resume <uuid>` 恢复命令，再提交、启动恢复
  controller，之后 worker 才发布 ready。超时、提供方或来源不符、非 id 或非
  UUID 一律失败，不退回 Claude、不选最近会话、不新建会话；此时 recovery
  controller 尚未启动，只释放本次新建的 workspace、server 与命名 session，
  确认后清空 ownership（释放失败则保留并报告）。独占 server 启动时去掉继承的
  `CODEX_THREAD_ID`，否则 Herdr Codex hook 会丢弃 companion 自己的上报。
- 创建前写 provisional 记录；部分失败后保留，禁止自动再建。
- 每次 `probe!`/`bind!`/`wake!`/`cleanup!` 之前核对：session socket 可达、
  snapshot 中存在该 workspace/pane、`pane process-info` 的 shell_pid 与
  cwd 匹配、owner-start 一致。任一不符或含糊：失败并给诊断，不创建
  替代会话、不关闭猜测目标。
- POSIX owner-start 取 `/proc/<pid>/stat` 的 starttime（Linux/WSL）。
  Windows 的进程创建时间方案随原生支持退役。
- session 名必须让 socket 路径落在实测上限内：Herdr 0.9.5 下 101 字节可用，
  102 字节起 `workspace create` 失败并只报 closed-channel。生成长度按
  `XDG_CONFIG_HOME` 实算，装不下时以真实原因拒绝（ADR 0007）。
- 双项目隔离已验证：A 的 workspace 关闭与 server 停止不影响 B。
  `cleanup!` 只作用于记录中的 session，绝不停共享/其他 session。

## 3. SessionBackend 接口映射

对现有协议（`companion.clj` 的 `SessionBackend`）逐项给出 Herdr 实现
边界：

| 接口 | Herdr 机制 | 契约与已验证语义 |
|---|---|---|
| `create!` | `herdr --session <s> server`（隔离 server）+ `workspace create --cwd <root> --no-focus` | cwd 原样保留（含空格、`&`、中文已验证）。server 启动时去掉继承的 `HERDR_*`（见 §6）。pane 内命令经 **argv 桥接** 投递（见 §4）。 |
| `probe!` | `status server` + `api snapshot` | session 存在且 socket 可达、记录中的 pane 存在于 snapshot 才算存活；超时/含糊不得视为"已不存在"。 |
| `identity!` | snapshot + `pane process-info --pane <id>` | 返回并核对上节全部字段；`terminal_id` 在 pane 存续期稳定。 |
| `bind!` | 同上，纯核对，无副作用 | Viewer 新 JVM 重启后只重新绑定记录的 pane；缺失或身份变化则拒绝启动，不创建替代。Codex（#196）另要求 occupant 仍为 `codex` 且 `agent_session` 为保存的 `:codex-session`，不执行 `codex resume`。 |
| `wake!` | `agent prompt <pane> <text>` | 投递前核对 owned pane 的提供方（缺省 Claude）occupancy；Herdr 再次校验 agent 并提交字面文本及 Enter。wake 只证明投递（§5）；邮件消费由持久队列保证，不在 wake 内等待。 |
| `recovery!` | Herdr 不提供，由项目自有 runner + 监督层实现 | 见 §6。开启/关闭恢复即注册/注销项目监督。 |
| `cleanup!` | 先注销监督，再 `workspace close <w>`，最后 `session stop` + `session delete` | 顺序固定：先撤恢复再销毁。只清理记录拥有的资源。 |

错误语义（已验证，adapter 必须原样透出）：

- pane 缺失：`pane_not_found`，exit 1；
- 输出等待超时：`timeout`，exit 1，邮件保留在队列；
- 非法按键：`invalid_key`，exit 1；
- CLI 语法错误：exit 2。

## 4. argv 与终端连接契约

- `pane run` 的命令字符串**经 shell 解析**：裸 `$HOME` 会展开。字面 argv
  必须由项目桥接（对每个参数做 POSIX shell 引号转义后拼接）。已验证可
  字面保留：空格、单引号、反引号、`$`、`&`、`;`、Unicode/中文、空串、
  尾部反斜杠。历史上 Windows 曾另行使用 PowerShell ArgumentList 桥，随
  原生 Windows 支持一并退役（ADR 0007）。
- 退出码不由 Herdr 上报，桥接器负责显式捕获（`echo EXIT=$?` 模式或
  runner 写状态文件）。已验证 exit 37/0 均可捕获。
- 终端可见 attach：`herdr session attach <name>` 打开原生 TUI；历史 macOS 证据
  （JVM 时代）已人工确认 TUI 打开、中文渲染、`Ctrl+B q` detach；输入回显尚未人工确认，
  且 detach 后 pane 与会话存活；无 TTY 时干净失败 exit 1。
  WSL 上 adapter 经 interop 调用 `wt.exe -w new new-tab … wsl.exe -d <distro> --exec
  herdr session attach <name>`（自定义 `XDG_CONFIG_HOME` 经 `/usr/bin/env` 传入）；
  不在 WSL、没有 `wt.exe` 或启动非 0 时只报告 attach 命令，不重试、不另起 companion。
  边栏设置由 attach 客户端读取，server 上设置不生效（Herdr 0.9.5 实测）。
  因此每次 attach 前，adapter 把用户自己的配置（`HERDR_CONFIG_PATH`，否则取配置根下的
  `herdr/config.toml`）复制到 `.uml-viewer/herdr-attach.toml`，在 `[ui]` 里设
  `sidebar_start_collapsed = true`、`sidebar_collapsed_mode = "hidden"`，再经
  `HERDR_CONFIG_PATH` 传给 attach 命令和报告出的手动命令。生成结果过不了
  `herdr config check` 时，不传这个变量，客户端照常读用户原配置。Windows
  Terminal 即使在引号内也按 `;` 拆分命令行，所以项目路径含 `;` 时新标签页
  不传这个变量（手动命令照传）。
  原生 Windows 的 Windows Terminal 分支随原生支持退役（ADR 0007）。
- companion 的 EDN 邮箱联动不依赖 TUI：socket/CLI 控制与面板输入在
  无 TTY 下全部可用（已验证）。

## 5. wake 效果确认

wake 投递成功或 `pane wait-output`（socket `pane.wait_for_output`）匹配输出
不能单独证明 Claude 消费输入：匹配可能来自旧快照或终端回显。
已验证 `wait-output` 会立即搜索已有快照再轮询，超时 exit 1。

wake 只证明投递，不证明消费：投递前核对 owned pane 的提供方（缺省 Claude）occupancy 与
本次唯一 token 尚未被确认，`agent prompt` exit 0 后再核对 pane 身份未变。
Claude 何时 pop 邮件由它自己的回合决定（忙时先完成手头工作），companion
不在有限窗口内等待队列推进；早期 5 秒窗口在每次真实 mail 上都误报
`consumption was not acknowledged`（se-fish 实测 pop 晚于投递约 25 秒）。
Herdr 0.9.5 实测：Claude `working` 时 `agent prompt` 仍 exit 0，文本进入
Claude 输入队列，当前回合结束后被处理；按 `agent prompt --help`，`blocked`
时以 `agent_blocked` 拒绝（未实测）。

投递以 `agent prompt --wait --until working --until blocked` 发出：未开启回合时
Herdr 返回 `agent_prompt_stalled`，wake 失败。接受时返回的 `state_change_seq`
是该次投递的基线。Herdr 0.9.5 实测：忙时排入的 prompt 并入同一段 `working`
（seq 不变），回合结束回到 `idle` 时 seq 加一。worker 每秒按
`rules::wake_verdict` 核对：邮件已出队为已消费；Claude 处于 `idle`/`done`
且 seq 大于基线、邮件仍在队列，判定为 wake 丢失并报告
`Companion wake lost`；其余（`working`、`blocked`、尚未越过基线）继续等待，
不设超时。先读 agent 状态再读队列，回合内已 pop 的邮件不会误报。唤醒失败不丢邮件（队列保留由现有 mailbox 语义保证）。

精确 occupant（#196）：wake、agent 状态读取与 Codex 的 `bind!` 只接受 ownership
提供方的 occupant；缺省 `:coding-agent` 只接受 Claude，没有「任意 agent」fallback。
Codex 另要求该 occupant 的 `agent_session` 为 `{:agent "codex" :source
"herdr:codex" :kind "id"}` 且 `value` 等于保存的 `:codex-session`；同一 pane 中后来
出现的另一会话、或不再上报会话，均以 `No Codex CLI agent occupies the owned Herdr
pane with its saved session` 拒绝，不投递。verdict 规则与提供方无关；状态文本按提供方
命名（`Companion wake lost: Codex went idle ...`、`Regen queued; Codex session not
attached.`）。

## 6. 退出检测与恢复契约（项目自有）

#19 实现采用以下有界政策，而不是无限重启：

- **runner**：pane 内启动项目拥有的 Rust runner（`uml-viewer companion-runner`）。
  runner 用 `posix_spawn` 传精确 argv/cwd 并保留实际 Claude 子进程 handle；交互
  stdin/stdout/stderr 继承 pane，状态文件原子替换记录唯一 generation token、实际
  child PID/start、退出码与时间。只接受可直接 exec 的命令：脚本 wrapper
  （`.ps1`/`.cmd`/`.bat`）会在创建资源前拒绝，因为无法证明实际 child 身份。runner
  被杀死且没有匹配的最终 exit 记录时，原因未知，不恢复。上面描述的 Java runner
  与 Windows 专属约束属历史记录。
- **controller**（历史为 JVM `FileChannel`，现由 `adapters/lock` 的 `flock` 承担）：项目独占 detached controller，经 OS 单例 lock 注册
  UUID/PID/start。Viewer 只持久化 immutable supervisor 引用；mutable phase、
  generation、child、budget、deadline、diagnostic 位于单独 `recovery.edn`，
  controller 是唯一 writer，绝不重写 companion ownership。Viewer JVM 重启
  只核对既有 controller，不重置 budget/generation，不自动采用死亡 controller。
- **政策**：初始运行之外最多 3 次重试；deadline 为 1/2/4 秒。重试计数在
  提交前增加；只有实际 child PID/start 连续存活 60 秒才重置，与邮件 ack
  无关。时钟为单调 clock；Viewer 重启不影响政策。
- **区分**：当前 generation 的匹配 exit 0 → 正常退出，不恢复；匹配非零 →
  退避；unknown token、缺失/矛盾状态、child PID/start 消失、pane/server
  死亡或提交确认丢失均 fail closed，队列保留，不猜测替代会话。
- **提交**：启动与取消共用 OS lock；提交前重核 session/socket/workspace/
  pane/shell PID/start、pane foreground 只含 shell，并先 journal 唯一
  generation。提交 ack 不确定时不得重复提交；runner generation lock 阻止
  重复 child。恢复命令显式 `--resume <原始 Claude session id>`，提示只消费
  FIFO 并返回 display，不重新扫描或发明 proposal。
- **Codex 恢复**：政策、退避、存活重置和停止规则相同，提交的只能是 ownership 里
  `codex resume <options> <saved uuid> <recovery prompt>` 形式的恢复命令：`-C` 为
  项目目录，不含 `--last`/`--all`。缺失、为空或非法时不碰 pane、不退回 `:command`，
  本次恢复以 `:failed` 停止。提交后 journal 记 `:session-check`，child 启动后
  controller 每拍核对 Herdr 的 `agent_session`：同一 UUID 清除标记；其他 UUID、
  source 或 kind 立即 `:failed`；45 秒无报告也 `:failed`。停止后 controller 不杀
  pane 里的进程，wake 的 occupant 核对同样拒绝它，邮件队列保留，等用户关闭 viewer
  清理。viewer restart 只重新绑定（§7），从不走这条路径。
- **关闭**：先写 durable cancel，经同 lock 与 controller 同步；controller
  必须确认取消且退出后才销毁 workspace/server。close 阶段在每次破坏性
  操作之前落盘，可核对并续接已关闭 workspace、已停止 server 与已删除 session；
  身份变化或含糊时失败，不影响其他项目或共享 Herdr 服务。

Windows 已记录真实 Claude 异常恢复、namespace JVM 重启与身份连续性等部分
原生结果，详见 `windows-herdr-21-acceptance.md`（历史）。WSL 上由
`scripts/test_posix_recovery_e2e.py` 经公共 `./uml` 实测 1/2/4 秒恢复、恢复后
FIFO、退避期间关闭取消、预算耗尽、正常退出不恢复，以及另一项目与无关 Herdr
会话不受影响；companion 为占位进程，证据见
`acceptance-evidence/106-wsl/recovery/`。真实 Claude 的 `--resume` 恢复与恢复后 FIFO 已由 #114 实测（`acceptance-evidence/106-wsl/real-companion/`）。
macOS 历史验收见 §8；macOS 已于 2026-10-27 恢复为交付平台（ADR 0007 增补）。

Herdr 0.9.5 在 API socket 监听后即回答 `status`，但首个 `workspace create`
要等 bootstrap 结束才返回；WSL interop 追加的 `/mnt/c` PATH 项使 bootstrap
实测 5.7 秒（空闲）至 10.2 秒（另一项目并存），因此该请求使用 60 秒上限，
其他 CLI 请求仍为 10 秒。

从 Herdr pane 里启动时，launcher 会继承这个 pane 的 `HERDR_*` 变量。Herdr 0.9.5
的 headless server 只要继承到 `HERDR_STARTUP_CWD`，就会在该目录自建一个
startup workspace，隔离 session 因此多出一个空 shell workspace。所以 companion
启动隔离 server 前要去掉继承的 `HERDR_*`，只保留用户自己选的
`HERDR_CONFIG_PATH`。CLI 调用不受影响：`--session` 优先于继承的
`HERDR_SOCKET_PATH`，pane/workspace 也都显式传 id。对照见
`acceptance-evidence/herdr-startup-workspace/`。

### Herdr 原生证据边界（#14）

- pane 内子进程正常退出与被 SIGKILL，pane shell 均存活且 shell_pid 不变，
  Herdr **不提供任何区分信号**；Claude 死亡后 agent 从列表消失
  （`agent_not_found`），无原因字段。
- socket `events.subscribe` 的 `pane.exited` 仅在 **pane 进程本身死亡**
  时推送，载荷仅 `{pane_id, workspace_id}`，无退出码；API 主动 close
  不触发；末 pane 死亡自动关闭整个 workspace。
- 同 pane 重启已验证可行：Claude 被 SIGKILL 后在原 pane 重新
  `agent start`/`pane run` 可正常交互。

因此契约如下：

- **runner**：companion 不由 shell 直接跑 Claude，而是跑项目拥有的
  runner（脚本/manifest 进程），runner supervise Claude 子进程，等待并把退出码、退出
  时间、信号信息写到项目 `.uml-viewer/` 下的状态文件。这是区分
  正常/异常退出的唯一可靠来源。
- **监督层**：项目实现有限退避重试（次数上限、退避序列、计数重置规则
  在实现时明确记录并确定性测试）。监督通过轮询 `pane process-info`
  的 foreground + 读 runner 状态文件判断退出；`pane.exited` 仅用于
  pane 整体死亡（此时 runner 也死了，视为恢复失败；重建的 ownership
  规则须由 #19 单独定义，不能猜测或自动替代身份缺失的资源）。
- **区分语义**：runner 状态文件退出码 0 → 正常退出，不恢复；主动关闭
  标记也独立禁止恢复。非零退出且未主动关闭 → 异常，进入退避重试；
  Viewer 关闭 → 先注销监督取消待重试。
- **恢复动作**：在原 pane 重新投递 runner 命令（同 pane 重启已验证）。
- 若未来 Herdr 版本提供带子进程退出码的事件，可简化 runner，但
  protocol 22 必须按上述实现。

## 7. Viewer 重启连续性

当前实现：Rust viewer 由 POSIX wrapper 按 §9.1 的 `/proc` 身份核实并等待旧进程
退出后重启，companion 与 ownership 保持不变（ADR 0002，WSL 证据见
`acceptance-evidence/106-wsl/`）。以下 JVM 措辞是该流程的历史记录，语义不变。

- `:quit-for-restart` 流程不变：保存图路径、讨论 context、视图与 mailbox
  cursor → JVM 退出 → public wrapper 核实 launcher/JVM 记录并等待旧 JVM
  退出后启动新 JVM。
- 省略重启路径时，新 JVM 恢复上次保存的图；显式路径是明确 override。讨论
  context 独立于当前 selection；不会由 proposal 推测或发明。
- 新 JVM 用 §2 的持久化记录 `bind!` 原 pane（核对 socket/workspace/
  pane/shell_pid/owner-start），不创建新 companion。记录缺失或身份
  含糊 → 明确报错，不开 viewer、不建替代会话。
- Codex（#196）：restart 只重新绑定仍在运行的原 Codex 进程，`bind!` 核对
  occupant 与保存的 UUID（§5），不执行 `codex resume`、不开第二段对话；切换提供方
  必须先正常关闭旧 companion 再 fresh start，不支持热切换。
- Herdr server 独立于 Viewer JVM 存活（headless server 模型已验证），
  companion 跨 JVM 更换存活是设计预期；restart 绑定后调用 `recovery! true`
  核实既有 controller，不重置 generation/budget、不重建 ownership。
  Windows namespace 重启已部分原生通过，proposal/detail 等缺口见验收记录；
  macOS 端到端连续性已由 spec #32 #39 原生验证（三次重启循环，companion
  身份连续、视图状态与邮箱游标恢复）。

## 8. 原生验收边界（历史）

本节是 Windows 原生与 JVM 时代 macOS 的历史验收记录，不代表当前支持。当前
交付平台为 WSL2 + WSLg 与 macOS（ADR 0007 及其增补），两者的 companion 都是交付路径；下文
"Linux/WSL companion 不受支持"一句只描述当时的状态。

Windows #21 已部分验收收尾；未验证的七项转由 [#30](https://github.com/qinyuanj/uml-viewer/issues/30) 跟踪：命名 proposal
恢复、f5ceb0b detail 窗口复验、恢复预算耗尽、正常退出不恢复、待重试关闭取消、
手动 terminal attach、focus/open-layer 与 metrics overlay 重启复验。
完整结果及证据见 `windows-herdr-21-acceptance.md`，历史结果不视为本次重跑。

macOS 端到端原生验收由 spec #32 完成并通过（#22 以验收日志关闭，Herdr 0.9.4 /
protocol 22，Darwin 27.0.0 arm64）：fresh 创建、FIFO 邮件、A/B 并行隔离、
异常退出 1/2/4s 退避至多 3 次、正常退出零恢复、close 取消待重试、wrapper
--restart 身份连续与视图恢复、质量分发串行门控均实测通过；平台终端 attach
因宿主锁屏判 NOT VERIFIED，移交 [#49](https://github.com/qinyuanj/uml-viewer/issues/49)。
验收期间修复三个 macOS 阻断缺陷（#45：sun_path 上限、登录 shell argv0、fresh
pty canonical mode），证据见 acceptance-evidence/acceptance-log.md。
低于 0.9.3 或 protocol 不为 22 的版本不接受；高于 0.9.4 的版本未验收。
Linux/WSL companion 不受支持；独立扫描/质量命令保持各自平台要求。

2026-10-16 的 #49 定向复验在解锁的 macOS、Herdr 0.9.5 / protocol 22 上，
已验证修复候选的公共 Terminal 打开、目标窗口归属、键盘聚焦及准确 session
标题；其他既有窗口保留且相对顺序不变。历史 -10006 可由对 tab 设置不支持的
`title displays settings name` 属性在解锁环境复现，不能仅归因于锁屏或权限。
证据与未验证边界见 [定向运行记录](../acceptance-evidence/issue-49-terminal-attach.md)。
这不是 0.9.5 的完整生命周期验收；中文输入、人眼视觉和 attach 客户端 detach
退出仍未验证，宿主未主动重新锁屏。修复与定向验收证据随本次变更一起交付。

#48 的同次定向检查已验证真实 JVM shutdown hook 取消恢复并释放完整 ownership
所属的 Herdr server、shell、受监督子进程及 controller；重启保留分支另有受控
JVM 证据。它不替代实际 ⌘Q / Dock 事件或恢复退避窗口的原生复验。

### 本地收尾记录 — 2026-10-12（#13 / #23）

Herdr 迁移及旧后端退役实现（604aadb）已通过 cc57110 合入本地 `lua`。
本轮静态检查确认 public wrapper/preflight、JVM fresh/restart、wake、recovery
及 close 路径均使用 Herdr；旧名称仅用于拒绝 legacy ownership/参数、对应
验证及历史证据。未发现需要补改的活跃源码，本轮仅补齐交付与历史说明。

截至本次记录，本地 `lua` 尚未同步远端，本轮文档修改尚未提交；发布未完成。
#13 / #23 的实现已在本地合入，不代表其交付、最终回归或原生验收已完成，
本轮未修改或关闭 GitHub issue。

按维护者要求，本轮不运行测试（含入口及生命周期回归）、CRAP、差分 mutation、
IR 或原生验收。合并后的最终运行验证仍待安排，历史通过结果不作为本轮结果；
仅执行 `git diff --check` 检查文档差异格式，不能据此宣称运行行为通过。
Windows #21 仍为部分验收，上述七项欠项继续由 #13 跟踪；macOS #22 是取消，
不是通过。本轮不启动或重启 Viewer，也不处理所有权未知的遗留 fixture/进程。

## 9. 旧后端退役与迁移

Herdr 是唯一后端；tmux/psmux adapter、pane 恢复 hook 和旧终端桥接均退役。
旧或未标记 backend 的 `.uml-viewer/companion.edn` 保留并拒绝使用，避免
猜测接管、误关闭或以 fresh 启动覆盖活动资源。

迁移按以下顺序进行：

1. 用旧版本正常结束该项目的旧 Viewer/session；确认该项目的 legacy 资源已退出。
2. 归档该项目的 `.uml-viewer/companion.edn`。保留 `session.edn`、policy/proposal
   和两个 mailbox 队列；不要把旧记录改标为 Herdr。
3. 在 WSL2 中安装原生 Linux Herdr >= 0.9.3（protocol 22），并通过 `./uml` fresh 创建新 companion。
   `--restart` 用于复用已验证的 Herdr 身份，不用于跨后端迁移。

新版本不会自动清理活动 legacy 资源，也不会影响其他项目或共享 Herdr 服务。
维护者已明确取消双平台完整验收作为退役前置条件；这项决策不改变未验证状态。

### 9.1 运行时身份文件与 `--restart`

Rust viewer 经 POSIX 进程 adapter 读取自身 PID 与 `/proc/<pid>/stat` 的
starttime（第 22 字段），在启动成功后写入
`.uml-viewer/viewer-process-identity.json`：

```json
{"pid":1234,"started":"5381724","runtime":"rust","cwd":"/home/user/project"}
```

- `started` 是不透明十进制文本，restart wrapper（`scripts/uml_launch.py`）只做
  精确相等比较，不解释、不设时间窗容差（ADR 0007）。Windows FILETIME 表示随
  spec #106 退役，不再写出或接受。
- #72 切换后 JVM 实现已删除；#144 切换后 LOVE/Lua 实现也已删除：wrapper
  **不再识别** `.uml-viewer/viewer-jvm.json`，并拒绝 `runtime` 为 `love` 或
  `jvm` 的旧记录。`--restart` 只验证 `rust` 记录。
- 每次新启动的 launcher 记录 `.uml-viewer/viewer-process.json` 带
  `runtime` 字段（只允许 `rust`），作为当前会话的权威来源。声明其他运行时或缺
  该字段的旧记录被明确拒绝，不猜测替代。
- 记录缺失、PID/starttime 不符或身份文件无效同样拒绝；已有身份文件保持原字节，
  不删除、不改写、不以新进程替代。
- 旧 launcher/旧 viewer 均须在 30 秒内实际退出后才启动替代进程。Rust viewer 的
  `:quit-for-restart` 先原子写入 `session.edn` 再退出；重启后恢复图路径、
  独立 discussion context、视图（focus/proposal/layer/declutter/camera/
  zoom/selection/detail）与 mailbox cursor（ADR 0002）。
- 拒绝规则由 `scripts/test_posix_restart_entries.py` 覆盖，WSL 端到端由
  `scripts/test_posix_restart_e2e.py` 实测；macOS 上 restart 拒绝规则同样通过，端到端待实测。
