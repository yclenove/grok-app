# Computer Use：原生普通输入、锁屏监控与回归门槛修复

日期：2026-10-01。完整用户目标仍是「接手 grok 的工作，完成 Computer Use 所有功能开发，推进到最终版」。
状态：**active / partial — not releasable**。这不是最终版、完整 P0 关闭或发布批准。

## 实际改动

- 新增严格 `lock_acceptance.py` 与14项回归、CI 调用：必须真实 unlocked baseline、原 Shell/session、先订阅后快照、先收到 blocked Changed 后读回、同 epoch 连续性。缺失 endpoint、重启、伪造/迟到通知不得改成成功。
- 新增受精确 VM UUID、UID1000、root-owned 0444 marker 和原生图形会话约束的 `installed_lock_probe.rs`。使用真正的生产 `GnomeNativePolicyWatch`，但 Host 输入策略始终 false；不建立 portal/EI/App grant。
- 全量回归发现测试 fixture 在 zbus async-io executor 上调用 Tokio timer：测试线程可绿、后台线程却 panic。改为不依赖 runtime 的通知等待，先注册再检查状态，显式 release，fixture Drop 也释放等待；不改变生产监控逻辑或 workspace zbus feature。
- 新增真实 D-Bus held GetUser 先保持 pending、再由原请求恢复的 Rust 回归。Wayland runner 同时审核 stdout 和 stderr；新增3个契约，禁止后台 panic/SIGSEGV 被 libtest 绿色摘要掩盖。

## 同一实际 GNOME 会话的证据

owned VM UUID `812400f8-a6c6-4c38-a735-2b8d0ef8d3e2`，attempt15、原 QEMU PID30914、原句柄65807。
实验均属于 Shell PID2474 / unique owner `:1.25` / login1 session5 / local active seat0 native Wayland。
仍使用上阶段精确 Mutter DSO 与 RUNPATH-only Shell 副本；生产 helper 未接入实验计数器。

| 检查 | 原始结果 | 能证明与不能证明 |
| --- | --- | --- |
| 普通 native GTK 输入002/003 | 两轮7/7；每个 client 与代次增量均1 | 两次指针移动、左键按下/抬起、滚轮、Shift按下/抬起。不是全部输入、真人或 App 授权。 |
| 真实 native policy watch001/002 | 原监控先存活350ms；锁请求后2ms/3ms因 login1 lock signal 终止并 join | 同一原 Shell、LockedHint=true、原图形 launcher/进程收尾已核验；不是已授权输入撤销或原子 stop 延迟。 |
| 严格连续 helper 锁屏001 | **失败，保留红色** | 收到原 epoch blocked Changed 后，GetState 报 UnknownMethod / endpoint不存在；不能证明同 epoch 连续读回。 |

普通输入仍使用未修改 `input_acceptance.py` 和 fixture SHA256 `e6761c3739de4028b453c80d134da996975f79925f11767edab91f7ebff8576c`。
初次001在焦点预检失败、没有执行七个 case；记录 windowActive=false 和 client计数0。随后仅增加启动/焦点点击后的稳定等待，并保存实际 QMP截图，未放松焦点判据或改写 oracle。

严格锁屏记录：baseline `[1,"1e909bcd-a685-42e0-9931-6819e2cf4bb8",1,false]`；原 sender`:1.25` 的通知 `[1,same epoch,2,true]` 在后续 GetState 前到达。读回失败即红色，未捏造 after snapshot。
从实际加载的 Shell resource 提取的 extensionSystem/sessionMode 显示扩展重排会 disable/re-enable；这是生命周期机制的源码证据，**不是实际调用栈证明**。未通过禁用 Ubuntu 扩展、重排顺序或修改 Shell JS 制造连续性。

原生 watch 的实际二进制002 SHA256：`4e53799d413e0078a526eccb9d9c89a98ce319965066a35a646482b153cf3aab`，177782080字节。
它通过真正 `gnome-terminal --wait` 执行；原 monitor future、原 launcher、原 SSH/进程均收尾。新 probe 与生产库不是用模拟结果代替；但 Host policy全程false，所以所有 actualGrant/atomicStop/unlockRecovery 字段仍false。

## 回归中的失败及修复

- 001/002私有全量运行在进程启动前失败：root映射后的 user namespace 无法遍历 UID1000 mode0700 SDK/test目录。未 chmod，改为使用该私有 SDK 的原 UID1000。
- 003实际执行完整188项，184通过、4失败。继承环境指向旧 `owned-pw-source`：缺少当前 `MODE` 控制协议和旋转实现，导致命令无确认/源移除。重新从当前 `pw-source.c`、`eis-server.c` 严格编译，不修改生产 PipeWire/输入判据。
- 同一次 stderr 暴露3次 zbus executor 无 Tokio reactor 的 panic。第一次尝试绑定 runtime Handle 后，004虽189项全部绿，仍出现 runtime shutdown panic；**新 stderr 门槛正确拒绝004**。保留该中间源码和原始日志。
- 最终改为 runtime-independent Notify，无跨 await runtime guard；增加原请求 pending→reply 回归。005一次编译因 fixture字段可见性失败，修正后再构建；编译诊断保存。
- 最终006（1线程）与007（4线程）使用**同一个冻结测试二进制及当前两种 C fixture**，均 **189 passed / 0 failed / 0 ignored / 0 filtered**；stdout与stderr无 panic/崩溃。GTK原 UI owner 集成在完整集合内，不是过滤测试。
- 两次运行都在私有 PID/network/tmp namespace 下；原 test/compositor 已退出0并 join，原后代回收、live/errors为空。这是私有协议/GTK回归，不能替代 installed GNOME/App/E4。
- 当前 all-targets strict Clippy、Rustfmt、Node33、helper Python46（含新lock14）、overlap Python10、runner Python20、语法/YAML/quality/diff通过。早先“158通过、30 ignored”的普通 cargo test 不冒充完整189。
- 早期不存在 quality脚本、错误resource路径、权限预检等失败均保留；未把每次尝试描述成绿。历史全平台、Mutter773-step构建、C sanitizer、真实 IME/EI配对和12h soak 未在此阶段重跑。

