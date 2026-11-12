# 0017 标准覆盖率路径复用 crapper 的 stamp，覆盖率脚本按平台生成

**Status**: accepted

## 背景

第一个撞上来的使用方还是 qinyuanj/kacha 的质量门禁（[ADR 0016](0016-copy-outside-paths.md) 的同一个调用）：
它先跑 `crap`，再按包跑 `mutator --root client`。`crap` 已经量过整个包，把各语言的
报告写在 `target/coverage` 下，并写下 `target/coverage/crapper-stamp.json`；这个
stamp 的用途，就是让 `mutator` 不必重跑一遍。

本移植的标准覆盖率路径（既没有 `--coverage-command`，也没有 `--test-command`）从来
没读过 stamp：Lua 之外，Rust、Python、TypeScript、Go、Java、Clojure 都被无条件重
跑。这是移植缺口，不是设计选择——mutate 的 help 文本（`fixtures/mutate/help.txt`）和
被固定版本的 Python 原版都明确承诺：

```text
Before generating coverage, mutator reads target/coverage/crapper-stamp.json.
A language whose reports crapper produced for these files, with the same
--coverage-command, from the tree as it is now, is reused, and the run prints
"Reusing <languages> coverage from crapper's last run." Only the other
languages are regenerated.
```

缺口在 Linux 上只表现为白跑一遍工具；在 Windows 上是硬失败，因为生成的重跑脚本只
有 POSIX 一种：`rm -rf -- <绝对报告目录>`、`code=$?`、`[ -f … ]`、`&&` 与 `if … then
… fi`。cmd.exe 下第一句就错：

```text
The filename, directory name, or volume label syntax is incorrect.
Error occurred while processing: 'D:\dev\github\kacha\client\target\coverage\typescript'.
```

于是 TypeScript 覆盖率报告根本没有产生，全部站点落到 `:uncovered`（`executed=0`），
门禁一个变异体都判不出来。同一个脚本还有第二个 Windows 缺陷：它把**真实树**的绝对
路径写进副本里的命令，而 Windows 副本在 `<slot>/project`，绝对路径会指回真实项目
——`rust_coverage` 早就按「相对命令自己的工作目录」处理过这条规则，TypeScript 与
Python 的脚本没有跟上。

## 决定

1. 标准覆盖率路径（以及只给了 `--test-command` 的路径）先读
   `target/coverage/crapper-stamp.json`：`stale_files(root, files, None)` 把选中文件
   按语言分成 stale 与 fresh。**一种语言要么整体重测、要么整体复用**（一个文件 stale
   就带上该语言的全部文件），与原版 `_stale` 的判定一致。
2. fresh 非空时，向 stderr 打印 `Reusing <languages> coverage from crapper's last
   run.`，语言名升序、逗号加空格分隔——与原版逐字节相同，也是 help 文本承诺的那行。
3. fresh 语言的数据从 crapper 已经落盘的报告里读：项目根的 `coverage/lcov.info`、
   `target/coverage/**/lcov.info`（LCOV 语言）与 Go/Java/Clojure 各自的 reader；只有
   stale 语言才在视图（Linux overlay）/副本（Windows）里重跑。读取顺序是先读盘上报告
   、再合并本次重跑的报告，同源条目以新报告为准。
4. Lua 不查 stamp：它由本次运行自己的 instrumented suite 收集（原版
   `_prepare_coverage` 里 Lua 也在 `_stale` 之前）。
