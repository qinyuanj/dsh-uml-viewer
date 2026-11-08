# 0011 以 DeepSeek Harness 插件交付

**Status**: accepted

本 ADR 取代 [ADR 0003](0003-herdr-version-floor.md)、[ADR 0004](0004-herdr-executable-selection.md)、
[ADR 0009](0009-codex-stable-tui-via-herdr.md)，以及 [ADR 0007](0007-posix-platforms-drop-windows-native.md)、
[ADR 0008](0008-rust-egui-viewer-drop-lua.md)、[ADR 0010](0010-native-windows-delivery-platform.md)
中关于原生窗口、交付平台与 Herdr companion 的部分。ADR 0008 的 Rust 运行时与场景核心、
ADR 0010 的快照与 IR 跨平台字节一致、mutator 的分平台隔离继续有效。

## 背景

`get-uml-viewer` 把 uml-viewer 与三个质量工具逐项目 clone 并就地构建，用户需要 Rust
工具链、Python、git 与 Herdr。companion 是 Herdr 托管的外部 Claude Code / Codex 进程，
图在独立的 egui 窗口里，两者靠项目内的邮件文件通信；启动、身份校验、崩溃恢复与终端
attach 各平台各有一套。DeepSeek Harness（DSH）已经提供会话、消息队列、会话持久化、
沙箱、工具注册、Web 界面插槽与插件分发。

## 决定

- **发行**：整套工具作为 DSH 插件 `dsh-uml-viewer` 发布，是**机器安装**的唯一来源；
  安装、更新与卸载走 DSH 插件管理。全部包共用一个 SemVer，从 `1.0.0` 开始。
- **包布局**（参考 dsh-ops 与 dsh-claude-style）：`crates/` 为 Cargo workspace；
  `packages/host`、`packages/client`、`packages/contracts` 为不单独发布的 TypeScript
  工作区包，构建合成主包 `dsh-uml-viewer`（含 Sarasa 字体）。只有平台二进制包
  `@dsh-uml-viewer/{win32-x64,linux-x64,darwin-arm64,darwin-x64}` 单独发布，作为主包
  的 `optionalDependencies`，由 CI 交叉编译并以 npm provenance 发布。
- **二进制定位**：插件加载时把当前版本的二进制复制到 `<DSH_HOME>/dsh-uml-viewer/<version>/`
  并记录当前版本；全新启动才切换版本，无进程使用的旧版本才清理。Windows 不能覆盖运行中
  的 exe，运行中的会话也不换版本。
- **companion**：面板所绑定的 DSH 会话即 companion。删除 Herdr、Claude Code / Codex
  提供方、companion 归属记录、恢复控制器、`--restart` 与 `:quit-for-restart`。
- **viewer 面板**：viewer 是会话右侧边栏的面板（可展开为 `main` 页面），绑定当前会话，
  被查看的项目即该会话的工作目录。host 端原生二进制计算场景并流式推送；客户端以
  Canvas 2D 画图，检查器、类卡片与源码窗口用宿主提供的 React 18 与
  `dsh-client-ui-primitives`。颜色取自宿主主题 token；质量色与违规红只随明暗适配，
  不改色相。egui 原生窗口在功能对齐后删除。
- **面板请求代替邮件**：面板操作以 `session.prompt` 投递为对话中可见的消息，FIFO 由
  DSH 消息队列保证；删除 to-companion / to-viewer 邮件文件。展示由插件工具推送给面板。
- **工具**：插件注册 `uml_ir`、`uml_crap`、`uml_mutate`、`uml_dry`、`uml_display`，经
  `ctx.subprocess` 运行二进制，遵从会话沙箱策略，只读模式下明确拒绝、不自行提权。
- **companion 规则**：由插件以 `systemPrompt.section` 提供，只对打开了面板的会话生效；
  被查看项目的 AGENTS.md 不再需要 uml-viewer 规则。
- **状态**：视图状态与讨论上下文随 DSH 会话保存；项目内只留 policy、提案与 `.metrics/`。
- **前提条件**：DSH 宿主与 git。不再需要 Rust 工具链、Python、Herdr。
- **交付平台**：运行 DSH 的 Windows x64、Linux x64、macOS arm64 与 macOS x64。
- **过渡**：插件不读、不迁移、不删除旧的 `.uml-viewer/`。`get-uml-viewer` 保留一个
  发行版本周期，只提示关闭旧 viewer 与 companion、可手动删除 `.uml-viewer/`，并给出
  插件安装命令；之后删除。

## 被否决的方案

- **插件只做分发、保留 Herdr companion**：仍要维护各平台的启动、身份与恢复，DSH
  已有的会话能力闲置。
- **独立 `main` 页面**（dsh-media-desk、dsh-guizang-design 的形式）：图与对话分处两页，
  失去边看边讨论。
- **eframe 编译为 wasm 嵌入 canvas**：改动最少，但不随宿主主题，与皮肤插件不协调。
- **场景核心编译为 wasm 在浏览器运行**：IR、扫描器与质量工具本就要在 host 原生运行，
  多维护一套 wasm 构建。
- **写入输入框草稿再提交**（dsh-claude-style 的编辑消息做法）：会覆盖用户正在输入的
  草稿。
- **`./uml` 直接指向 `node_modules` 内的二进制**：插件升级在 Windows 上会撞上运行中
  的 exe。
