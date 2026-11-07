# 0002 Session restart compatibility and view retention

**Status**: accepted

session 恢复契约采用显式路径、保存路径、旧会话 policy 输出的优先级。仅缺少保存路径时选择 policy：优先 `examples/<项目目录名>.policy.edn`，否则按文件名排序的第一个 `examples/*.policy.edn`，否则根目录 `policy.edn`，与公共 `./uml ir` 一致。`:out` 相对路径按项目根目录解析；缺省值沿用 IR 的项目根目录 `diagram.edn`。policy 缺失、格式错误、输出无效或图无法加载均明确失败；保存或显式路径无效时不尝试其他图。恢复继续保留视图、discussion context 和邮箱游标，不修改 companion ownership。公共 wrapper 的旧 launcher/JVM 身份验证与等待、新 JVM 的 Herdr 验证及 companion 复用保持不变，不跨后端 restart，不将 legacy/未知 ownership 改标为 Herdr。

增补（2026-10-21，spec #106）：下段的 Windows wrapper/JVM 时间比较是历史实现记录，随 JVM 与 Windows 原生支持一并退役（ADR 0007）。当前行为契约不变但表示不同：**进程创建身份是不透明文本，只做精确相等比较**。已退役的 Windows 实现曾用 FILETIME，当前 POSIX 实现用 `/proc/<pid>/stat` 的 starttime；不存在时间窗口容差，也不把一种平台的表示换算成另一种。安全 restart 由 `scripts/uml_launch.py` 实现：旧 launcher 与 LOVE 身份在 preflight 之前核对，替换前等待记录中那个进程真实退出，缺失、损坏、未知运行时、PID 复用一律拒绝。

公共 Windows wrapper 的 JVM 开始时间按 UTC instant 校验。Java `ProcessHandle.Info.startInstant()` 在 Windows 将 FILETIME 截断到毫秒（JDK 21 实测同进程 Java `.900Z`、.NET `.9008143Z`），因此 Java 的零位或三位小数记录与实际开始时间截断到毫秒后比较，不采用时间窗口容差。六位、九位 Java 记录及七位 .NET 往返记录保持精确比较；九位记录若包含 .NET ticks 无法表达的亚 tick 部分则拒绝。只接受完整 UTC `Z` 时间及这些小数位数，缺失、损坏或无法确认的身份仍拒绝。PowerShell 7.5 会自动将 JSON 时间字符串转成 DateTime，因此读取原始 JSON 字符串以保留记录精度。launcher 继续使用 UTC ticks，PID、进程名、Herdr ownership 和 30 秒等待契约不变。

#28 自动验证覆盖应用恢复及隔离的 Windows 公共 wrapper fixtures（受控 JVM/进程），不启动或重启用户正在运行的 viewer。此验证不等同于真实 Herdr 端到端验收；Windows 原生验收仍为部分完成，macOS 未执行验收，不能声明通过。

增补（2026-10-15，spec #32）：上一句的 macOS 部分自此过时——macOS 端到端原生验收由 spec #32 执行并通过（Herdr 0.9.4 / protocol 22，Darwin 27.0.0 arm64），wrapper --restart 身份连续与视图恢复、质量分发串行门控均实测通过，验收日志见 acceptance-evidence/acceptance-log.md；平台终端 attach 未验证，移交 #49。Windows 原生验收仍为部分完成。

同图 `:display` 以 canonical file 身份比较路径，重新读取 IR 和质量快照，恢复 focus、camera、zoom、proposal、open layer、declutter 以及仍然有效的 selection/detail card。相对路径按 project root 解析，当前加载路径按其原始文件位置比较。focus 消失时退到最近有效的 namespace 祖先；proposal 消失时回 namespace tree；open layer 消失时回该 proposal 根；失效 selection/card 清除，不创造 proposal。不同图加载采用新图根视图。discussion context 独立于浏览视图，直到后续 `:context` mail 才改变。mtime/metrics reload 与 FIFO mail 契约保持不变。

companion 提示需写明质量命令失败停止后续 IR/展示，以及项目声明的 mutation 退出 3 为有效结果。

增补（2026-11-07，#201 / UV-5，ADR 0010）：原生 Windows 的当前 Rust/Python
实现用 GetProcessTimes 创建 FILETIME 的十进制 u64 文本，Rust companion、viewer
与 Python wrapper 完全一致；只做精确相等比较，已退出进程视为不存在。
这不是恢复旧 JVM/.NET 的截断/时间容差规则。公共入口为 uml.cmd，正常关闭为
WM_CLOSE；:quit-for-restart 继续保留 companion，--restart 核对原身份后复用它，
恢复保存路径、discussion context、camera/zoom/focus 与 mail-seen。原生 Herdr
pane shell、socket 与 Windows Terminal 细节见 herdr-backend-contract.md §2/§4/§6。
