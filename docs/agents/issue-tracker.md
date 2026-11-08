# Issue tracker: GitHub

本仓库的 issues 和 specs 使用 qinyuanj/dsh-uml-viewer 的 GitHub Issues。
使用 gh CLI，显式指定 --repo qinyuanj/dsh-uml-viewer，确保操作目标明确。

## 常用操作

- 创建：gh issue create --repo qinyuanj/dsh-uml-viewer --title "..." --body-file <文件>
- 读取：gh issue view <编号> --repo qinyuanj/dsh-uml-viewer --json number,title,body,labels,comments
- 列表：gh issue list --repo qinyuanj/dsh-uml-viewer --state open --json number,title,body,labels
- 评论：gh issue comment <编号> --repo qinyuanj/dsh-uml-viewer --body-file <文件>
- 添加标签：gh issue edit <编号> --repo qinyuanj/dsh-uml-viewer --add-label "..."
- 移除标签：gh issue edit <编号> --repo qinyuanj/dsh-uml-viewer --remove-label "..."
- 关闭：gh issue close <编号> --repo qinyuanj/dsh-uml-viewer --comment "..."

多行正文写入临时文件，通过 --body-file 传入。

## 编号引用约定

见 [ADR 0013](../adr/0013-repository-migration.md)：

- 历史 commit、代码注释与 ADR 0001–0012 中的裸 `#N` 指 `qinyuanj/uml-viewer#N`，不改写。
- 此后新写的裸 `#N` 指本仓库 `qinyuanj/dsh-uml-viewer#N`。
- 跨仓库引用一律写全名 `owner/repo#N`，例如 spec `qinyuanj/uml-viewer#203`。
- 读到旧 `#N` 时用 `--repo qinyuanj/uml-viewer` 查询，不要按本仓库编号解读。

## Pull requests as a triage surface

**PRs as a request surface: no.**

## 技能指令的含义

“发布到 issue tracker”表示创建 GitHub issue。
“获取相关 ticket”表示读取对应 issue 的正文、标签和评论。

GitHub 的 issues 和 PRs 共用编号空间。编号类型不明确时，
先用 gh pr view 检查，再回退到 gh issue view；均指定上述仓库。

## Wayfinding

/wayfinder 使用一个标记为 wayfinder:map 的 issue 作为 map，
其正文记录 Notes、Decisions-so-far 和 Fog。

子 ticket 使用 GitHub sub-issue 关联到 map。
不支持 sub-issues 时，在 map 的任务列表中列出子 ticket，
并在子 ticket 正文顶部写 Part of #<map>。
类型标签使用 wayfinder:research、wayfinder:prototype、
wayfinder:grilling 或 wayfinder:task。

阻塞关系使用 GitHub 原生 issue dependencies。
通过 gh api 创建关系时，使用阻塞 issue 的数据库 id，
而不是 issue 编号或 node_id。
不支持 dependencies 时，在正文顶部写 Blocked by: #<编号>。
所有阻塞 issue 均关闭后，ticket 才可执行。

选择 map 中按顺序排列、未指派且无开放阻塞项的第一个 ticket。
认领时使用 gh issue edit 的 --add-assignee @me。
解决后先评论结果，再关闭 ticket，
并在 map 的 Decisions-so-far 中追加结论和链接。
