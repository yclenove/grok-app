# Computer Use：原生指针交错与输入源退出验收

日期：2026-10-01。完整目标仍是「接手 grok 的工作，完成 Computer Use 所有功能开发，推进到最终版」。
状态：**active / partial — not releasable**。本阶段不是最终版、全输入关闭或发布批准。

## 本轮实际改动

- 新增 `pointer_acceptance.py`：严格15步物理/EI交错、同按钮重叠、拖动、滚轮、源退出与后续物理恢复 oracle。要求同 Shell owner/client PID/helper epoch，原生前台焦点、无 blocked/饱和/倒退和步间未解释事件；不能以客户端有输入代替来源区分。
- 新增 `pointer_acceptance_test.py` 的15项契约并接入 CI。缺步、乱序、额外输入、虚拟误报、物理漏报、计数/owner/epoch异常及退出后缺失释放必须失败。
- 新增严格 owned-VM 的 `ei-pointer-peer.c` 与 `ei-pointer-launcher.py`，只接受六个固定命令，不提供任意输入API或产品授权。真实 Shell owner/UID/PID绑定，连接的是受限测试 transport，不是 portal consent。
- peer最终保持右键，普通断开不显式发送补偿释放；新增 kill 模式强制终止原来仍活着的 child，要求实际 `-SIGKILL`、真实 join、没有伪造 cleanup reply。随后检查原 session Closed/endpoint消失，并验证物理输入恢复。
- 新增 `POINTER-ACCEPTANCE.md`，更新 compositor README。生产 helper/counter wiring/App grant代码未改；默认关闭和既有红色判据没有被绕过。

## 真实 installed GNOME 结果

owned VM UUID `812400f8-a6c6-4c38-a735-2b8d0ef8d3e2`，attempt16，原 QEMU PID45338 / 原句柄61354。
测试全部使用同一实验 Shell PID2469 / unique owner `:1.25` / login1 session5 / local active seat0 Wayland。
Mutter DSO仍为 `33c6ce53a99820a4173c2b320798d63cd6904fc83d61145f84e5fcf000a8800d`，保持前阶段相同 RUNPATH-only私有 Shell 加载方式；系统库/程序没有被替换。

| 实际运行 | 结果 | 证明边界 |
| --- | --- | --- |
| pointer-001 | 预检失败，0个 case | 默认umask生成group-writable peer，launcher正确拒绝；未降权限检查。 |
| pointer-002/003 | 初版 launcher各15/15 | 普通断开、右键自动释放、后续物理恢复；作为早期版本证据保留。 |
| pointer-kill-004/005 | 最终 launcher各15/15 | 原peer分别PID4363/4646，实际退出-9，`peerStopped=null`；源销毁后客户端收到一次释放，后续三步物理输入正常。 |
| pointer-disconnect-006/007 | 同一最终 launcher各15/15 | 原peer分别PID4903/5158，实际退出0；保持右键断开，没有额外注入释放；同样恢复。 |

最终四轮每次 helper代次增量均为 `[0,1,0,1,1,0,1,0,1,0,0,0,1,1,1]`。
两个物理重叠边沿在 GTK 的所有类别增量均0，但原生代次仍各增加1；EI运动/拖动/滚轮/按钮及源退出的物理代次都不增加。
每个非重叠步骤只收到期望类别；源退出后的释放正好一次。快照连续，未靠步间重置计数、重启 helper或丢弃多余输入过关。
原生GTK fixture保持 SHA256 `e6761c3739de4028b453c80d134da996975f79925f11767edab91f7ebff8576c`，只记录类别计数，不记录输入内容。

严格C11构建（`-Wall -Wextra -Werror`，实际libei1.2.1）及错误VM身份拒绝通过。
实际peer SHA256 `62af7a90760782edfbe6b8ebf449aaa5b22f734b5d85bc85f75c45a85b6d2157`，21992字节。
最终launcher SHA256 `1402018407aa47ca958373d1f1043fb8fc521f988bd6812c908d72c66c04e449`；早版源码/二进制/失败日志保留。新建私有build目录使用umask077，未 chmod已有SDK或放松peer安全门槛。

## 原资源退休与恢复

