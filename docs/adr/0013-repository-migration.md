# 0013 开发迁入 dsh-uml-viewer 仓库

**Status**: accepted

本 ADR 修订 [ADR 0011](0011-dsh-plugin-delivery.md) 的“过渡”一条，以及 spec
qinyuanj/uml-viewer#203 的推进顺序；ADR 0011 与 [ADR 0012](0012-quality-tools-in-rust.md)
的其余决定不变。

## 背景

ADR 0011 与 ADR 0012 写于 qinyuanj/uml-viewer，其中的包布局、“本仓库”和单仓库、单版本号
（spec 的 Story 52）都默认插件在 uml-viewer 里逐步长出来。实际做法是新建
qinyuanj/dsh-uml-viewer 承接全部插件开发，uml-viewer 冻结在现有的 `get-uml-viewer` /
`./uml` 流程上。两个仓库的 issue 编号各自从 1 开始，历史里的 `#N` 不再有唯一含义。

## 决定

- **唯一开发仓库**：dsh-uml-viewer 是开发 `dsh-uml-viewer` 插件的唯一仓库。ADR 0011 的包
  布局（`crates/`、`packages/host`、`packages/client`、`packages/contracts`）与 ADR 0012 中
  的“本仓库”都指 dsh-uml-viewer。三个质量工具与 viewer 共用一个仓库、一个版本号和一套
  CI（Story 52）由本仓库满足；uml-viewer 不再发行新版本。
- **推进顺序的偏离**：spec 的“第 5 步之前，旧的 `./uml` 流程保持可用”改为：旧流程留在
  冻结的 uml-viewer 中一直可用；本仓库只要维护者能用二进制子命令（`uml-viewer crap`、
  `mutate`、`dry`、`ir`）刷新本仓库的质量数据，就可以先于第 5 步删除旧流程（#18）。
  在那之前，本仓库的质量刷新走 `.uml-viewer/` 中冻结的 uml-viewer 与 Python 工具。Story 56
  “随时都能刷新质量数据”由此仍成立：删除前用冻结版，删除后用子命令。
- **迁移指引**：`get-uml-viewer` 改为只输出迁移指引、并保留一个发行版本周期，这件事在
  uml-viewer 仓库执行（qinyuanj/uml-viewer#204）。本仓库不经过迁移指引阶段：旧入口在
  本仓库直接删除。
- **`#N` 引用约定**：历史 commit、代码注释与 ADR 0001–0012 中的裸 `#N` 均指
  `qinyuanj/uml-viewer#N`，不改写。从本 ADR 起，裸 `#N` 指 `qinyuanj/dsh-uml-viewer#N`，
  任何跨仓库引用一律写全名 `owner/repo#N`。tracker 配置见
  [docs/agents/issue-tracker.md](../agents/issue-tracker.md)。
- **TypeScript 工作区包进入 policy**：`packages/host`、`packages/client`、
  `packages/contracts` 各自以一个 `:lang :typescript` 源加入
  `examples/uml-viewer.policy.edn`，`:root` 为 `packages/<pkg>/src`，`:prefix` 为
  `uml.<pkg>`，与每个 crate 一个源的做法一致。哪张票新建该包，就由哪张票加入它的源和
  `:levels` 位置（`contracts` 与最内层同级，`host`、`client` 在外层）；宿主注入的库
  （如 `react`）按需列入 `:foreign`。在包存在之前 policy 不预先列出它。

## TypeScript 包进入 policy 的理由

- host 端承担工具门控、串行执行和沙箱拒绝（spec 接缝 ②），客户端承担面板请求投递，
  这些是插件的业务代码，不是胶水。排除它们会让本仓库的图只剩二进制一半，正好漏掉
  1.0.0 之后最常改动的部分。
- TypeScript 本就是受支持的扫描器与质量语言（ADR 0012 的语言集合），多源 policy 已经
  按源声明 `:lang`，不需要新机制，也不改 policy 或 IR 格式。已验证：顶层 `:lang :rust`
  加一个 `:lang :typescript` 源的 policy 经 `uml-viewer ir` 生成的 IR 同时含两种语言的
  类及 TypeScript 模块间的依赖边（`crates/ir` 的 `scan_policy` 逐源取 `:lang` 后合并）。
- 缺少 CRAP 或 mutation 数据的类显示为红色、不隐藏；TypeScript 包没测量时在图上变红，
  正是这条规则要暴露的质量缺口。明确排除等于对一半代码长期隐藏这一缺口。
