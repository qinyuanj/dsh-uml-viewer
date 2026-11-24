# NOTES-150 — 发版漂移检查（npm / tag / package.json 三方一致）

Issue: qinyuanj/dsh-uml-viewer#150

## 背景

`package-release` 的 `publish` job 声明 `environment: release`，该环境带 `required_reviewers`
保护规则（reviewer 只有 `qinyuanj`）。门禁失败时是静默的：main 上的 `ci` run 全绿，
run 列表里那条 `package-release` 只是「waiting」，没有任何检查会发现
「tag 存在、CI 绿、但包根本没发出去」。

已经因此丢了一个版本：

| tag | commit | run | publish job | npm 上有吗 |
|---|---|---|---|---|
| `v1.2.1` | `8168004a` | `45031367826` | cancelled（2026-11-22T14:16:01Z） | **没有** |
| `v1.2.2` | `ec032adc` | `45421102962` | waiting 1h28m 后由 lead 批准 | 有 |

要守住的三个事实：npm `dist-tags.latest`、仓库 `package.json` 的 `version`、最新的 `v*` git tag。

实测传播窗口：平台包 16:37:06Z 可见、主包 16:38:13Z 可见，相差 67 秒；npm 自己也提示
「Your package is being processed and may take a few minutes to become available」。
所以刚发布完立刻检查必须容忍宽限窗口。

约束：只读公开 registry，不需要 npm 凭据，不引入新的 npm 依赖。

## 实现

### `scripts/release-drift.mjs`（新增）

- 读取三个事实：
  - npm：主包 `dsh-uml-viewer` 与平台包 `@dsh-uml-viewer/win32-x64` 的 `dist-tags.latest`
    与版本列表（一次 packument 请求同时拿到两者）。包名复用 `scripts/release.mjs` 的
    `platforms`，tag 解析复用同文件的 `tagVersion`，仓库根目录复用 `scripts/version.mjs` 的 `root`。
  - 仓库版本：根 `package.json` 的 `version`。
  - 最新 tag：`git tag --list 'v*' --sort=-v:refname`，取第一个能解析成 SemVer 的。
- 比对（`compareReleaseFacts`，纯函数）：
  - 仓库版本 == tag 版本；
  - 每个包的 npm `latest` == tag 版本（并区分「registry 落后于 tag」/「领先于 tag」）；
  - **每个包的版本列表里必须存在 tag 指向的版本**（这就是「registry 缺该版本」，
    只比 `latest` 抓不到 v1.2.1 这种从未发布过的版本）；
  - 每个包的版本列表里必须存在仓库版本（对应验收里的本地复现）。
- 失败时 `formatDrift` 打印三个值与逐条差异。
- 宽限窗口：默认 5 分钟（`--grace-ms` 可调，`0` 表示不等），退避
  15s/30s/45s/60s/60s/60s，每次重试重新读取三个事实，并在重试之间 best-effort
  `git fetch --tags`（这样 tag 在窗口内才推出来也能被看到）。
- npm 不可达：`RegistryError` + 清晰信息（「npm registry is unreachable (dsh-uml-viewer): ...」，
  「The published versions are unknown, so no release claim can be made」），最多重试 3 次后放弃，
  不吐栈。
- 本地复现用的开关（CI 不使用）：`--registry`、`--tag`、`--repo-version`、`--grace-ms`。

### `scripts/release-drift.test.mjs`（新增）

13 条用例，全部离线（registry 用进程内 double，git 用临时仓库）：

1. registry 缺 tag 指向的版本 → 失败并点名 `npm has no version 1.2.3 ...: the tag's version was never published`
2. registry 落后于 tag → 失败并说 `the registry is behind the tag`
3. 仓库版本从未发布 → 失败并说 `the repository version was never published`
4. 三方一致 → 通过
5. 报告格式：三个值与差异都在
6. 最新 tag 的挑选规则
7. registry double：两个包都答、scoped 包的 URL 编码、404 → 包不存在
8. 端到端收集事实：临时 git 仓库 + manifest + 两个包
9. npm 不可达 → 清晰信息，不是栈
10. 宽限窗口内变得可见 → 不是误红（假时钟，2 次尝试后绿）
11. 窗口耗尽 → 报最后一次的差异
12. 命令行在 registry 落后时退出码 1 并打印差异（进程内 HTTP registry）
13. 命令行在三方一致时退出码 0

### 接线

- `scripts/test-package.mjs`：suite 列表加上 `scripts/release-drift.test.mjs`。
- `scripts/release.test.mjs`：既有的两个断言补上新门禁（suite 列表里必须有新测试；
  `ci.yml` 必须有 `release-drift` job、跑 `node scripts/release-drift.mjs`、只在 main 上跑）。
- `.github/workflows/ci.yml`：新增 `release-drift` job
  （`if: github.event_name == 'push' && github.ref == 'refs/heads/main'`，
  `ubuntu-latest`，`fetch-depth: 0` 取全量 tag，Node 24，不装依赖、不需要凭据，
  `timeout-minutes: 15`）。正常状态几秒完成，不会拖慢其它 job。
- `docs/releasing.md`：新增「Release drift gate」一节，并把它加进本地可跑清单。

## 验证记录（本 worktree，branch `ticket/150-release-drift-check`）

环境：Node v26.13.1（仓库要求 24+）、pnpm 12.11.2、Python 3.15.2、git 2.56.0。
本 worktree **没有 node_modules**，新脚本与新测试只用 Node 内置模块，因此不需要
`pnpm install`（`pnpm version:check` 与 `pnpm test:package` 也都是纯 node 脚本，实测直接可跑）。

