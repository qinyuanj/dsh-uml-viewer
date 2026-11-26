# UML viewer

本表定义项目的受控词汇。每条以英文 canonical term 开头，括号内为中文受控用语，两者同形时只写一次；代码、测试、Issue 与提案使用英文，中文 UI 使用中文。_Avoid_ 只记录仍易混淆的叫法。

## 架构与 IR

**policy**: 生成 IR 所依据的架构意图：源码命名空间结构，以及可选的外来包、架构级别、依赖边调整和提案。

**IR**: 供 viewer 消费的图拓扑，由命名空间树、类和关系组成；质量测量由质量叠加提供。

**scanner（扫描器）**: 把一种语言的源码模块及关系识别为 IR 类和关系的语言视角；架构分组由 policy 定义。
_Avoid_: parser, 语言图

**namespace tree（命名空间树）**: 按源码命名空间嵌套形成的结构；policy 前缀之后的每个点分隔段都是一级。

**component（组件）**: 图中的命名空间分组，或提案中的具名分组；**Layer** 是其同义词，不是另一种结构。
_Avoid_: 包（指命名空间分组时）

**source diagram（源码图）**: 按源码命名空间嵌套得到的图，不含提案分组。
_Avoid_: real diagram, 真实图

**proposal（提案）**: policy 中不存在于源码里的具名分组，与源码图并列。
_Avoid_: 方案

**foreign package（外来包）**: policy 声明的项目外库，在图中表示为 **foreign package ellipse（外来包椭圆）**；未声明的外部依赖不上图。椭圆不承载项目类质量或无数据标记，当前视图之外的项目代码使用依赖端口。

**architectural level（架构级别）**: Clean Architecture 的内／外层级，用于判断依赖方向；命名空间嵌套深度和视觉排列顺序都不代表它。

**violating dependency（违规依赖）**: 从较高架构级别（内层）指向较低架构级别（外层）的依赖；同级别依赖不违规。

**Dependency Rule（依赖规则）**: Clean Architecture 的规则，要求源码依赖只指向内层；违反它的依赖是违规依赖。

**Common Closure Principle (CCP)（共同闭包原则）**: 将因同一原因、在同一时间变化的模块归入同一组件的划分准则；是否采用由用户决定。

## 图与交互

**class box（类框）** 与 **component box（组件框）**: 项目源码模块或类的矩形，以及可下钻组件的矩形。模块兼具命名空间父节点时，同一框仍可从模块行打开类卡片。

**stereotype marker（stereotype 标记）**: 类框中的 `«…»` 类别标记。**α（抽象类标记）** 和 **I（接口标记）** 分别补充标识抽象类与接口，均不改变矩形轮廓。

**level number（架构级别编号）**: 类框和类卡片上的架构级别数字，0 为最内层，数值越大越靠外。

**dependency port（依赖端口）**: 类框上、下方的药丸，分别表示当前视图之外的传入和传出依赖；外来包不使用端口。

**declutter mode（精简模式）**: 面板「更多」菜单中一组互斥的图展示模式，按 `full`、`arrows`、`triangles`、`elements`、`classes` 排列。`triangles` 的 UI 文案为「移除箭头」，使用 **compact dependency indicator（精简依赖标记）**。
_Avoid_: 箭头模式, 精简依赖标记（指整个模式组时）

**compact dependency indicator（精简依赖标记）**: 「移除箭头」模式下类框上方或下方的三角，分别表示被隐藏的传入或传出依赖。

