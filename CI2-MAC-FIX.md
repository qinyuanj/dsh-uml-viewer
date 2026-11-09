# CI2 macOS host seam 夹具修正

本分支只修正四个 host 测试夹具的临时目录创建点，保留所有原断言；本地 junction 红绿验证已通过，真实 macOS 仍需 CI 验证。

## 基线与范围

- CI：run `42585882825`，job `129055477147`；输入为外部只读日志 `D:/dev/github/spec203-notes/CI2-typescript-mac.log`。
- 指定工作树：`D:/dev/github/dsh-uml-viewer-wt/tcimac`，分支 `tci-macos-fixtures`，起点 `0d115b2`。
- 已合并最新本地 `spec-203`：`3d329e5d04b7ce7a974c6647b88a23b56f78f261`，fast-forward；该提交含 lead 的一行 clippy 修正。本分支没有自行修改生产源码。
- CI 原结果：host 15 个文件中 4 失败、11 通过；171 个测试中 12 失败、157 通过、2 个 Windows 专属测试跳过。contracts 74、client 152 均通过。

## 根因与逐项证据

`mkdtemp` 返回保留临时目录父路径拼写的字符串。macOS 的 `/var` 经符号链接指向 `/private/var`；[prepareConfinedCommand](packages/host/src/process.ts#L69) 正确地对会话根目录调用 `realpath`，而测试把原始 `/var/...` 用作预期值或注入记录。版本夹具另外由真实 Node 子进程记录 `__filename`，Node 将该路径解析成 `/private/var/...`。这些差异在四个夹具的创建点解决，无须改变生产路径或安全策略。

以下行号为原 CI 日志中的失败测试位置：

| 原测试位置 | 失败用例 | 实际失败字段与原因 |
| --- | --- | --- |
| [ir.seam.test.ts](packages/host/tests/ir.seam.test.ts) 原195 | loads in Cordis and generates IR in the session cwd, returning a version-1 record | `projectRoot`：规范路径 `/private/var` 对夹具 `/var`；后续子进程 cwd 断言也需要同一真实根目录。 |
| [panel.seam.test.ts](packages/host/tests/panel.seam.test.ts) 原212 | refuses uml_display until the latest uml_ir in that session succeeded | 成功 display 记录的 `projectRoot` 拼写差异。 |
| 同上原358 | streams the resident scene frame to the panel bound to that session | resident invocation 的 `cwd` 拼写差异。 |
| 同上原392 | keeps one resident per session and stops it when the last panel closes | 第二个会话 resident 的 `cwd` 拼写差异。 |
| [quality.seam.test.ts](packages/host/tests/quality.seam.test.ts) 原138 | runs quality crap in the session cwd and returns its version-1 record | 成功记录 `projectRoot` 拼写差异；后续事件 cwd 断言同理。 |
| 同上原154 | forwards every mutation target and --mutate-all in one quality call | mutate 记录 `projectRoot` 拼写差异，argv 本身无差异。 |
| 同上原189 | returns a configured-operation record field for field | configured 记录 `projectRoot` 拼写差异。 |
| 同上原283 | fails closed on a different exit in the quality record | 注入 stdout 的 `<root>` 是 `/var`，宿主先拒绝 root 不匹配，尚未进入预期的 `reported exit 7` 校验。此项不是直接相等断言失败。 |
| [versions.seam.test.ts](packages/host/tests/versions.seam.test.ts) 原182 | keeps an old panel on vN after updating, while a fresh open runs vN+1 | Node 记录的 `binary`（`__filename`）是 `/private/var`，夹具预期为 `/var`。 |
| 同上原200 | survives unloading vN and loading vN+1 in the same Cordis root until the old panel closes | 同一 `binary` 差异，先于后续关闭、清理断言失败。 |
| 同上原225 | binds a second live panel of the same session to the same vN resident | 同一 `binary` 差异，先于重新打开 vN+1 的断言失败。 |
| 同上原254 | keeps vN when a withdrawn stream is followed by the same session’s remount (D13) | 同一 `binary` 差异，先于最后关闭和 cleanup 断言失败。 |

## 修正与生命周期判定

[IR 夹具](packages/host/tests/ir.seam.test.ts#L45)、[panel 夹具](packages/host/tests/panel.seam.test.ts#L86)、[quality 夹具](packages/host/tests/quality.seam.test.ts#L81)、[versions scratch](packages/host/tests/versions.seam.test.ts#L36) 的 6 个 `mkdtemp` 创建点改为 `await realpath(await mkdtemp(...))`；home 与项目根目录在交给服务、写入配置、记录清理列表前统一使用真实路径。

versions 的 home 从 scratch 创建后始终传给相同的插件激活和版本存储；[pinLocked](packages/host/src/versions.ts#L339) 的 lease、release、cleanup 均捕获同一 home，未见日志所指的生产生命周期缺陷。修正后真实子进程 seam 全部继续运行，原有卸载后存活、重挂载保持 vN、显式关闭后删除旧版本、root shutdown 清理租约、Windows 映射中 exe 保留与退出后删除断言均通过。没有修改这些断言。

生产的规范路径解析、质量记录严格 root/exit 校验、沙箱门控、版本所有权和符号链接边界未作修改；没有全局 strip-prefix、字符串替换、平台例外或放宽相等比较。

## 同类扫描

扫描仓库 TS/MJS 的 `mkdtemp` / `mkdtempSync` / `symlink`。host 共 12 个测试文件、19 个临时目录创建点；四个受影响文件的 6 点已修正，其余 8 个文件的 13 点保留。全部 host 套件在带 junction 的临时目录下 171/171 通过，给这些保留点提供了同一条件下的运行证据。

| 保留的 sibling 夹具 | 判定 |
| --- | --- |
| [activation.seam.test.ts](packages/host/tests/activation.seam.test.ts)、[binary.test.ts](packages/host/tests/binary.test.ts) | 检查返回或传入的配置 argv 字符串、文件内容和存在性；没有让真实 Node 子进程把 argv 解析成 `__filename` 再与原路径比较。无需把受信配置的原始 argv 语义改成 canonical。 |
| [overlay.seam.test.ts](packages/host/tests/overlay.seam.test.ts)、[requests.seam.test.ts](packages/host/tests/requests.seam.test.ts)、[view-requests.seam.test.ts](packages/host/tests/view-requests.seam.test.ts)、[withdrawal.test.ts](packages/host/tests/withdrawal.test.ts) | 验证协议/动作、请求序列与生命周期；临时根目录用于定位文件，没有跨真实路径/别名拼写相等断言。junction 全量验证通过。 |
| [state.test.ts](packages/host/tests/state.test.ts) | 测试持久化原始 diagram 字符串及文件内容指纹；没有调用会话根目录 canonical 边界，保留存储语义。 |
| [versions.test.ts](packages/host/tests/versions.test.ts) | 直接调用版本存储，检查存在性、租约和 digest，没有真实子进程 `__filename` 比较。junction 全量验证通过。 |

另有 [package-version.test.mjs](scripts/package-version.test.mjs)、[package.test.mjs](scripts/package.test.mjs)、[release.test.mjs](scripts/release.test.mjs)、[test-package.mjs](scripts/test-package.mjs)、[e2e lane](packages/e2e/lane.mjs) 共 6 个创建点，分别使用显式 scratch 或仓库目录，而非这次失败的 host `os.tmpdir()` 夹具边界；本轮只读扫描并保留，未据此声称其真实 macOS 接受测试已通过。e2e 未运行。

## 验证

环境：Windows x64，Node `v24.19.0`，pnpm `12.11.2`。依赖仅执行 `pnpm install --frozen-lockfile --offline`，下载 0 包。

| 验证 | 结果 |
| --- | --- |
| 原代码普通 Windows host 基线：`pnpm build && pnpm --filter @dsh-uml-viewer/host test` | 15 文件、171 测试通过。 |
| 原代码 junction 临时目录下四个 seam（RED，退出1） | 13 失败、69 通过，共82；包含全部12个 CI 失败用例，另有 Windows 专属运行中 exe 用例的 `process.execPath` 别名差异。 |
| 修正后完全相同 junction 条件与四个 seam（GREEN，退出0） | 4 文件、82 测试全部通过。 |
| 合并最新 spec-203 后 `pnpm typecheck` | contracts、client、host 全部通过。 |
| 合并后 `pnpm test`（脚本已检查，仅 esbuild + Vitest） | contracts 8文件/74测试，client 18文件/152测试，host 15文件/171测试，合计41文件/397测试全部通过，0跳过。 |
| junction 条件下全部 host sibling sweep | 15文件、171测试通过。 |
| `git diff --check` | 通过。 |

RED/GREEN 使用现有的公开 Cordis seam 测试，没有另加只证明实现细节的单元测试；macOS CI 的同一套断言也是持久的回归保护。junction 为本工作树内一次性目录，TMP/TEMP/TMPDIR 仅在子进程 env 设置；没有改用户/global 环境，没有碰 live GUI/profile。临时目录和 runner 已清理，红绿计数及复现方式保留在本文。

下面是实际探针的等价复现脚本（保存为此工作树根目录下的临时 `.mjs`，通过 `pnpm exec node <脚本名>` 运行；附加 `--all` 可运行全部 host）。Windows pnpm 为 exe，直接 spawn；POSIX 可使用 pnpm 可执行文件路径。必须先运行 `pnpm build` 生成 TS bundle，此命令不调用 Cargo。

```js
import { mkdir, mkdtemp, realpath, rm, symlink, writeFile } from 'node:fs/promises'
import path from 'node:path'
import { spawn } from 'node:child_process'
const base = await mkdtemp(path.resolve('.ci2-canonical-'))
const target = path.join(base, 'actual')
const alias = path.join(base, 'alias')
try {
  // Avoid inheriting this repository's ESM package type for the CJS fake payload.
  await writeFile(path.join(base, 'package.json'), JSON.stringify({ type: 'commonjs' }))
  await mkdir(target)
  await symlink(target, alias, process.platform === 'win32' ? 'junction' : 'dir')
  console.log(JSON.stringify({ alias, canonical: await realpath(alias) }))
  const suites = process.argv.includes('--all') ? [] : [
    'tests/ir.seam.test.ts', 'tests/panel.seam.test.ts',
    'tests/quality.seam.test.ts', 'tests/versions.seam.test.ts',
  ]
  const child = spawn(process.env.npm_execpath, [
    '--filter', '@dsh-uml-viewer/host', 'test', ...suites,
  ], { stdio: 'inherit', env: { ...process.env, TMP: alias, TEMP: alias, TMPDIR: alias } })
  process.exitCode = await new Promise((resolve, reject) => {
    child.on('error', reject)
    child.on('close', code => resolve(code ?? 1))
  })
} finally {
  await rm(base, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 })
}
```

限制：这是 Windows junction 对规范路径差异的真实红绿证明，不能代替真实 macOS CI；原 macOS 12 个失败的修正仍待下一轮 CI 验收。没有运行任何 Cargo 命令、本地 Rust build/test、浏览器或真实 native payload 接受测试。hunt 的可选 `references/failure-patterns.md` 读取被工具以 PATH_RESTRICTED 拒绝，未绕过该限制；hunt/tdd 主说明及仓库领域/资源约束均已加载。没有推送或 tracker 操作，集成分支未改动。

维护者质量刷新需要的生产源文件列表：本修正为空（仅测试夹具与本文）。合入的 lead clippy 生产源变更属于既有 `spec-203` 提交。
