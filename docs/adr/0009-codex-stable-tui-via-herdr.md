# 0009 Codex 提供方走 Stable 交互式 TUI＋Herdr integration，不走 Experimental app-server

**Status**: accepted

Spec #192 增加 Codex CLI 作为第二个 coding-agent 提供方，ADR 由 #195 写成。
本 ADR 不改变 ADR 0003/0004/0007 的 Herdr-only companion 后端，也不改变 ADR 0008 的 EDN 外部契约。

Claude Code 启动时就能接收调用方选定的会话 UUID（`--session-id`），恢复时 `--resume` 同一 id。
Codex 的交互式 TUI 没有对应参数：会话 id 在 Codex 内部生成，只能事后得知。
Codex 0.166.0 另有 `app-server` 子命令，可以经 JSON-RPC 直接创建和驱动线程，但它在 `codex --help` 中标为 experimental。

## 决定

- companion 运行用户自己安装并登录的 Codex **Stable 交互式 TUI**，沿用 ADR 0007 的进程契约：
  字面 argv、显式环境、固定项目 cwd，由本项目的 runner 在 Herdr pane 中直接 exec。
  无人值守授权只来自这一进程的启动参数（`--dangerously-bypass-approvals-and-sandbox`、
  `--enable hooks`、`--no-daemon`、`-C <root>`、`-c developer_instructions=...`），
  不读写用户全局 Codex 配置。
- 会话身份来自 **Herdr Codex integration**：Codex 的 SessionStart hook 把会话 id 报给 Herdr，
  snapshot 中占据 pane 的 agent 带 `agent_session {:agent "codex" :source "herdr:codex" :kind "id" :value <uuid>}`。
  fresh 启动在提交 ownership 前等待并验证它，写入 `:codex-session` 和显式 `codex resume <uuid>` 恢复命令，之后才 ready。
  等不到或不符即启动失败，只释放本次新建资源；不退回 Claude，不选最近会话，不新建会话兜底。
- 生产路径不使用 `codex app-server`、`exec` 或 `queue`；也不用 `herdr agent start`，
  因为 runner 自己记录的退出码是恢复判定的证据（契约 §6）。

## 代价

- 身份要等 hook 上报，fresh 启动多一段有界等待（45 s，位于 worker 120 s 启动上限内）。
- 正确性依赖 hook 链路：Herdr integration 必须是 `current`（preflight 检查），hook 必须被信任，
  `python3` 在 pane 的 PATH 上，且环境里没有继承来的其他会话的 `CODEX_THREAD_ID`（server 启动时去掉）。
  任一环节断开，表现为身份超时，按上面的规则失败。

## Considered Options

- **Experimental app-server**：可以拿到确定的线程 id，不必等 hook，但接口随版本变化不受稳定性承诺约束，
  而且绕过了用户在终端里能看到、能接管的 TUI。待 app-server 稳定后可以重新评估。
- **按 cwd 或时间挑最近会话**（`codex resume --last`、扫描 `~/.codex/sessions`）：同一项目里
  用户自己的 Codex 会话会被误认作 companion，违背只操作已核实所有权的原则。
- **`codex exec` 非交互模式**：每封 mail 都要另起一个进程，失去常驻会话，也不符合 Herdr wake 的交互模型。
