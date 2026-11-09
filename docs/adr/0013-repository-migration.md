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

## 附录（2026-11-09，#21 落地）：客户端只用宿主注入的 React

ADR 0011 说面板由宿主注入 `dsh-client-ui-primitives`。实际落地时
`DSH` 的 `practices.md` 明确禁止 `require('dsh-client-ui-primitives')`：
它不是稳定 API，只是模块表里的冻结项。因此 #21 的面板：

- 只从平台模块表取 **React 18**（`require('react')`），构建产物里不带
  React、`react-dom` 或 `dsh-client-ui-primitives`（`packages/client/tests/build.test.ts`
  在产物上断言）；`dsh.client.external` 为空。
- 复制所需的骨架标记与类名，样式用宿主主题 token（`--dsw-alias-*`）写成
  普通 CSS；Canvas 2D 不能读 `var()`，所以每次重绘用 `getComputedStyle`
  读一次 token 快照，并在 `body[data-ds-dark-theme]` 变化时重绘。
- 质量色与违规关系色的**色相固定**，只随亮/暗改变明度：两套字面色板
  （`DARK_HUES`/`LIGHT_HUES`）按 HLS 只动 L，中间等级按 `frame::theme` 的
  sRGB 线性混合插值，色相偏差个位数度。

代价是面板不能复用宿主组件的行为（焦点、键盘、无障碍），换来的是不依赖
未公开 API；若 DSH 将来公开 primitives，应优先改用公开组件。其余部分（
右侧栏 tab、`main` 页、Remote `watch`/`action`、会话绑定）仍按 ADR 0011
的宿主接缝实现。

## 附录（2026-11-09，#22 落地）：浏览器验收改动的两处生产语义

#22 的 Playwright lane 第一次把面板挂进真实 DSH Web，于是顺带修掉两处此前只有
单元测试覆盖、其实不满足宿主契约的生产语义：

- **`ViewerService.typertRemote` 绑定**：面板的 typed Remote 是手写描述符
  （`UML_VIEWER_REMOTE`，经 `typert.register` 注册），`ViewerService` 不继承
  `TypertRemoteService`。Gateway 在每次调用前都要求接收者带一个可见的
  `typertRemote = { service, serviceKey: 'umlViewer', namespace: 'umlViewer' }`，
  否则以 `gateway/binding-invalid` 拒绝；源码模式（SRC）的端点发现也只枚举带该
  绑定的 Service。缺了它，浏览器的 `remote.$mount()` 拿不到 `remote.umlViewer`，
  面板永远停在 `connecting`。同一轮还要求严格 lookup 描述符的 `typeSymbol` 用
  provider 真正的 wire symbol（`@deepseek-ai/dsh-session/types#SessionId`），
  否则 `agent` 查找以 `gateway/provider-mismatch` 拒绝。
- **`ResidentSessions.refresh(..., restoreContext)`**：`display` 会重新生成 IR 并
  让 resident 回到源图，而会话持久化的讨论上下文可能仍是某个提案。只转发
  `display` 会让“看到的图”与“会话在讨论的上下文”分叉，下一次保存还会把上下文写回
  源图。因此 `display` 传 `restoreContext`：先 `context` 切回持久化的提案，再
  `reload` 取回该上下文下的帧。同理，`open` 把状态里的 `context` 记进
  `latestContext`，`watch` 在 `ready` 之后先推一个 `context` 事件，浏览器才不会
  从源图开始画。

两处都只是让会话状态、resident 与面板三者一致，没有改协议：`context` 事件与
`context` 动作原本就在 ADR 0011 的面板接缝里。面板为此多了一个隐藏的
`role="status"` 摘要（`.dsh-uml-viewer-summary`），它只镜像帧与上下文，不是新的
状态来源；验收断言读它，而不是 Canvas 内部。

## 附录（2026-11-09，#18 落地）：旧入口删除，刷新改用二进制子命令

- 删除 `crates/companion`（Herdr 后端、Claude Code/Codex 提供方、恢复控制器、归属记录、
  邮件与提示词）、`companion-*` 子命令、`./uml` / `uml.cmd`、`get-uml-viewer(.cmd)` 与
  `scripts/uml_*.py` 及其测试套件。egui 窗口只剩 `uml-viewer viewer [diagram.edn]`：
  单独运行、热重载，没有邮件、Regen、元素菜单、进程身份或 `--restart`。
- companion 规则（串行质量调用、IR 与展示门控、隐藏、提案、就地编辑 policy、讨论上下文）
  移入 [docs/companion-rules.md](../companion-rules.md)；插件提示词段落构建时原样嵌入
  该文档，二者只有一个来源。
- 维护者的质量刷新从此在 Linux 与原生 Windows 上都用二进制：`uml-viewer quality crap`、
  对改动文件的差分 `uml-viewer quality mutate`、（可选）`uml-viewer quality dry`，然后
  `uml-viewer ir examples/uml-viewer.policy.edn`。`.uml-viewer/` 中的冻结工具不再使用。

