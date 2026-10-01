# Computer Use：installed GNOME 授权、键盘独立性与自动撤权

日期：2026-10-01。目标 **active / partial — not releasable**。
原目标仍是接手 Grok 并完成 Computer Use 所有功能至最终版；本阶段不是全平台发布验收。

## 本阶段实现

1. 新增 `computer-use-wayland/examples/installed_grant_probe.rs` 及 `installed_grant/{identity,ui,run}.rs`。
   仅允许精确标记的 owned VM、UID1000、当前原 Shell owner/PID 和实际 active/unlocked/local Wayland 会话。
   GTK 原生 parent export、生产 `PortalRegistry`、`HostOwnedAdapter` 与授权 ticket 使用真实实现。
   Host 输入策略只在受限 acceptance 例程中显式开放，**不是 App/ACP/MCP 权限链或发布开关**。
2. 真实 consent007 暴露键盘路径误依赖绝对指针区域配对。
   `PortalHostSession::prepare` 对 `Key` 消费同一原 observation，保留一次性 snapshot、身份、generation、geometry、取消、
   EI keyboard capability 和原生 sequence preflight；不再为了无指针动作而调用位置映射。
   `PresentedFrame::into_observation` 不刷新或新造授权。按键的允许列表没有扩大。
   Monitor 键盘权限原本就是 session-wide；window-scoped Host 仍拒绝。
3. Click / 非零 scroll 仍必须唯一配对 portal stream 与 EI region；没有猜测全局坐标、Notify 回退或放宽检查。
   新增原生 PW/libeis 回归：portal 缺 mapping 和 mapping 不匹配两种情况，键盘 Enter 成功、指针拒绝，
   preview/replay/cancel 拒绝，原 portal owners 与 EI peer disconnect 均验证。
4. runner 显式归档 `keyboard-without-pointer.json`，新增精确 owned fixture 归档契约。

## 原生结果与证据边界

证据目录：`tools/computer-use-probe/.run/gnome-installed-grant-20261001/`。

| 检查 | 结果 |
| --- | --- |
| 旧生产代码 + 新回归 `baseline-test-003.log` | **0 pass / 1 fail / 198 filtered**，精确失败是 `no uniquely paired absolute EI region`；原失败保留 |
| 最终完整 Wayland `suite-005-1/4` | 同一二进制分别串行、四线程：每轮 **199 pass / 0 fail / 0 ignored / 0 filtered**；私有 namespace/PW/EIS，不是 installed App |
| installed `consent-010/011` | 同一 probe004、同一原 Shell5031/:1.35、同一 helper epoch，两轮无重叠的实际系统选择、PNG capture、GTK Enter receipt |
| 原生 GTK receipt | 每轮恰为 `(65293, true), (65293, false)`；EI 后250ms仍保持原 grant，没有虚假 physical takeover |
| 实际物理接管 | QMP 虚拟 USB Shift 下/上，经 guest 实际输入路径；helper25→26、33→34，无后续 act/Stop 触发也自动关闭原授权 |
| 原 target | alive/claim/capture/action 均拒绝；先等待原 owners joined，之后才执行例程 cleanup cancel；不是通过 cleanup 制造撤权 |
| 独立收尾回读 | 原10个 probe PID 均缺失、无 running probe unit；两轮原 D-Bus client owner 与共10个 portal request/session path 均消失，原 portal owner 未更换 |
| PNG | 两张640×400实际portal图片，独立CRC/解压/逐行滤波验证并人工查看；显示 owned GTK 验收窗口而非 Grok App |
| 编译/契约 | Wayland all-targets、Linux App preview/default all-targets 严格 Clippy通过；Linux runner22、Node33、Python61+10、fmt/语法/quality/diff通过 |

全量测试二进制 SHA-256：`de8bdadb87614601b21fc8d474c6f570001e4a56158379cef13b11812831717a`。
实际安装 probe004：`f9d13e350ee64b12c8d5e47ecf3ff398be0ee7f46141bd92806286dd275b6b64`。
相对上一份574源 receipt，修改5文件、新增4例程文件，当前冻结 **578** 条。
冻结发生在测试后；随后 `--locked --offline` 同缓存构建回放，前后578源一致，两个产物均与已执行二进制 SHA 完全相同。
这是同缓存一致性核对，**不是独立 clean/reproducible build 证明**。
App Clippy 使用既有真实 Linux 资源的 check-only overlay；不等于 bundle、安装或 E4 验收。