5. TypeScript 与 Python 的覆盖率脚本按平台生成。Linux 保持原来的 POSIX 文本（与原版
   一致）；Windows 生成 cmd 单行链，因为 `cmd /c` 的字符串里**没有换行分隔符**：
   - 删除/创建报告目录用 `rmdir /s /q … 2>nul`、`mkdir … 2>nul`、`del /f /q … 2>nul`；
   - 失败守卫用 `|| (…)` 块，链上每个条件检查都放进括号并排在链尾：cmd 会把
     `if 条件 (块) & 后续` 的后续整段当条件块吃掉，括号块 `( if … (…) else (…) )`
     则不会；
   - 脚本写入的每个路径都相对**包目录**（`relative_from`），绝不写真实树的绝对路径；
   - 参数按 cmd/MSVCRT 的规则引用（双引号），不是 `shlex` 的单引号；
   - `echo` 文本里插的路径同样引用：cmd 在运行之前就解析整行，未引用的 `&` 会
     截断 `echo` 并把后文当命令执行，未引用的 `(`/`)` 会让整条 `/c` 行以
     `… was unexpected at this time.` 中止，覆盖率报告因此静默缺失（#66）。
     诊断里的包名用相对**项目根**的写法（`relative_from`；跨盘时退回绝对路径），
     报告路径本来就是相对包目录的；引号只挡元字符，不挡 `%VAR%` 展开；
   - Vitest 走 `node_modules\.bin\vitest.cmd`：cmd.exe 执行不了 npm 写出的 POSIX shim。
6. 退出码语义保持原样：覆盖率脚本永远不让本次运行失败。Windows 上拿不到退出码，所以
   `TypeScript coverage exited N in <包>` 改成 `TypeScript coverage failed in <包>`，
   报告缺失的 N/A 消息保持原措辞。

## 被否决的方案

- **Windows 上把 TypeScript/Python 标成覆盖率 N/A**：门禁会整批 `:uncovered`，而
  kacha 的 `client/src` 几乎全是被测的 TypeScript。这等于放弃两种语言，否决。
- **副本里照旧用绝对路径，只把 POSIX 语法换成 cmd 语法**：报告会写进真实项目树，
  正是 `rust_coverage` 与写契约（[ADR 0010](0010-native-windows-delivery-platform.md)）禁止的事。否决。
- **把脚本写成真正的 `.cmd` 批处理文件**：要在副本里先写一个文件，而副本由隔离层
  创建，脚本本身没有写文件的时机；而且每个包都要一份，还得自己清理。否决。
- **靠 `cmd /v:on` 延迟展开或用 `%ERRORLEVEL%` 取退出码**：本移植的进程边界固定
  `/v:off`，且 `%ERRORLEVEL%` 在整行解析期展开，单行链上取不到真实退出码。否决：
  宁可换措辞，也不引入隐藏的展开期依赖。
- **在标准路径用 `--coverage-command` 绕过重跑**：调用方要自己写一遍命令，且它与
  crapper 写进 stamp 的 command 不再一致（stamp 因此永远不匹配）。否决。
- **直接调 `quality_core::runner::run_coverage` 来「复用」**：它写真实树，隔离不允许。
  否决。

## 后果

- kacha 一类的门禁在 Windows 上第一次拿到 TypeScript 覆盖率；crapper 已经量过的语言
  不再重跑，本次运行也省下整轮工具时间。
- help 文本的承诺现在真的成立，`fixtures/mutate/help.txt` 不需要改（它早就写了这句）。
- `--coverage-command` 路径仍然总是执行命令：即便 stamp 里记着同一条命令，本移植也
  不复用（原版会复用）。这是已知偏差，写在 [docs/mutation-linux.md](../mutation-linux.md)，因为它是跨平台
  的语义选择，不是 Windows 缺陷。
- Windows 上 `if … (块)` 的吞行行为是本 ADR 发现的坑：脚本里凡是跟 `&` 的条件块都
  必须括号化，`crates/mutator/src/{typescript,python}.rs` 的注释记录了这条规则。
- 测试：`crates/mutator/tests/coverage_reuse.rs` 六个用例——stamp 新鲜时只读报告
  （变异体被杀证明数据来自 stamp）、没有 stamp 时 Windows 上自己的 TypeScript 脚本
  产出覆盖率、Python 同理（`coverage.py` 可导入时，`MUTATOR_REQUIRE_TOOLCHAINS=1`
  下缺失即失败），以及包目录名含 `&`、`(`、`)` 时两种语言仍写出 LCOV（#66）。
- CI：`windows-mutate` 在跑 mutator 套件前安装 `coverage`（`setup-python` 提前），
  否则 Python 用例在「缺工具链必须失败」的约定下只能算缺工具。
