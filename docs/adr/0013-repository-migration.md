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