1. 新检查自己的测试：

```
$ node --test --test-reporter=tap scripts/release-drift.test.mjs
1..13
# tests 13
# pass 13
# fail 0
EXIT=0
```

2. `pnpm version:check`：

```
$ pnpm version:check
Release version 1.2.2: Cargo/npm manifests consistent
EXIT=0
```

3. `pnpm test:package`（含新 suite）：

```
$ pnpm test:package
ℹ tests 100
ℹ pass 100
ℹ fail 0
EXIT=0
```

4. 对当前真实状态运行新检查（npm 公开 registry、真实 tag、真实 package.json）：

```
$ node scripts/release-drift.mjs
release facts agree: 1.2.2 = v1.2.2 (dsh-uml-viewer@1.2.2, @dsh-uml-viewer/win32-x64@1.2.2)
EXIT=0
```

5. 对故意不一致的输入运行（临时把 `package.json` 的 version 改成 npm 上不存在的
   `9.9.9`，跑完立刻还原）：

```
$ node scripts/release-drift.mjs --grace-ms 0
release drift detected after 1 attempt(s) over 2s:
  npm latest dsh-uml-viewer                 = 1.2.2
  npm latest @dsh-uml-viewer/win32-x64      = 1.2.2
  repository package.json version     = 9.9.9
  newest release tag                  = v1.2.2 (1.2.2)
differences:
  - repository package.json version 9.9.9 != newest release tag v1.2.2 (1.2.2)
  - npm has no version 9.9.9 for dsh-uml-viewer: the repository version was never published
  - npm has no version 9.9.9 for @dsh-uml-viewer/win32-x64: the repository version was never published
EXIT=1
$ node -e "console.log(JSON.parse(require('fs').readFileSync('package.json','utf8')).version)"
1.2.2
$ git diff --stat -- package.json      # 空：已还原
```

   `--grace-ms 0` 只是为了本地复现不等满 5 分钟；默认宽限窗口的行为由单元测试
   「a release that becomes visible inside the grace window is not a false red」覆盖。

6. 仓库自带的快速 python 套件（我新增了文件，怕踩行尾/进程边界规则）：

```
$ python -I scripts/test_line_endings.py     # OK
$ python -I scripts/test_process_boundary.py  # OK (4 tests)
```

## 提交后的复验（两个 commit 之后，`git status` 干净）

```
$ python -c "import yaml;d=yaml.safe_load(open('.github/workflows/ci.yml',encoding='utf-8'));print(sorted(d['jobs']))"
['panel-e2e', 'release-drift', 'typescript', 'windows-crap', 'windows-mutate', 'windows-quality']
release-drift 的 if: github.event_name == 'push' && github.ref == 'refs/heads/main'

$ node --test --test-reporter=tap scripts/release-drift.test.mjs
# tests 13 / # pass 13 / # fail 0        EXIT=0

$ pnpm version:check
Release version 1.2.2: Cargo/npm manifests consistent        EXIT=0

$ pnpm test:package
ℹ tests 101 / ℹ pass 101 / ℹ fail 0                         EXIT=0

$ node scripts/release-drift.mjs
release facts agree: 1.2.2 = v1.2.2 (dsh-uml-viewer@1.2.2, @dsh-uml-viewer/win32-x64@1.2.2)
EXIT=0

$ node scripts/release-drift.mjs --grace-ms 0 --tag v9.9.9 --repo-version 9.9.9
release drift detected after 1 attempt(s) over 2s:
  npm latest dsh-uml-viewer                 = 1.2.2
  npm latest @dsh-uml-viewer/win32-x64      = 1.2.2
  repository package.json version     = 9.9.9
  newest release tag                  = v9.9.9 (9.9.9)
differences:
  - npm latest for dsh-uml-viewer is 1.2.2, not the tag version 9.9.9 (the registry is behind the tag)
  - npm has no version 9.9.9 for dsh-uml-viewer: the tag's version was never published
  - npm has no version 9.9.9 for dsh-uml-viewer: the repository version was never published
  - npm latest for @dsh-uml-viewer/win32-x64 is 1.2.2, not the tag version 9.9.9 (the registry is behind the tag)
  - npm has no version 9.9.9 for @dsh-uml-viewer/win32-x64: the tag's version was never published
  - npm has no version 9.9.9 for @dsh-uml-viewer/win32-x64: the repository version was never published
EXIT=1
```

## 已知边界 / 备注

- `scripts/check.py` 在这个 worktree 里跑不了完整形态：它要 `cargo build --release --locked`
  加 `cargo test --workspace --locked`，与本次改动无关且耗时很长；它的三个快速 python
  套件已单独跑过（见上）。
- CI 上的 `release-drift` job 用 `ubuntu-latest`（其它 job 是 `windows-latest`）：
  这个检查只读 registry 与 git tag，与平台无关，用 ubuntu 更省。仓库既有 job 仍用
  windows-latest，未改动。
- 门禁在「版本已经提到 main、但 tag 还没推」的窗口里会红（仓库版本领先于最新 tag）。
  这是 issue 要求的三方严格一致；宽限窗口内的重试会重新读取 tag，
  `git fetch --tags` 能把窗口内推上来的 tag 捡回来。默认窗口 5 分钟。
- 没有 push、没有开 PR、没有动 issue、没有改 `.metrics/`、`examples/uml-viewer.edn`，
  也没有改任何已发布版本号。