- 每个包一个源、前缀 `uml.<pkg>`，与 crate 的约定一致；`host`、`client`、`contracts`
  不与任何 crate 名冲突，叠加按 `:ns` 连接不会混淆。

## 被否决的方案

- **在 uml-viewer 中逐步替换**：新旧流程在同一仓库并存，迁移步骤与冻结流程的修补互相
  牵制；issue 和发布也难以区分“旧安装器”与“插件”。
- **把 ADR 0001–0012 与历史注释里的 `#N` 改写为全名**：改写历史 commit 不可行，只改文档
  又会让文档与 commit 不一致；一条约定即可消歧。
- **本仓库也保留一个发行版本周期的迁移指引**：本仓库从未以 `get-uml-viewer` 发行，
  没有需要迁移的用户。
- **在 policy 中明确排除 TypeScript 包**：图与质量叠加不覆盖插件的 host / client 逻辑，
  违背“无数据显示为红、不隐藏”。
- **把三个 TypeScript 包合成一个源**：`contracts` 被 host 和 client 共同依赖，合并后看不到
  这条依赖方向，也与每个 crate 一个源的约定不一致。

## 附录：交互边界（2026-11-09，#17）

viewer 会话（`crates/viewer-session`）不再依赖 companion 与邮件，二进制以 `uml-viewer
resident` 长驻：stdin 收动作，stdout 发场景帧与事件，均为逐行 JSON，协议见
[docs/resident-protocol.md](../resident-protocol.md)，帧见
[docs/scene-frame.md](../scene-frame.md)。面板（#21–#27）与二进制之间的分工如下。

### 决定

- **客户端独立完成**：平移、缩放、悬停命中。场景帧带着世界坐标的全部图元、各自的
  细节层级（`lod`）与包围盒，客户端在本地变换相机、按层级与视口裁剪、对 `world` 图元
  做悬停命中并画出悬停反馈，不往返 host。相机变化以 `camera` 动作告知会话，只为保存
  视图状态，二进制不回帧。
- **往返 host**：下钻（`drill-in`、`back`、**P** 即 `namespace-tree`）、选中
  （`select`，帧里带出**依赖链高亮**与**关系淡化**）、搜索（`search` 与 `locate`）、
  切换**讨论上下文**（`context`）、展示（`display`）、箭头模式（`declutter`）。这些
  要重排版面、沿依赖边求闭包或读 IR 全文，结果是一张新帧（或搜索结果）。
- **讨论上下文**：`context` 动作先回 `context` 事件、再回新帧；host 把该事件变成
  **面板请求**（`session.prompt`），会话本身不写邮件。
- **状态归宿**：视图状态与讨论上下文由 `save` 以数据形式交给 host
  （`dsh-uml-viewer/session-state` 版本 1），随 DSH 会话保存；`open` 带回即恢复。
  项目内不再写会话快照。
- **热重载**：二进制每 250 ms 检查 IR 与 `.metrics/` 快照，变化时主动推一帧
  （`seq` 为 null）。
- **egui 窗口**：过渡期仍是同一 viewer 会话的另一个外壳；companion 链接、邮件、
  进程身份、`--restart` 与正常关闭只留在窗口的适配层（`viewer::shell_session`），
  由 #18 删除。没有 companion 时窗口单独运行并热重载 EDN（driver `standalone`）。

### 理由

- 平移、缩放、悬停每秒几十次，往返 host 与子进程会让指针跟手性取决于管道延迟；它们只
  改相机或临时反馈，不改语义状态，客户端已有绘制所需的全部几何。
- 下钻与选中要重新编译视图（`scene::document::compile_view`）或沿出向依赖边求闭包，
  搜索要读整份 IR；这些是场景核心的纯逻辑，放在 Rust 一处，面板与 egui 窗口的行为才
  一致（spec 接缝 ③ 对照 egui 窗口验收）。
- 依赖链与淡化随选中写入帧（`selected`、`dimmed`、`dependency-chain` 色角色），客户端
  不需要知道依赖规则。

### 被否决的方案

- **悬停也往返 host**：每次指针移动都要经子进程回一整帧 JSON，帧随图的大小增长。
- **客户端在 TypeScript 里重写下钻与依赖链**：同一逻辑两份实现，迟早分叉。
- **会话状态仍写 `.uml-viewer/session.edn`**：与“项目内只留 policy、提案与 `.metrics/`”
  （ADR 0011）冲突，也无法区分同一项目的多个会话。