## 附录（2026-11-09，#23）：导航、会话恢复与展示门控

- **交互边界保持不变**：双击下钻、返回、P、选中依赖链、搜索与定位都经
  typed Remote 到 host，再由同一 resident 处理。平移、Ctrl+滚轮光标锚定缩放、
  普通滚轮平移、悬停与 Ctrl+0 fit 在客户端完成，相机动作只用于保存。
  Ctrl+F 打开搜索，输入法输入与粘贴使用原生 search input；搜索开启时捕获画布
  快捷键，Down/Up 显式选择，Enter 定位，Escape 只关闭搜索。
- **全宽页仍属于原会话**：`main` 是根插槽，不带 `sessionId`。Expand 在调用
  `layout.selectPanel('dsh-uml-viewer')` 之前记录侧栏会话 id，全宽页只使用该 id；
  返回 Conversation 调用 `layout.selectPanel(null)`。会话 id 另存一份在浏览器
  sessionStorage 中，让全宽页自身刷新时仍知道绑定；视图与讨论上下文仍以 host 为准。
  切换呈现不重新生成 IR。
- **随会话持久化**：host 在改变视图的动作完成后请求 resident `save`，在插件卸载
  （DSH 退出）时也保存，以每会话一个原子 JSON 文件保存在插件 home 的 `sessions/`。
  不依赖浏览器 localStorage，也不修改项目文件或宿主会话日志。已展示的状态优先于
  `open waiting:true`，恢复 focus、camera、selection、detail 与讨论上下文；从未
  展示过的等待画布保存 `diagram:null`，重开仍等待。
- **IR→display 门控刻意不恢复**：`uml_display` 仍要求本次 host 进程、同一会话内
  最新一次 `uml_ir` 成功。持久化的视图不是 IR 成功凭据，不能恢复这项授权，也不能
  从渲染后的工具文本推断授权。拒绝与失败会撤销当前进程内的授权，fork 不继承。
- **恢复是重放已展示文件，不是新展示授权**：视图保存携带已加载 IR 文件字节的
  SHA-256：resident 从解析成功的同一 UTF-8 字节缓冲生成
  `loadedDiagram: {version: 1, sha256}`（session-state v1 的加法元数据），
  初次加载、display 和成功热重载均更新；host 保存时只传递该值，不重新读取文件。
  重开或 DSH
  重启时，文件摘要相同才自动恢复；摘要不同或文件已丢失时显示等待画布，保留上次
  快照，须重新运行 `uml_ir`、`uml_display` 才展示新文件。旧版没有摘要的状态同样
  先等待；一次成功展示后保存摘要，后续即可自动恢复。热重载仅在已打开 resident 的
  生命周期内工作，沿用原协议。resident 在重开解析后再次核对该指纹，避免 host
  比较后文件又被替换的竞态；缺少 loadedDiagram 的旧二进制状态也先等待。

验证接缝：真实二进制 resident 的 waiting+state 恢复与首次 display；Cordis 工具面
的每动作保存、卸载保存；SessionStates 对相同、修改、删除 IR 的重开行为；#22 的
隔离 Playwright lane 验证导航、搜索、全宽页绑定、重开与 DSH 重启。

## 附录（2026-11-09，#26）：质量叠加、依赖链高亮与本地悬停

- **依赖链高亮色相固定**：`dependency-chain` 与质量色、违规关系色、无数据一样，
  使用 `theme.ts` 的字面色板（暗 `#3e98ec`，即 egui 的 `frame::theme` RELATED；
  亮 `#136ec3`，同色相、只降明度），不再读 `--dsw-alias-state-business-primary`。
  默认暗色 token 中这个 token 是金色，皮肤（如 dsh-claude-style）又会改成别的强调色，
  两种情况都与 egui 窗口不一致。界面骨架色仍随宿主 token。
- **悬停在客户端完成，依赖链不在客户端计算**：选中、依赖链与淡化仍只来自 resident 帧。
  `packages/client/src/hover.ts` 按 `scene::hit::at` 的顺序在帧几何上做悬停命中，
  再按 `frame::commands::edge_style` 的悬停分支改样式：与悬停类相连或指针下的关系
  变为 `relation-hover`，违规关系变为 `violation-hot`、宽 3.2，悬停的关系不淡化，
  闭合箭头仍用生产者给的不透明填充；概览层级下用弹窗显示悬停类的类名。
  违规与否只读生产者已给的角色，不在客户端求依赖规则。
- **已知差异**：帧里的关系线不带 `deps`，所以悬停关系时不显示依赖列表弹窗；
  经过类（`via-ids`）的捆绑关系也不会因悬停这些中间类而变亮。补齐这两点需要在
  帧里增加字段并重新生成 goldens，留给后续工单。