## 候选、撤销与原进程收尾

实验 libmutter SHA256 `33c6ce53a99820a4173c2b320798d63cd6904fc83d61145f84e5fcf000a8800d`；Meta typelib `7ca9d706187621212dfe0e8da02f3bf1c9af3818dccf5b656090034d31166e03`。
私有 Shell副本 SHA256 `0e5040f66eb43fdba9a20641eda8c24fcba3945c5a7bad270d31821ee164cff2`；系统原件 `5f678999f051dd1e17f2da50cb55ddc152d9b1f5d45a6ea8ead8b4df603b2a17`。使用历史逐段对比证据和当前真实maps确认同一加载方式，不宣称分发支持。

各轮 helper都停用并确认 endpoint缺失；实验四文件按hash移出扩展发现目录。仅删除精确 owned user-service override，恢复后实际新 stock Shell6991 / owner`:1.24` / session53 的 system-only maps 已回读；`dpkg -V gnome-shell libmutter-14-0 mutter-common`无差异，生产helper各文件hash未变且 disabled。
输入源恢复 `[('xkb','us')]`。最终原 candidate及所有probe/client PID均缺失，成功client socket目录缺失。VM正常关机，**原65807实际退出0并join**；PID30914缺失、SSH42791拒绝连接。未重启该VM，未改用户桌面/输入权限、系统compositor或发布。

候选 journal 的 UbuntuDock invalid-NULL、actor allocation、DockManager-null、Dbusmenu等警告保留；不声称无警告。环境仍是 Ubuntu24 GA Mesa24.0.5 对照VM，不等于 Mesa25/virtio问题已解决。

## 证据与冻结

阶段目录：`tools/computer-use-probe/.run/gnome-counter-controls-20261001/`。

- `guest-evidence/lock-001.json`、两组 `native-watch-*.json`/原guest日志、`ordinary-controls-{001,002,003}.json` 和实际截图：成功、红色结果和原来源均保留。
- `owned-session-artifacts.tar.gz`：33文件、82637字节，SHA256 `de5bfe3e2ca1f371b3ead880bf597479149ac42044ed27aac22e327d1eb1f3b9`；归档/解包成员逐项hash验证。
- `wayland-suite-{003,004,006,007}.tar.gz`：失败及最终完整套件的原stdout、stderr、owner收尾、全部可识别native fixture输出，原目录也保留。
- `before-checks-sources.*` 保存初始450源；`post-fix-sources.*` 保存最终fixture/runner修复后的450源；`receipt.json`绑定最终文档、源、测试、归档及前一阶段，不只检查 passed布尔值。
- 前一封存 receipt SHA256 `85a58bf5d4d7c2f331a525a229746be4d5577b8bfe3a1198326bc833a2b96f8a` 与其归档保持原样；旧4份文档备份在`previous-documents/`。不以当前 live owner覆盖历史 owner证据。

## 仍需完成的完整范围

| 要求 | 当前证据与下一工作 |
| --- | --- |
| Stock native观察及支持的集成 | Stock物理/EI IME均 `[0,1,0,1,0,1]` 的红项仍在；上阶段实验6/6配对和本阶段普通输入不能替代生产provider、受支持ABI/Ubuntu downstream版本路线。 |
| 全输入与生命周期 | 补齐物理/EI指针滚轮交错、capture/pad/touch/tablet/grabs、设备/源销毁；真实已授权锁屏/焦点/拓扑/重登录/owner丢失，以及明确 fresh consent恢复。当前只读watch成功不关闭这些项。 |
| App/ACP/MCP所有目标 | 验证真实授权、执行、停止、取消、恢复及原资源收尾；Desktop、managed browser、existing Chrome+Edge、App WebView 全范围不变。 |
| 全平台最终候选 | Windows x64、macOS arm64/Intel、Linux X11、Ubuntu24 installed Wayland、Ubuntu22 baseline，必须分别具备当前最终候选证据。 |
| 最终交付 | 全部输入/IME/剪贴板、签名安装/更新/修复/回滚/卸载、UX/DPI、真实 Grok E4、同一冻结最终候选12h active soak仍需完成。 |

继续保持目标active，不 commit/push/tag/release，不把局部189项或原生监控通过改写成全功能最终验收。
