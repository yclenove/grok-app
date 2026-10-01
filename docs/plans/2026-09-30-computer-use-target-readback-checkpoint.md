# Computer Use：授权目标回读与并发撤销

日期：2026-09-30。状态：**active / partial — not releasable**。
继续原「所有功能开发、推进最终版」目标；以下是实际产品修复与验证，不是全目标完成声明。

## 接续的已验证状态

上一轮剩余 Windows、Linux、运行器负例任务均已读取原句柄，确认退出码 0，没有因观察超时重启。
`wayland-parent-registry-final2-20260930/receipt.json` 已经 Node 与独立 Python 两次核对
4,294 项文件记录，SHA-256 为
`70953f861f5485da08a1c16a444cba4a336295f7cae9caee33b20be08709938d`。
这仅封存上一候选；本次源码继续向前推进，不能拿旧 freeze 冒充新代码的证明。

## 实际缺陷与修复

发现 App `authorized_target_dto` 在授权后重新调用**不带 run 的发现列表**，失败时自行编造
`desktop/window` 等元数据。这不仅丢失真实 Wayland monitor/WebView 信息，还会在 Stop、
同一 targetId 换代和界面类型不一致时返回过期授权结果。

先在真实 App test artifact 上运行 5 个回归用例，得到 **0 passed / 5 failed，exit 101**，
日志、失败 test EXE、构建输出和原产品源码副本均保留。随后修复，未放宽断言。
失败时的全部测试源码没有单独冻结，因此不宣称拥有完整红灯阶段 source snapshot；
原始失败产物及两个产品源码副本可审计，新候选源码则完整冻结。

- Broker 新增 `authorized_target_info(run, surface)`：只访问已经绑定的 executor 和原 run，
  不使用全局发现、不把 browser profile picker 当成已授权 tab，不跨界面回退。
- 原适配器回调期间不持 Broker mutex。回调前后核对相同 target generation、executor 身份
  与运行状态；Stop/feature-off/同 targetId 重新授权后，旧回复均被拒绝。
- 缺失、重复、死亡或失败的目标元数据不再伪造成成功授权；App 将原错误交给既有失败撤销流程。
- App 直接转换 Broker 返回的真实 target，删除手工构造 window/managed-tab 等 fallback。
  不增加用户文案、不改变 App shell、不开放模型自授权，也未更改 GUI consent 权限边界。

## 新候选验证

证据目录：`tools/computer-use-probe/.run/wayland-app-target-readback-20260930/`。
346 源文件冻结，相对上一候选 5 项变更、2 项新增；执行后核对源码哈希。

| 检查 | 本候选结果 | 证明边界 |
| --- | --- | --- |
| 新 Broker 用例 | 3 通过 | 四 surface 的绑定路由 double、死亡/Stop，以及产品 managed adapter + recording worker 的实际 tab grant；不是浏览器实机 E4 |
| 新 App 回读用例 | 9 通过 | 最初 5 个失败回归 + feature-off、预检拒绝、死亡、重复元数据 |
| App 命令/WebView/session-MCP/feature/supervisor | 13/69/21/1/7，共 **111** | 实际 Windows App lib test artifact 的独立 manifest 副本，原 EXE 不变；不是全 App 1,746 项测试全跑 |
| Windows/Linux core | **590/589**，0 failed / 0 ignored | 全量库测试，平台条件用例数不同 |
| Windows/Linux driver 与 FFI contract | 各 **16/21** | FFI double 不是 macOS 实机验收 |
| Wayland 同一 executable | **105 × 3**，0 failed / 0 ignored / 0 filtered | 私有 Portal/PipeWire/libeis/labwc，线程 1/4/4，不是 GNOME |
| GTK parent 原生 probe | 三轮各 **12 Wayland + 4 X11 负例** | 每轮 9 exports / 9 destroys，原 compositor/Xvfb owners 精确 join |
| X11 unit | **49** | 本阶段未将历史 native 19 / AT-SPI 41 算作新跑结果 |
| Linux App + 四原生 crate 严格 Clippy | 均通过，all-targets、`-D warnings` | 完整 App 编译面，不等于安装态 UI/权限验收 |
| 实际 Windows/Linux App cargo check | 均通过 | Linux 仍使用独立 seed resource overlay，不改 Windows seed |
| fmt、include-command fmt、diff、代码质量 gate | 通过 | 质量脚本中的 FINAL=PASS 不表示 Computer Use 全目标完成 |

原生 GTK→Registry→Broker 测试增加真正的授权回读断言：返回原 `wayland:` target、
`monitor`、`Wayland portal` 与 `wayland-portal`；unmap 后回读立即失败，不再显示旧授权。
每轮原始 `parent-registry.json` 保存实际返回字段与错误；Node/Python 独立检查这些字段、
原 compositor handle、PNG 与 C EI events，165 个 PW/EI 夹具目录无缺失、owned residual 0。
输入仍只发生在私有 fixture 中，不能把 Applied 或元数据正确冒充目标应用效果已验收。

`seal.mjs` 验证源码、产物、日志和上一收据；独立 `verify-independent.py` 重新计算哈希并
复核原生协议与原始结果。最终收据以本目录 `receipt.json` 为准，失败尝试不改写。

## 完整剩余范围不变

Linux App **原生 consent/factory 尚未接通，native_wayland=false**。
当前私有 SDK 的 GLIBC_2.38 需求仍不能证明 Ubuntu 22.04 发行包兼容，不能上调发行基线替代修复。
原目标仍包括：Windows x64、macOS arm64/Intel、Linux X11/native GNOME；Desktop、managed browser、
existing Chrome/Edge、App WebView 经 App/ACP/MCP；完整输入/中文 IME/clipboard/取消恢复；
真实 OS 权限/锁屏/用户接管/恢复；签名安装/更新/修复/回滚/卸载；原生窄窗口/DPI/权限 UX；
真实 Grok E4；**同一最终冻结候选 12h active soak**。

更早 Windows prepare/即时 PipeWire 节点快照/初期 residual 身份未证的问题仍保持各自原始状态。
没有提交、推送、tag、发布、签名或安装；没有将目标缩成仅单元测试、私有协议或仅 Wayland。
目标保持 active，本阶段不调用 complete/blocked/paused。
