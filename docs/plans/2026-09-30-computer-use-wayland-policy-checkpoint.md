# Computer Use：权限撤销的主动回收与不可复活授权

日期：2026-09-30。状态：**active / partial — not releasable**。
继续原「所有功能开发、推进最终版」目标；本阶段不是 native GNOME、安装态 App 或全目标完成声明。
证据目录：`tools/computer-use-probe/.run/wayland-policy-revocation-20261001/`（目录沿用本地运行标识）。

## 真实缺陷与产品修改

原 PortalInputPolicy 主要在操作时检查。等待 portal consent、已授权但空闲的 Registry、
独立 PortalAdapter 在权限撤销/用户接管后，可能一直保留 native owners，直到下次模型调用或外部 Stop；
简单恢复 policy bool 还能让旧授权重新可用。

先编译并运行四个新回归：**0 passed / 4 failed，exit 101**。保留失败二进制、原始日志、
347 条红灯 source hash、完整当时 Wayland crate 副本及三个原产品源码副本。不是完整 workspace 副本。

- 每次显式选择拥有独立 sticky PolicyLease；任何已观察到的权限丢失或接管永久撤销该次 grant。
  恢复底层权限不会复活旧 target；必须 join/forget 原 owner 后重新显式选择。
- Registry 在排队导出 parent、等待原 UI future、portal consent 以及空闲授权阶段持续监督策略。
  取消和撤权都保留、等待同一个 UI/native owner，不丢弃晚到 parent 或偷偷重开 consent。
- 独立 adapter 空闲时也主动回收；执行异步 native dispatch 期间撤权会取消并 join 原 future，
  不做重放；同步 capture 完成后重验策略，不发布晚到观察结果。
- 操作准入拒绝也永久封锁旧 grant；policy 回调不持 adapter status mutex；已撤权但 owner 尚未
  收到撤权/完成回收时，is_idle 不允许伪报空闲。拒绝无效策略的 handoff 不丢失原 owner。
- 10ms 检查只是 Host policy 的回收后备机制，**不是 OS 输入/锁屏检测器**。真实 Host native monitor
  必须锁存短暂丢失信号、提供快速不阻塞快照。本阶段没有伪造 permissive OS policy。

新增九个测试：policy 七项、queued parent 一项、实际 PipeWire/EIS 的双场景原生回收一项。
原生新测试先取得真实 PNG，再改变 fixture policy；由独立 C EIS peer 观察 disconnect，
无需后续 capture/act/Stop 触发回收。验证原 PW owner 已 join、节点消失、零输入事件及恢复后旧票拒绝。
这些是实际 native owner 回收证据，**不是操作系统检测器或应用交互效果证据**。

## 冻结候选验证

348 个主源文件冻结，相对前一候选 335 不变、11 变更、2 新增。
前一收据 SHA-256：`e751d658e54c21a2310efb387af29736edd595764f9dd4bb1d7f869bce4d7e4f`。

| 检查 | 结果 | 证明边界 |
| --- | --- | --- |
| 同一 Wayland executable | **114 × 3**，0 failed / ignored / filtered | 私有 portal/PW/EIS/labwc，线程 1/4/4；不是 GNOME |
| 独立原生目录审计 | **177**，无缺失/owned residual | 每轮 59；PNG 解码、原始 C EI 事件、精确 peer 身份、policy disconnect |
| GTK parent probe | 三轮各 **12 Wayland + 4 X11 负例** | 每轮 9 exports / 9 destroys，原 compositor/Xvfb 全部 join |
| Windows/Linux core | **590 / 589**，无失败或忽略 | 完整库测试，平台条件数不同 |
| Windows/Linux driver / FFI contract | 各 **16 / 21** | FFI double 不是 macOS 实机 |
| 实际 App selected tests | **111**：13/69/21/1/7 | command/WebView/session-MCP/feature/supervisor；不是全 App 1746 项全跑 |
| X11 unit | **49** | 不把历史 native/AT-SPI 结果当成新跑 |
| public runner contracts | **17** | 新增 policy 报告保留与精确私有目录清理 |
| Linux 全 App + 四原生 crate strict Clippy | 通过 | all-targets，-D warnings；不是安装验收 |
| Windows/Linux App check | 通过 | Linux 使用独立 seed overlay，没有替换 Windows seed |
| fmt / command fmt / diff / quality | 通过 | 质量 gate FINAL=PASS 不表示全目标完成 |

Windows App 使用原 test EXE 的独立 CommonControls manifest 副本，原 EXE 哈希不变。
三轮 native 使用同一套冻结 executable，执行前后哈希相同。
Node 校验原始 PNG/协议/cleanup，Python 独立重算收据文件哈希并检查原始 policy/peer/parent/测试结果。
本目录 seal.mjs/verify-independent.py 与 receipt.json 给出确切证据，不用旧 freeze 证明新源码。

## 失败和恢复不隐瞒

- 首次 unit：82 pass / 1 fail / 26 ignored。旧测试把 monitor 回调入口当成命令占用；改为等待真实占用，保留超时/原 owner 断言。
- 首次 native：111 pass / 2 fail / 0 ignored。旧用例在 bool 恢复后继续用旧 grant；将不可恢复的 policy case 放最后，并新增旧 target 已死断言。原取消/no-image coverage 未删除。
- 一次 strict Clippy 报 let_underscore_lock：测试锁绑定改为具名 guard。原日志保留。
- 修正后开发轮：114 pass / 0 ignored；随后才冻结并执行上述三轮。
- 最终 Linux pipeline 首轮 114 已通过后，parent 辅助脚本缺 sdk-path.txt，启动任何 parent 子进程前失败。
  复制与前一候选完全相同的 SDK 路径，先核对现有 executable 哈希，再从 parent 第一轮续跑；
  没有重编译/覆盖已通过第一轮。原 session 7049 exit 1；续跑 session 90434 exit 0。
- 封存辅助脚本一处正则转义语法错误在执行封存前修复，错误版本保留；它不改变产品源码或测试结果。

## 完整剩余范围

Linux App **native consent/factory 未接通，native_wayland=false**；真实 OS permission/锁屏/接管
monitor 和恢复、installed GNOME、完整输入/中文 IME/clipboard 尚不能宣称完成。
私有 SDK 要求 GLIBC_2.38，仍不是 Ubuntu 22.04 发行基线证明；不得上调发行基线替代修复。
原范围包括 Windows x64、macOS arm64+Intel、Linux X11+native GNOME；Desktop/managed browser/
existing Chrome+Edge/App WebView 经 App/ACP/MCP；签名安装/更新/修复/回滚/卸载；
原生窄窗口/DPI/权限 UX；真实 Grok E4；**同一最终冻结候选 12h active soak**。
更早 Windows seed publication、即时 PW 节点快照失败与早期残留 PID 身份的根因未证状态不变。

未提交、推送、tag、发布、签名或安装；不调用 complete/blocked/paused。
