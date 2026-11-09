# viewer 技术栈基准：合成场景与测量协议

服务于选型 map「viewer 技术栈选型：替代 LOVE + Lua」(#124)。所有入围原型和当时的
LOVE viewer 都用这里的场景和协议测量，结果才能横向比较。当前 viewer 是 Rust
（`bench/run_rust.sh`）；LOVE/Lua 测量工具已随 #144 删除，下文的 LOVE 结果保留为
历史对照。

## 场景

| 文件 | 内容 |
|---|---|
| `scenes/synthetic-500.edn`、`scenes/synthetic-2000.edn` | 合成 IR（非层级，`:packages` 分组），与真实图同一种输入 |
| `scenes/synthetic-N.scene.edn` / `.scene.json` | 已布局的渲染场景，内容相同，供任何语言直接读取 |

生成方式（确定性；生成脚本是 Lua，已随 #144 删除，下面的命令仅存档）：

```sh
# historical, no longer runnable: the Lua generators were removed in #144
luajit bench/gen_synthetic.lua 500 5 bench/scenes/synthetic-500.edn
luajit bench/gen_synthetic.lua 2000 10 bench/scenes/synthetic-2000.edn
luajit bench/export_scene.lua bench/scenes/synthetic-500.edn bench/scenes/synthetic-500
luajit bench/export_scene.lua bench/scenes/synthetic-2000.edn bench/scenes/synthetic-2000
```

已提交的场景文件是当前基准的输入；重新生成需要 tag `archive/benchmark-protocol` 里的 Lua 工具。

分布按 `examples/uml-viewer.edn`（89 类、262 边）校准：每类成员数 p50 3 / p90 13 /
最大 34，类名长度 p50 7 / p90 13，成员文本长度 p50 10 / p90 17，约 3 条依赖边/类
（80% 包内，20% 跨包；4% 继承，3% 违规）。5% 类名为中文，用来压测 CJK 字形路径。
质量数据：10% 无数据（红色虚线框 +「无数据」），15% 只有 CRAP，75% 两者都有。

| 场景 | 包 | 类 | 边 | 世界尺寸 | 边折线点 |
|---|---|---|---|---|---|
| 500 | 5 | 500 | 1617 | 8298 × 13742 | 95k |
| 2000 | 10 | 2000 | 6495 | 14606 × 29035 | 416k |
| 500-chain（#87 遗留） | 0（扁平） | 500 | 499（链） | — | 极小 |

`500-chain`（`gen_synthetic.lua 500 1 <out> chain`）复刻 #87 的旧基线场景：
无成员、无质量数据的 500 个扁平类排成一条依赖链。它只做存量对照，选型以
500 / 2000 写实场景为准。

**cimgui 顶点上限**：viewer 完全没有视口剔除和 LOD，每帧把全部类框（圆角描边、
C/M 圆点、全部文本字形）与边折线交给同一个 16 位 `ImDrawIdx` 绘制列表，而圆角
弧段与圆点细分随 zoom 增长、文本字形数与 zoom 无关（写实 500 类场景仅文本就约
12 万顶点）。后果按场景分档：

- 写实 500 / 2000（有成员文本）：**任何 zoom 下首帧即崩溃**，与 hide-edges 无关。
- 链式 500（无成员）：旧协议 zoom 0.1 可运行；本协议 zoom 1 的细节相位同样崩溃。

因此 LOVE 对照只有一档可测量：`UML_VIEWER_LEGACY=1`（仅 idle / pan-fit，
zoom 锁在钳制后的 fit，即 viewer 自己的 0.1 下限）跑链式 500。缩放相位在
1.3×0.1 即溢出顶点上限，无法测量。其余组合记录为「不可渲染」，崩溃 stderr
保留在 `results/` 下，这本身就是选型依据。候选原型按 LOD 要求自行处理，
不受此限。

### 可重新生成的 `:levels` 场景（#188）

`crates/scene/tests/common/levels.rs` 的 `scene(n)` 在测试里按规模确定性地生成 IR，
不提交文件。形态是一个按 `:levels` 分层的包加 9 个 foreign oval，即真实 `:levels`
包视图编译时的形态；参数按 fs-monopoly（569 类，8 个 level）校准：各 level 类数比例、
每类 3.2 条边、61% 的类间边在同一 level 内、跨 level 边的跨度分布都取自它，
8% 的边指向 foreign oval（fs-monopoly 0%，本仓自举图 16%）。上面两个已提交场景是
10 个包、4 个等大 level、只有 25% 的边在 level 内，与真实项目不符，仍保留作存量对照。

`bench_scene` 在 250、500、1000、2000 四个规模上各取三次最好值，拟合 log(时间)–log(类数)
斜率，要求小于 log 10 / log 4 ≈ 1.66（与 500→2000 不超过 10 倍是同一个界），
2000 类不超过 30 s。2026-10-28 release 实测：0.14 / 0.30 / 0.68 / 1.80 s，斜率 1.21。

### 渲染场景格式（version 1）

坐标是世界单位（zoom 1 = 1 view px，DPI 由前端另乘），颜色是 0–255 RGB，取自
`domain/theme`。分级规则与 `adapters/class_boxes` 一致：任一指标缺失即最差等级。

```
size      {w h min-x min-y}
packages  [{id label rect:[x y w h] grade fill stroke crap-band mutation-band}]
classes   [{id package rect shape:"box"|"oval" level missing grade fill stroke
            crap-band mutation-band lines:[{kind:"name"|"stereo"|"rule"|"field"|"op"|"child" text?}]}]
edges     [{from to kind violating strokes:[[x0 y0 x1 y1 …] …] tip:[x y] behind:[x y]}]
```

`strokes` 是已展平、已在类框处断开的折线；箭头方向由 `behind → tip` 给出。
原型必须画出：类框（填充 + 描边 + 名称 + 成员行 + 分隔线）、C/M 两个色点、
无数据标记、包框与标题、边折线与箭头、违规边红色。悬停/选中高亮、端口、
依赖三角不在基准范围内。

## 测量协议（protocol 1）

**固定条件**：窗口 1500 × 920 view px，DPI 缩放 1；画布宽 = 窗口宽减去该栈的侧栏
（LOVE 为 1220）。记录 vsync 状态与刷新率，不强制关闭 vsync。每次运行记录实际
renderer 字符串（LOVE：`love.graphics.getRendererInfo()`；WebGL：
`WEBGL_debug_renderer_info`；wgpu：`AdapterInfo`）。

**平台**：

- WSLg：`GALLIUM_DRIVER=d3d12`，wgpu 系另加 `WGPU_BACKEND=gl`。renderer 字符串
  必须含 `D3D12`，否则这次运行作废。`llvmpipe` 只作对照。
- Windows 原生 / 浏览器：独立浏览器 profile，关闭扩展，只开基准页。

**相位**（相机按相位内经过的时间直接设定，与帧率无关，所以各栈走同一条轨迹）：

| 相位 | 相机 | 目的 |
|---|---|---|
| warmup 3 s | zoom 1，场景中心 | 不计入 |
| idle | 不动 | CPU 占用；按需绘制的栈帧数可为 0 |
| pan-detail | zoom 1，绕中心半径 1200 的圆，周期 8 s（约 940 px/s） | 细节级平移 |
| zoom-detail | 中心不动，zoom 0.5 ↔ 2 几何插值，周期 4 s | 细节级缩放 |
| pan-fit | zoom = fit，Lissajous 扫过场景 25%，周期 8 s | 全图（LOD）平移 |
| zoom-fit | 中心不动，zoom fit ↔ 4×fit，周期 4 s | 跨 LOD 档缩放 |

`fit = min(1, 画布宽/场景宽, 画布高/场景高)`，**不受**现有 viewer 0.1 缩放下限约束。
每个相位持续 ≥ 10 s 且 ≥ 20 帧，上限 180 s。

**指标**（每相位）：帧数、fps（帧数/帧间隔总和）、帧间隔 mean / median / p95 /
p99 / max，CPU% = 进程树 (user+sys) 时间增量 / 墙钟时间（100% = 一个核）。
帧间隔是相邻两次 present 之间的时间，包含输入、更新和绘制。

- Linux / WSLg：`/proc/<pid>/stat` 第 14、15 字段，除以 `CLK_TCK`；多进程时累加子进程。
- Windows：`Get-Process` 的 `TotalProcessorTime`，累加该 profile 下全部浏览器/GPU 进程。
- 浏览器帧间隔：`requestAnimationFrame` 时间戳差。

**判定**（来自 map Notes；百分位解释为本协议的约定）：

| 场景 | 相位 | 达标 |
|---|---|---|
| 2000 | 四个交互相位 | p95 帧间隔 ≤ 33.3 ms（≥ 30 fps） |
| 500 | 四个交互相位 | mean fps ≥ 58 且 p95 ≤ 18.3 ms（一个 60 Hz 帧 + 10%） |
| 两者 | idle | CPU ≤ 2%（一个核的百分比） |

启动时间和 IR 时间只记录，不设门槛。

## 当前 Rust viewer

```sh
bench/run_rust.sh bench/scenes/synthetic-500.edn <out-dir>
bench/run_rust.sh bench/scenes/synthetic-2000.edn <out-dir>
```

`run_rust.sh` 用 `target/release/uml-viewer viewer` 和 `UML_VIEWER_BENCH` 环境变量跑同一协议，写 `protocol.edn` 与 `summary.txt`；
#143 的实测结果见 `acceptance-evidence/143-rust-install/`。

## 历史 LOVE 对照

```sh
UML_CJK_FONT=<SarasaMonoSC-Regular.ttf> bench/run_love.sh bench/scenes/synthetic-500.edn d3d12 <out>
```

`love_protocol_driver.lua` 通过 `UML_VIEWER_TEST_DRIVER` 注入当时的 LOVE viewer，按上表设置
相机，输出 `protocol.edn` 和 `summary.txt`。两个脚本都已随 #144 删除（存档于 tag `archive/benchmark-protocol`）；结果见下节。

## 结果

机器：Ryzen 5 5500 / GTX 1080 Ti，WSL2 + WSLg，历史 LOVE 结果使用 LOVE 11.5.0 /
LuaJIT 2.1，D3D12 渲染（`GALLIUM_DRIVER=d3d12`）；当前 Rust 结果使用 egui/eframe 0.37
与 wgpu GL 后端。vsync 在 WSLg 下报告关闭（`flags.vsync=0`），刷新率 60 Hz。

### 历史 LOVE 对照（本协议，工具已删除）

可测量的只有链式 500 legacy 档（idle + pan-fit，zoom 钳制 0.1）：

| 场景 | renderer | idle fps / p95 ms / CPU | pan-fit fps / p95 ms / CPU |
|---|---|---|---|
| 500-chain | D3D12 (GTX 1080 Ti) | 10.7 / 134 / 95% | 10.2 / 145 / 95% |
| 500-chain | llvmpipe | 10.5 / 140 / 109% | 10.3 / 145 / 109% |

CPU% 可超 100%（多线程渲染驱动计入同一进程时间）。GPU 与否依旧无差别，
再次坐实瓶颈在 CPU 端每帧重建几何。

与 #106 基线（合成 500：idle 9.8 fps、interaction 2.0 fps）一致，说明协议
没有改变 viewer 的既有表现。

不可渲染组合（cimgui 16 位顶点断言 SIGABRT，stderr 留档）：

| 场景 | 说明 |
|---|---|
| 500 / 2000 写实，任何 zoom | 文本字形约 12 万顶点即超 65,536 上限；hide-edges 无效 |
| 500-chain，zoom ≥ 1.3×0.1 | 顶点 60,048 → 66,952（`UML_VIEWER_DEBUG_VTX` 实测） |
| 500-chain，真实 fit zoom 0.0101 | 73,964 顶点：clip 剔除失效后全部文本入列 |

启动开销（LuaJIT，只记录不设门槛）：500 类 IR→scene 34 s，2000 类 532 s；
瓶颈在 `engine/compose.prepare_scene` 每条边重建全量类索引（O(E·N)）。
