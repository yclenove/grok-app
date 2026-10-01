# Computer Use：GNOME ScreenShield 与 login1 组合监控

日期：2026-09-30。完整目标继续 **active / partial — not releasable**。
本次不是最终版、真实 GNOME 验收或对原目标的缩减。
证据：`tools/computer-use-probe/.run/wayland-gnome-policy-20261001/`（本地运行标识）。

## 本次实际进展

新增生产 `GnomeSessionWatch`，组合原有 login1 与 GNOME Shell ScreenShield：

- 只读连接 system/session bus；固定 `org.gnome.Shell.ScreenShield` 的 unique owner，
  并要求 `org.gnome.Shell` 同属这一 owner，而不是把独立 ScreenSaver 代理当作 Shell。
- UID/PID 从总线取得，交给原 login1 owner 的 `GetSessionByPID`，核对与本进程相同的
  具体 session。相同 UID/共享 user bus 不足以跨登录会话授予权限。
- 先订阅后调用 `GetActive`，启动阶段 true→false 脉冲仍永久撤销；owner 替换/丢失、
  session bus 断开、错误类型信号、login1 的任何撤销都终止组合 watch。
- 两个原 future 在同一个组合 future 内直接持有，没有内部 detach。
  未 poll 不授予；stop/drop/cancel/任一监控结束均关闭原监控；晚到 Host callback 不复活。
- 仍要求调用方提供 Host 权限/物理接管策略，保留其转发语义，生产代码没有 allow-all 默认。
  不调用 Lock/SetActive，不更改系统权限，不操作用户真实桌面。

新增 12 项私有总线用例以及 1 项 native 联测。覆盖初始拒绝矩阵、真实 peer 凭据取回、
不同 session 拒绝、启动脉冲、owner 替换、同名信号冒发、不同 path/interface、
malformed 消息、组合 owner 生命周期、原 Host callback 晚到、pending portal 自动关闭。

Native 联测将私有 login1/ScreenShield 的实际 D-Bus 消息接到生产 adapter，先取得真实 PNG，
再发 ScreenShield 激活信号。之后不发送模型操作/显式 Stop/新 capture：先由独立 C EIS peer
观察 disconnect，随后检查原 portal/输入线程/PipeWire 消费者已收尾、旧截图动作拒绝、
inactive 信号不能恢复授权。保留 `gnome-shield-revocation.json` 和原始 peer 日志。

## 上游证据与能力边界

保存官方 GNOME Shell/Mutter **49.0** 的六份源码、路径和 SHA256：
`upstream/sources.json`。这不是“最新版本”或已安装环境的版本探测。
Shell 的 ScreenSaver XML、shellDBus、独立 ScreenSaver service 相互印证服务名/方法/信号。
`screenShield.js` 明确解释 ActiveChanged 在锁屏动画后才发布，存在动画期间已锁但 D-Bus
仍报 inactive 的窗口；单独 ScreenShield 不能证明即时锁屏隔离。
login1 的 LockedHint 也不能被当作物理用户输入真值。

因此继续保持 `native_wayland=false`。本组件是新增的真实生产信号消费路径，不是完整
OS policy，也不是用服务协议 fixture 替换 installed GNOME。还需要验证 compositor 原生
锁屏/物理接管来源、真实 GNOME 会话中的 PID/session 绑定和 App 原生授权接线。
不新增 input 组权限，不读取用户 `/dev/input`，不靠合成 idle 活动假装物理接管。

## 本次冻结验证

353 个主源：相对上一候选 346 未变、5 修改、2 新增。

| 检查 | 当前候选证据 |
| --- | --- |
| login1 + GNOME 定向 | **22 passed**，含新增 GNOME **12** 项；另有 native 新增 **1** 项 |
| 同一冻结 Wayland binary | **137 × 3**，线程 1/4/4，0 failed / ignored / filtered |
| 原生证据 | **183** 个原始 fixture 目录，PNG/C EI/精确 peer/parent/policy disconnect 独立核对 |
| GNOME 信号到原生撤销 | 三轮各 **1**，独立 C peer disconnect，输入事件 0，旧 grant 拒绝复活 |
| 完整 Linux App / Wayland strict Clippy | all-targets、`-D warnings` 通过 |
| 隔离 runner contracts | **17** 通过；三轮原进程 cleanup 均无残留 |
| workspace fmt / git diff / quality final | 通过 |

源码 freeze、executable 测试前后 SHA、原始输出、runner cleanup、Node 图像/协议核对和
独立 Python 文件/结果核对由本目录 receipt 封存。原 Linux 会话 **48526** 已确认 exit 0；
定向会话 **87766 / 38219** 均 exit 0，没有因为观察超时另开重复测试。

本轮有一个辅助 diff 检查错误地覆盖仓库的 CRLF 规则，导致已有 WIP 的 CR 被报 trailing
whitespace；日志原样保留。恢复仓库既有配置后通过，没有为此重写其他文件。
本轮没有产品红测失败；不要借用上一轮的已修复竞态声称本轮复现了缺陷。

Windows/App selected/core/X11 的上一轮结果仅保留在历史收据中，本轮不虚报重新全跑；
本轮 Linux 全 App all-targets Clippy 证明编译接入，不证明已安装 App 的原生授权 UX。

## 完整目标仍未完成

Windows x64、macOS arm64/Intel、Linux X11/native GNOME Wayland；Desktop/managed browser/
existing Chrome+Edge/App WebView 经 App/ACP/MCP；完整输入/中文 IME/clipboard/取消恢复；
真实 OS 权限、锁屏、物理接管与恢复；签名 clean install/update/repair/rollback/uninstall；
窄窗口/DPI/权限 UX；真实 Grok E4；**同一最终冻结候选 12h active soak**。

App native consent/factory 尚未启用。Ubuntu 22.04 包内运行时尚未验证，当前 SDK 的
GLIBC_2.38 需求不代表发行基线可接受。早期 Windows publication、即时 PW 节点快照和
残留 PID 身份的未证根因不因本轮通过而消失。没有 commit/push/tag/publish/install；
没有更新目标为 complete/blocked/paused。