## 失败与局限均保留

- consent001 直接 SSH 启动被真实 login1 绑定拒绝；后续使用原生 user-manager launcher，不伪造会话身份。
- consent002 真实实验 Shell2464 崩溃。发现私有 `mutter-x11-frames` 资源路径缺失并复制精确构建产物后，后续 picker 可工作；
  没有原崩溃 backtrace，**不声称已证明该资源是唯一崩溃原因**。系统包没有被替换。
- consent003 的 F8 不在允许列表，改验收输入为已有 Enter；没有为测试扩大产品权限。
- consent004 超时；005关闭、006因实际 LockedHint=yes拒绝。仅对 owned VM 显式 OS 解锁后再试，不削弱锁屏策略。
- consent007 是键盘位置配对的实际红色证据。portal 响应**有非空 mapping_id**，不能误写成 portal 未提供。
  安装环境的 EI region 状态尚未取得，指针配对根因与真实指针效果仍未证明；禁止猜 region 或归咎版本。
- consent008 将 down/up 放同一 QMP batch，实际 picker 未被点击；恢复分开的原边沿。
  consent009 虽成功但与008重叠，**不计作隔离验收**；010/011前增加实际 running-unit 检查后独立复测。
- baseline初始tar误选 `/tmp`，真正fixture在 `/var/tmp`；保留空原档，另存明确标注的 native fixture supplement。
- Windows运行Linux runner失败（killpg/路径、7 skipped），原日志保留；随后在Linux运行22项全过，未通过增加skip掩盖。
- 回滚后额外stock截图请求因调用用户无QMP socket权限失败，未提权重试或重启VM；不声称有stock截图。
  stock恢复依据是实际 `/proc/exe/maps`、包校验、原PID缺失、同一新bus owner绑定及endpoint缺失。
- 早期构建/脚本错误、自然锁屏、GNOME EIS/virtio警告仍保留，不用绿摘要覆盖。

## 环境清理

原grant进程/对象退出后，禁用实验extension、验证endpoint消失、按精确hash移入本阶段 `retired-probe`。
删除本阶段override及新加的私有frames副本；恢复stock Shell8686/session55、system libmutter，
`dpkg -V` 检查gnome-shell/libmutter-14-0/mutter-common通过。生产helper三个文件hash保持不变且disabled。
最终只对owned VM正常关机；原QEMU attempt17/PID76378由保留的原Popen.wait退出0并join，
独立回读原PID缺失、SSH42791连接拒绝。没有修改用户桌面、系统包或输入设备权限。

## 完整目标与下一步

本次依赖**实验compositor计数器和实验helper**；不是stock GNOME支持。
`appAcpMcpVerified`、`stockGnomeSupported`、`atomicStopVerified`、`freshRecoveryVerified` 均为false。
两轮新进程授权不冒充同进程 fresh recovery；键盘一次Enter不冒充全部输入、IME、clipboard或pointer。

下一步优先取得真实授权会话的 EI capability/region/mapping 生命周期，验证并修复指针实际效果；
继续原授权 lock/owner loss/focus/topology、同进程显式fresh recovery，以及支持的生产compositor分发集成和真正 App/ACP/MCP 链路。
不得用私有 mock 或实验关闭保护的测试替代这些门槛。

Windows x64、macOS arm64/Intel、Linux X11/Ubuntu24 native Wayland/Ubuntu22；
Desktop/managed/existing Chrome+Edge/App WebView；全部输入/IME/剪贴板、授权/接管/锁屏/焦点/拓扑/取消/恢复；
签名安装/升级/修复/回滚/卸载、原生UX/DPI、真实Grok E4、同一冻结最终候选12h active soak，
仍需按原范围逐项证明。没有commit/push/tag/release，没有将goal标记完成或缩小目标。
