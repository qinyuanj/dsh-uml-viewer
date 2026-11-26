# 0019 CI 分层与按变更域路由

**Status**: accepted

本 ADR 记录 [issue #157](https://github.com/qinyuanj/dsh-uml-viewer/issues/157) 的 CI 结构：把
「每次推送都跑全部 Windows 通道」改为按变更影响面路由的四层结构，并在 pull request 维度开启
并发取消。路径规则与操作细节见 [docs/ci-routing.md](../ci-routing.md)。

## 背景

2026-11-26 的运行记录：纯文档提交耗时 30 分钟以上并占满 Windows runner 并发配额；完整运行约
45 分钟，其中最长的单个通道约 20 分钟，其余约 26 分钟是 `panel-e2e` 在排队，因为 6 个 Windows
通道同时起跑。PR 连续推送新提交时，旧的运行不会被取消，新提交排在旧提交之后。

## 决定

- **单一事实源的变更分类器**：`scripts/ci-changes.py` 把变更路径映射为各通道标记，两份 workflow
  各有一个 `changes` job 读它；路径规则不在 YAML 中重复。
- **四层**：Tier 0 只有分类器；Tier 1 是 `windows-fast`（格式、lint、快速守卫）与 `typescript`；
  Tier 2 是按域的 Windows 通道与 `package-release` 的 `assemble`；Tier 3 是全量兜底。
- **兜底条件**：`workflow_dispatch`、`schedule`、`v*` tag 推送、workflow 或分类器自身变更、
  未登记的路径、无法解析的 diff（缺失或不可达的 base SHA）以及空变更集，都跑整张矩阵。
- **并发**：`ci.yml` 按 `github.ref` 与事件名分组，`pull_request` 开启 `cancel-in-progress`；
  `push` 到 `main` 与 tag 推送从不取消。
- **耦合文档按构建输入路由**：`docs/companion-rules.md`（host 以文本导入）、
  `docs/plugin-development.md` 与 `README.md`（打入 npm 发行归档）触发各自的通道；其余文档、
  术语表、ADR 与验收证据是纯文档，不占用 Windows runner。
- **质量工具 crate 不触发 `panel-e2e`**：`crapper`、`mutator`、`dryer` 的独立改动只走 Rust
  通道；面板与 resident 运行时、插件源码或打包资产的改动仍触发浏览器验收。

## 后果

- 纯文档 PR 只跑分类器，不占 Windows runner。
- 未登记的路径让整张矩阵跑一遍：慢，但不会静默跳过；新增路径的做法见 `docs/ci-routing.md`。
- `main` 当前没有 required status checks，跳过的 job 不会被当作失败。将来开启分支保护时，
  只把 `changes` 或一个汇总门禁设为必检项即可，条件跳过的下游 job 不会卡住合并。
- 不改变交付平台：`changes` job 放在 `ubuntu-latest` 只是为了让分类在秒级启动且不占 Windows
  配额，它不构建、不测试任何交付物；ADR 0010、0011、0017 继续有效。

## 被否决的方案

- **只在 workflow 里写 `paths:` 过滤**：两份 workflow 各维护一份路径表，必然漂移；而且
  `paths` 无法表达「workflow 自身变更强制全量」这类兜底。
- **把 `changes` 也放到 `windows-latest`**：分类只需 git 与 Python，放 Windows 会白白占掉一个
  runner，正是本 issue 要消除的排队来源。