所有测试原 SSH/RPC/EIS launcher/client进程均已真实退出并join。原实验Shell仍在时的独立回读确认：13个原peer/client PID、六个原bus connection/session endpoint、七组client/socket目录全部消失；helper停用且endpoint缺失，生产helper文件hash未变。

仅退休精确自有probe四文件及user-service override，恢复实际stock Shell5653 / session34。恢复脚本第一次在“unique owner字符串必须变化”的旧断言失败；原样保留错误。诊断发现新Shell的名字也为`:1.25`，不可把跨会话的名称字符串当全局身份。
后续**只执行读回，不再次重启**：核对 D-Bus owner实际绑定PID5653/UID1000、新session34、原PID2469缺失、实际`/usr/bin/gnome-shell`、全部system-only Mutter maps和包完整性。记录新bus ID `36f7c04e01b32e3c812d76dd6abdc2da`及进程start61303；不能据此声称已验证生产watch的跨bus owner-loss撤销。

`dpkg -V gnome-shell libmutter-14-0 mutter-common`无差异，系统Shell SHA256仍 `5f678999f051dd1e17f2da50cb55ddc152d9b1f5d45a6ea8ead8b4df603b2a17`；production helper disabled且endpoint absent，probe不再可发现，输入源保持`[('xkb','us')]`。
VM正常关机，原句柄61354实际退出0并join；PID45338缺失、SSH42791拒绝连接。未改变用户桌面/输入权限，也未commit/push/tag/release。

## 证据与回归

阶段目录：`tools/computer-use-probe/.run/gnome-pointer-interleave-20261001/`。
`sources-001.*`/`sources-002.*`保留两版冻结源码；`source-freeze.json`/`selected-sources.tar.gz`绑定最终455源。`receipt.json`绑定当前源、四份文档、实际运行与原失败，不只检查passed字段。
原始行在各 `pointer-*.json`，原子进程输出/exit/join在对应log和process记录；最终focused截图已检查为实际GNOME native GTK client，不能外推产品UX/DPI验收。
guest归档含23文件、85864字节，SHA256 `63ea6c949389b5ada03f7a0ba449493a466ae68133bc0e704a4806ebf4335a9e`；包含exact peer/两版launcher、候选/恢复maps、journal、原session/retirement/rollback记录。候选日志原样保留，不声称零警告。
上一阶段receipt `e69bc7f0dd80c8e8dc31fd71c9d39ce9ce5761628dd6a6c449dbb67afbc7ca30`未覆盖，初始450源和4份文档已核验并备份。

当前回归：Node33、Python61（含新15）+overlap10、Python/JS语法、CI YAML、quality final与diff通过。
此前Wayland189×2、Rust strict Clippy、Mutter全构建/ABI/线程sanitizer、IME6/6配对及只读锁屏监控属于历史证据；本阶段没有重跑，也不能冒充新整机全功能验收。

## 完整目标仍需推进

- **原生输入/来源：** capture、touch、tablet-tool/pad、其他grab、物理设备移除、compositor/bus owner丢失。当前QMP USB-tablet只用于普通指针，不等于绘图板/触摸覆盖。
- **真实授权链：** 已授权App/ACP/MCP的锁屏、焦点、拓扑、取消/停止、原子边界、原资源收尾以及明确fresh consent恢复；只读watch和EI测试session不是这些证明。
- **支持的生产集成：** stock GNOME物理/EI IME红色baseline `[0,1,0,1,0,1]`不变；实验Mutter ABI/RUNPATH路线不能替代受支持Ubuntu downstream交付或provider接线。严格同epoch锁屏读回红项仍保留，不要求锁屏后保留旧grant。
- **全部平台与目标：** Windows x64、macOS arm64/Intel、Linux X11、Ubuntu24 installed Wayland、Ubuntu22；Desktop、managed browser、existing Chrome+Edge、App WebView通过真实App/ACP/MCP覆盖。
- **最终交付：** 所有输入/IME/剪贴板，签名安装/更新/修复/回滚/卸载，UX/DPI，真实Grok E4及同一冻结最终候选12h active soak。

未完成项不改写为通过、不缩小范围，目标保持active。
