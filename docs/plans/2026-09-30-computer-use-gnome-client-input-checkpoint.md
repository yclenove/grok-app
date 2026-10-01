# Computer Use：原生 client 输入红色验收

目标仍为**接手 grok 的工作，完成 Computer Use 所有功能开发，推进到最终版**。
状态 **active / partial — not releasable**。这是继续定位并固定 P0 的进展，**不是修复完成**。
文档日期为开发环境指令的 2026-09-30；`20261001` 是已有本地运行目录标签。

## 本次实际变化

1. 完成上一 installed-VM/umask 阶段归档。首次 seal 错把 Windows Git 的 LF→CRLF
   stderr 警告当成 diff-check 错误；保留原脚本/日志，重新分离 stdout、stderr 与退出码，
   实际 `git diff --check` 为 0，stdout 空，stderr 仅匹配换行警告。恢复归档前逐项核对
   已存在 source tar 和 VM 证据，不覆盖冻结字节。receipt SHA256 为
   `fa25e9f420c8d3f34f161b7b10fb86ab64bbb2cb23ba6743bc193f1ea2ab6097`；
   独立 Node 校验 **880 条记录**及原成功/失败、进程 join 和明确未完成状态。
2. 新增 `tools/computer-use-gnome-shell/client-input-probe.py`：只允许明确 owned VM
   的非 root 用户，在真实 Wayland GTK 窗口记录输入类别计数。原生 backend、活动窗口、
   私有 0600 Unix socket、同 UID/token、原 Shell owner 均受约束；不记录按键内容/坐标，
   不读取 `/dev/input`、不授予输入、无任意命令入口。
3. 新增 `input_acceptance.py` 与 **8 项 oracle 单元测试**，CI 同时接入现有
   Node policy **10 项**。完整七项、实际 client receipt、helper generation、相同
   owner/epoch/PID、未锁/前台/native Wayland 都必须满足；不允许过滤、空集、重复项、
   代次倒退或内容字段被误判为通过。CI 单测只是验证判定器，不是真实 OS 输入验收。
4. 明确重启同一个已退出并 join 的独占 VM，attempt=7，原 QEMU PID9846；没有在
   观察超时后重启活进程。只在来宾暂时启用已核对 SHA 的原 helper，产品开关/授权未启用。
   前后均明确停用并验证 endpoint absent。原 SSH/fixture 子进程及最终 QEMU 正常退出并 join。

## 两轮真实红色结果

目录：`tools/computer-use-probe/.run/gnome-input-source-20261001/`。
每轮的两个连续 pointer move、button down/up、scroll、Shift key down/up，共 **7 项**，
实际 GTK client 对应计数均 **+1**，同一 helper epoch 的代次均 **+0**。
`client-delivery-001.json`、`client-delivery-002.json` 均是 **0/7 pass**，不是 skipped。

首轮出现 fixture 清理阶段重复调用 `gtk_main_quit` 的警告，已保留原日志；第二轮在
检查 main-loop 存在后再 quit，日志无该 warning/critical，输入漏报结果不变。
第二轮 fixture SHA256：`e6761c3739de4028b453c80d134da996975f79925f11767edab91f7ebff8576c`。
原 SSH exit=0、原 child joined=true 只证明测试进程正常收尾；验收进程仍因 **P0 而 exit=1**。

这排除了“QMP 未到达 client”作为本轮漏报的解释，也防止未来只看 helper 启用或
Shell panel 活动就宣称接管保护成功。QMP USB 是来宾内核硬件路径，不冒充真人输入；
GTK key 事件不冒充中文 IME；没有运行真实 App grant 撤销、EI 排除、touch/tablet/grab。

## 原生观察源调查边界

本轮另存官方 GNOME/mutter **46.2** 的 backend/seat/display/stage/input-capture 源码
与 URL/SHA。原早期 client routing STOP 结论仍成立。`last-device-changed` 实现对同一
device 直接返回，并通过 idle callback 发出变化，不能当逐事件观察器。新测试刻意保留
同设备连续动作和 down/up 两条边，不能用 last-device、idle 轮询、窗口 user-time 或
减少动作种类让门槛变绿。

尚未验证可用的新 pre-routing/native 观察源；**没有改写 Mutter、重排其 filter、
增加 input 组权限、读取用户输入设备或安装提权服务来绕过边界**。下一阶段必须落实
不被 client 消耗的内容最小化观察源，再把此红色门槛转绿并扩展完整输入/接管链路。
当前仅新增可重复的真实验收，不宣称已经修复源头。

## 冻结和最终版范围

- 上一阶段 archive/receipt 保持不变；本阶段新增 fixture/oracle/文档及 CI 接线另行冻结。
  老 receipt 的工作树 SHA 是当时状态；之后正常编辑的文件须查旧 source tar，不能
  用新工作树覆盖旧证据或伪称所有旧工作树记录永远不变。
- 本阶段运行 Node policy 10、Python oracle 8、实际 client 红色验收 7×2，以及
  quality/diff 检查。上次 full Wayland 171、Host 管理 4、Rust Clippy 等仍属于
  [上一阶段](2026-09-30-computer-use-installed-gnome-vm-checkpoint.md)，本阶段不重复冒领。
- 仍是 GA Mesa24.0.5 的 owned 对比 VM；Mesa25.2.8/virtio 的 Shell 崩溃未解决。
- Windows x64、macOS arm64+Intel、Linux X11/Ubuntu24.04 GNOME Wayland；
  Desktop/managed browser/existing Chrome+Edge/App WebView 经 App/ACP/MCP；
  完整输入/中文 IME/clipboard、OS 授权、接管/锁屏/焦点/拓扑、取消/恢复；
  Ubuntu22.04、AppImage/deb/rpm、签名安装/升级/修复/回滚/卸载、原生 UX/DPI、
  真实 Grok E4、**同一最终冻结候选 12h active soak**全部保留，未完成的不标绿。

本轮属于 progress：新增可执行测试和更强实际反例证据。目标保持 active；没有通过
完成审计，不调用 complete；也不是连续三轮相同外部 impasse，不调用 blocked/paused。
