# 0018 移除原生 Windows 的构建号下限

**Status**: accepted

本 ADR 修订 [ADR 0010](0010-native-windows-delivery-platform.md) 中"原生 Windows 从
Windows 10 22H2（build 19045）起交付"的版本下限条款：**取消构建号下限**，只保留操作
系统判断。ADR 0010 的其余条款（adapter 边界、mutator 隔离约定、快照跨平台一致）继续
有效。

## 背景

本仓库的 Windows 构建在构建它的机器（10.0.19042.630，Windows 10 20H2）上无法生成图：

    IR generation failed: Windows build 19042 is not supported: the minimum is Windows 10 22H2 (build 19045)

先把下限下调到 19042 曾是一版方案，但那个数字本身是任意的：19045 只是"当时最后一代
受服务的 Windows 10"，且 1904x 各版本共用同一 servicing base。

## 决定

取消 `crates/platform` 的构建号下限：

- `decide` 只判断 `std::env::consts::OS`：Linux / macOS / Windows 直接通过，其他 OS
  拒绝并指名交付平台；
- 删除 `MINIMUM_WINDOWS_BUILD`、`Platform.build`、`host()`、`host_build()` 与
  `RtlGetVersion` 的 FFI 绑定（约 40 行），连同"构建号必须从内核读而非 manifest shim"
  那条测试；
- 交付范围的支持声明留在 README 与 ADR，不再用运行时硬拒表达。

## 为什么是移除，而不是再选一个数

| 依据 | 结论 |
|---|---|
| 构建号的消费者 | 全仓库只有一处比较；`RtlGetVersion` 的唯一用途就是产出这个数 |
| 实际使用的 Win32 API | kernel32 / ntdll：Job Object、`LockFileEx`、`MoveFileExW`、`CreateProcessW`、`GetProcessTimes`，全部早于 Windows 10 |
| 原生渲染器 | 已随 #30 删除；`crates/frame/tests/no_window_deps.rs` 断言工作区不含 egui/eframe/winit/wgpu，ADR 0010 表格里 "wgpu 原生 DX12" 一行因此失效 |
| Windows 7/8 | 二进制根本起不来（Rust std 要求 Windows 10+），门禁在那里本就空转 |

即：门禁区分出来的 10240…19044 全都能正常跑，它拦不住任何真实故障，只产出一行错误文本。
任何具体数字都会在下一次换机器时重复同样的讨论。

## 被否决的选项

- **下调到 19042（Windows 10 20H2）**：能解当前这台机器，但数字仍任意；曾按此写过一版，
  被本决定取代。
- **保留检查、降为一行 stderr 警告**：仍要为产出一个没人据以行动的数字而保留整套 FFI；
  警告通道只在有可执行动作时才有价值。
- **只降我们自己的 crate、`quality-core` 保持不动**：曾是本决定的实际形态（见下），
  但它不是完整答案，完整答案要等上游；上游 ticket 把同一处改动写进各自的提交后，
  `quality-core` 随之移除。

## 后果

- 原生 Windows 上不再有"构建号过低"的拒绝；真正不支持的 OS 仍以一行明确原因拒绝。
- 质量三件套（crapper、dryer、mutator 的 Rust 端口）**的 fixture 钉在承载该改动的提交上**：
  改动本身由上游 ticket qinyuanj/crapper#18、qinyuanj/dryer#5、qinyuanj/mutator#30 承载，
  而这些提交由各自的 pull request 携带（分支已推送，尚未合入 main），所以下面引用的 SHA 可以在
  远端解析，但还不属于 main；三张上游 PR 合入后需再 pin 一次，把它们对齐到 main 上的提交。本仓库在 qinyuanj/dsh-uml-viewer#77 按这些提交
  重新 pin（crapper `91cd10c`、dryer `d452700`、mutator `436d5d3`）并重生成
  `fixtures/{crap,dry}/platform/cases.tsv`；`crates/quality-core` 不再有
  `MINIMUM_WINDOWS_BUILD`、构建号参数、拒绝分支与 `RtlGetVersion`。
- fixture 仍由 `scripts/*_platform_oracle.py` 从 pin 生成并写明 do not edit，而每个 oracle
  在生成前先断言被钉仓库的 `HEAD` 等于 `PIN`：一个不含该改动的仓库会让 oracle 立刻失败退出，
  而不是安静地产出另一批行。现在钉的是两个原件仅剩的平台决定——WSL `/mnt` 警告（原件已无
  拒绝可钉）。dryer 的 pin 具体是 `d452700`：它在 dryer#5 之后撤掉了自己新增的“其它 OS
  拒绝”，所以 dryer 的唯一行为变化就是“任何 Windows 构建都跑”。
- 低于 22H2 的机器上 viewer、IR 与质量刷新都可用；`quality mutate` 仍拒绝 macOS，那是
  本端口自己的隔离决定（macOS 没有 namespace view 也没有 copy），与构建号无关。

## 验收

- `cargo test -p platform` 通过：三个交付 OS 接受、其他 OS 拒绝并指名交付平台、本机是
  交付平台。
- 本机 10.0.19042.630：`uml-viewer ir examples/uml-viewer.policy.edn` 产出 IR。
- 工作区不再有 `RtlGetVersion` 绑定与 `MINIMUM_WINDOWS_BUILD`，`crates/quality-core`
  也不例外：本机 10.0.19042.630 上 `quality crap|dry|mutate` 均不再因平台被拒。