**relation line（关系连线）**: 六种关系的实线；头部形状表示关系种类，方向如下。绘制细节见 [scene-frame](docs/scene-frame.md#edges)。

| Canonical term | 定义 | 头部 |
| --- | --- | --- |
| **inheritance（继承）** | 一个类特化另一个类 | 空心三角指向父类 |
| **implementation（实现）** | 一个类或模块提供接口或抽象契约 | 空心三角指向契约 |
| **association（关联）** | 两个类之间的结构性联系，不参与依赖规则违规判断 | 开放箭头 |
| **dependency（依赖）** | 一个类使用或依靠另一个类 | 开放箭头 |
| **aggregation（聚合）** | 整体与部分的关系，部分可独立于整体存在 | 整体一端的空心菱形 |
| **composition（组合）** | 整体拥有部分并控制其生命周期的关系 | 整体一端的实心菱形 |

以下关系颜色与交互样式只表达依赖规则及交互状态，不表达质量测量：

| Canonical term | 定义 |
| --- | --- |
| **normal relation color（普通关系色）** | 未违规且未高亮的关系所用的灰绿色。 |
| **violating relation color（违规关系色）** | 违规依赖及其折叠或精简标记所用的红色；悬停或选中时使用亮红。 |
| **dependency chain（依赖链）** | 从选中类沿出向依赖可达的全部类及依赖边，遇环停止回溯。 |
| **dependency chain highlight（依赖链高亮）** | 依赖链的普通关系与相关框线的蓝色高亮；链上的违规依赖仍为红色。 |
| **relation hover highlight（关系悬停高亮）** | 悬停关系或相连类时的临时高亮，不改变选中状态。 |
| **dimming（关系淡化）** | 选中类后，依赖链之外且未悬停的关系和图元素的淡化；元素仍保持原语义。 |

**class card（类卡片）**: 一个类的成员、关系与所属包的卡片，同一时刻只显示一个。**member visibility（成员可见性）** 用 `+` 和 `-` 分别表示公开与私有成员。

**source window（源码窗口）**: 从类卡片成员打开并取代卡片的源码视图，定位到 **declaration line（定位行）** 即成员声明行，并可返回类卡片。

**hide（隐藏）**: 把元素排除在当前源码图或提案之外的操作。
_Avoid_: omit（作为 UI 用语时）

## 质量

**quality overlay（质量叠加）**: 按命名空间和类把 CRAP、coverage 与 mutation 快照关联到 IR 后形成的质量视图；重复结构是独立的附加标记。

**quality color（质量色）**: 类或组件的红—绿填充，表示 CRAP 风险与 mutation 得分的综合评价；组件聚合其成员质量。合成规则见 [README](README.md#ir)，视觉样式见 [scene-frame](docs/scene-frame.md#colors-roles)。

**CRAP**: Change Risk Anti-Patterns，结合圈复杂度与测试覆盖不足程度的风险分数，越低越好。**C（CRAP 质量圆点）** 表示其综合风险；类卡片的 `max` 是最差函数的 CRAP，不是总和。

**μ（函数 CRAP 平均值）**: 一个类或组件内各函数 CRAP 的平均值；组件按全部底层函数汇总，不对成员平均值再平均。

**σ（函数 CRAP 标准差）**: 一个类或组件内各函数 CRAP 的离散程度；μ 相同时，较大的 σ 对应更高的 C 风险。

**CC（圈复杂度）**: Cyclomatic Complexity，表示控制流分支的复杂度。

**Cov（行覆盖率）**: 类卡片中的行覆盖率，表示被测试执行到的源码行比例。

**M（mutation 质量圆点）**: 由 killed、survived 和 uncovered 共同得出的 mutation 质量指标，越高越好；计算与展示规则见 [README](README.md#ir)。

| Canonical term | 定义 |
| --- | --- |
| **killed（杀死数）** | 被测试判失败的变异体数。 |
| **survived（存活数）** | 有测试覆盖、但测试未能判失败的变异体数。 |
| **uncovered（未覆盖数）** | 没有测试覆盖的变异体数，表示覆盖缺口。 |

**no mutation sites（无 mutation 位点）**: 某行没有变异位点且三类变异体计数均为零的标记；缺少快照也会产生零计数，因此它不能证明没有可应用的 mutation 算子。

**no data（无数据）**: 类缺少 CRAP 或 mutation 至少一项输入的状态；类框按最差质量等级红色填充并显示无数据，缺失指标对应的 C/M 圆点为红，轮廓另用虚线。

**duplicate structure（重复结构）**: dryer 报告的函数两两相似配对，作为质量标记之外的附加信息；它不影响 CRAP、mutation 或质量色，未匹配到类的配对仍计入配对数。

**duplicate cluster（重复簇）**: 以重复结构配对为边、函数区间为点形成的连通分量；簇相似度取其中最弱的一条边。

**unmeasured（未测量）**: 重复结构快照缺席或未关联到某类的状态，不表示没有重复，也不改变质量色。
_Avoid_: no data（用于重复结构时）

**measured scope（测量范围）**: 三个质量工具共享、由项目质量配置声明的待测源码集合；难以测试的源码仍可属于范围。配置与分发规则见 [quality-dispatch](docs/quality-dispatch.md)。
_Avoid_: business scope, 业务范围

**copy-outside path (`--copy`)（副本外路径）**: 调用方声明的项目根之外、需要让 Windows mutation 副本访问的源路径；放置与校验规则见 [ADR 0016](docs/adr/0016-copy-outside-paths.md) 和 [mutation-windows](docs/mutation-windows.md#paths-outside-the-project-root---copy)。

**quality dispatch（质量分发）**: viewer 按项目配置调用 CRAP、mutation 或重复检测标准工具的分发；失败会阻止后续 IR 与展示。协议见 [quality-dispatch](docs/quality-dispatch.md)。

## 协作与运行时

**discussion context（讨论上下文）**: 当前与 companion 讨论的源码图或提案，只有选择另一张图时才改变。

**companion**: viewer 面板绑定的 DeepSeek Harness 会话，按到达顺序处理面板请求并驱动质量与图展示。

**viewer panel（viewer 面板）**: 与 companion 对话并排显示图的界面区域；一个面板属于一个会话，被查看项目是该会话的工作目录。

**panel request（面板请求）**: viewer 面板操作以对话中可见的消息交给 companion 的请求。

**request status（请求状态）**: 面板请求在 UI 中的进展，包括已排队、进行中、完成或失败；状态取自准入及实际执行结果。协议见 [plugin-development](docs/plugin-development.md#request-status-51-52-adr-0015)。

**selection action bar（选中操作条）**: 选中类或组件后出现的操作集合，提供适用的面板请求、类卡片与下钻入口。

**breadcrumb（面包屑）**: 面板顶栏从树根到当前组件的可点击路径；提案路径从提案本身开始。

**status code（状态码）**: resident 给出的画布状态枚举，客户端据此生成本地化显示文字。协议见 [resident-protocol](docs/resident-protocol.md#breadcrumbs-and-status-codes-45-additive-version-1)。

**discussion-context switcher（讨论上下文切换器）**: 列出源码图与各提案，并支持选择或新建提案的控件。

**scene core（场景核心）**: viewer 中把 IR 与质量快照转换为可绘制场景的部分，不包含绘制、窗口与输入。

**viewer session（viewer 会话）**: viewer 中与绘制无关的视图状态、讨论上下文及面板请求处理；状态保存于插件按 DSH 会话维护的 JSON 快照，不属于项目状态。见 [ADR 0014](docs/adr/0014-review-decisions.md)。

**viewer runtime（viewer 运行时）**: 提供 resident 会话、IR 生成、扫描器和三个质量工具的原生 Rust 二进制。

## 交付与生命周期

**delivery platform（交付平台）**: 承诺运行并验收的宿主：原生 Windows x64 上的 DeepSeek Harness。

**machine installation（机器安装）**: 本机当前用户共享的一份工具安装，供所有被查看项目使用；安装与维护见 [README](README.md#plugin-installation-and-maintenance)。

**plugin（插件）**: 名为 `dsh-uml-viewer` 的 DeepSeek Harness 插件，是机器安装的唯一来源，承载安装、更新与卸载。

**project state（项目状态）**: 工具留在被查看项目内的数据，包括 policy、提案与质量快照；viewer 会话状态不属于它。

**release（发行版本）**: 由单一 SemVer 版本号标识的一次完整对外发布；发布流程见 [releasing](docs/releasing.md)。

**automatic update（自动更新）**: 插件管理取得新发行版本后，在下一次全新启动时生效的更新机制；运行中的会话继续使用原版本。
