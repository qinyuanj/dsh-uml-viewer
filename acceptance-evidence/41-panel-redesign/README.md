# #41 面板交互重新设计的面板状态截图

日期：2026-11-11。分支 `spec-41-panel-redesign`（tip `a6f3e87`）的
`packages/e2e` 全量 Playwright 运行截图：真实 DSH Web、真实 resident 进程、
模拟 LLM（运行日志里的 `Isolated mock model`），临时 DSH home 在仓库之外。
除 `legend-chinese.png` 外都是英文界面（DSH 语言 en），`legend-chinese.png`
是中文界面（zh）。这些截图记录面板在浏览器里的实际状态，不主张其他平台行为。

| 文件 | 内容 |
| --- | --- |
| `panel-three-sections.png` | 右侧栏窄面板的三段：顶栏（面包屑 `Deep`、`Source diagram ▾`、`Search`、`More`）、画布、底部状态条（`Refresh CRAP :core: Done`） |
| `docked-card-and-legend.png` | 面板容器约 919px（≥ 900px）的停靠布局：类卡片停靠右侧、图与卡片并排，resident 排版的图例常驻画布底部，选中操作条压在图下缘 |
| `selection-action-bar.png` | 选中类 `Core` 的选中操作条：`Core · Drill in · Refresh CRAP · Differential mutation · Hide · ⋯` |
| `request-queue-failed.png` | 请求状态：展开的队列列表里有三条 `Queued` 与一条带原因的 `Failed`，收起后状态条显示同一条失败与 `3 more` |
| `legend-chinese.png` | 中文界面：顶栏（源码图、搜索、更多）与图例弹层（图例、形状、质量色、关系），以及类卡片/画布上的「无数据」标记，全部取自中文语言文件 |

对应故事：1/31–34（三段骨架与请求状态，`panel-three-sections.png`、
`request-queue-failed.png`）、16–23/28–29（选中操作条与停靠布局，
`selection-action-bar.png`、`docked-card-and-legend.png`）、45–48（中文界面，
`legend-chinese.png`）。请求状态的转移与 FIFO 归因在宿主接缝测试里覆盖，
不靠截图。
