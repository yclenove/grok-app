# Computer Use：Linux login1 会话监控组件

日期：2026-09-30。目标：**active / partial — not releasable**。
原所有功能/最终版范围不变。证据目录：`tools/computer-use-probe/.run/wayland-logind-20261001/`。
目录沿用运行标识；不是已安装 GNOME 验收，也不是把目标改成仅 D-Bus 测试。

## 接续与实际产品进展

先完成前一权限撤销候选的 Windows/Linux/原生回归、Node 和独立 Python 双重校验，
各复核 2,280 项文件记录，收据 SHA-256：
`f713dc4b8313a3b45bb9dfecaf74c91c4e36546dc0a01f466eea7715b68cf9df`。
该收据是前一候选的历史证据，不拿它的旧源码 freeze 证明本候选。

本次新增生产 `LogindSessionWatch`，不再仅依赖 fixture bool 作为会话锁屏来源：

- 通过 system bus 的 `GetSessionByPID` 定位本进程的具体 login1 session，核对 euid、local/user/Wayland、
  Active、LockedHint、State 以及 PreparingForSleep/Shutdown。缺失/失败/非本会话均拒绝，
  不取环境里的 session ID，不退回其他 seat/user，也不监视 caller-relative self/auto 对象。
- 锁定 login1 unique owner，先订阅再读快照、再核对同一 owner。初始化期间的 Lock→Unlock
  短脉冲不能被后续 unlocked 快照覆盖；不信任其他 D-Bus peer 冒发的同名路径/信号。
- Lock、Active/LockedHint/State 失效、关键属性 invalidated、SessionRemoved、休眠/关机、
  daemon 丢失/替换、总线断开均终止原 watch。Unlock/正向更新不复活旧授权。
- policy 默认拒绝，必须真正 poll 原 `run_until` future 才能监控；调用方必须保留并 await 原 owner。
  组件不 detach 内部任务。Drop/Stop/cancel 关闭原 guard，新恢复必须新 watch 加显式用户同意。
- 仍强制组合调用方的 Host policy，不生成 allow-all 默认策略。物理用户输入/portal permission
  不是 login1 提供的能力，不能用此组件替代；App factory 仍未启用 native Wayland。
- 与现有 PortalRegistry 联测：真正私有 D-Bus 的 LockedHint 信号自动取消并关闭原 pending portal，
  不需要后续模型输入/外部 Stop；原 session 清理后 permission reset 不能复活旧 policy。

对照并保存官方 systemd v257 的 login1 XML、URL 和 SHA256（upstream-source.json）。
**LockedHint 是会话报告的 hint，不等于所有 compositor 上完整、即时的锁屏真值**；实际 GNOME
锁屏/权限/物理接管接线与安装态验收仍缺。生产 system-bus 构造入口已实现、编译，自动化只用
本进程拥有的私有 D-Bus，不对用户真实桌面/session 做锁屏或输入测试。

## 新发现的竞态与保留的失败

初版 Guard 在调用 Host policy 前检查 owner，但没有在返回后重验；原 OS monitor 已 join 后，
一个被暂停的 Host callback 仍可晚到返回 true。先新增确定性测试，实际得到
**0 passed / 1 failed / exit 101**，保留失败 executable、原产品/测试源码与日志。
随后增加原子状态后验，原 owner 关闭后晚到许可不再发布。没有通过改断言掩盖缺陷。

最初七项测试另有 **1 pass / 6 fail**，原因是测试 D-Bus 的自动方法名生成 Pid 而非 PID；
测试显式指定 GetSessionByPID/Type，并让初始拒绝矩阵断言确切原因，防止 UnknownMethod
错误被误当作正确的安全拒绝。原失败日志保留，不把这六项当成产品已复现缺陷。

## 本候选验证

351 主源冻结：相对前一候选 345 不变、3 变更、3 新增。无 UI 文案/shell 状态增长。
新增十项测试覆盖：精确 PID/owner；11 个初始负例；初始化 lock/unlock 脉冲；9 类安全信号；
其他 session 隔离；owner/bus 丢失；drop/pre-cancel；晚到 Host callback；Registry 自动撤销；peer 冒发。

| 检查 | 本次结果 | 证明边界 |
| --- | --- | --- |
| login1 定向用例 | **10** 通过 | 私有实际 D-Bus 消息/订阅，不是 fake 方法直调，也不是 installed GNOME |
| 同一 Wayland binary | **124 × 3**，0 failed / ignored / filtered | 同一冻结 executable，线程 1/4/4，私有 portal/PW/EIS/labwc |
| 原生证据目录 | **177** | 全部原始 PNG/C EI/peer 身份/policy disconnect/parent 结果核对，owned residual 0 |
| GTK parent | 三轮各 **12 Wayland + 4 X11 负例** | 每轮 9 exports / 9 destroys，原 compositor/Xvfb 均 join |
| Windows/Linux core | **590 / 589**，0 failed / ignored | 平台条件数不同 |
| Windows/Linux driver / FFI | 各 **16 / 21** | FFI double 不是 macOS 实机 |
| App selected | **111**：13 command + 69 WebView + 21 session-MCP + 1 lifecycle + 7 supervisor | 真 App lib test artifact，独立 manifest 副本；不是全 1746 项全跑 |
| X11 unit / runner contracts | **49 / 17** | 不继承旧 native/AT-SPI 测试作为新跑 |
| 全 Linux App + 四原生 crate strict Clippy | 通过 | all-targets，-D warnings |
| Windows/Linux App check、fmt、diff、quality gate | 通过 | 不是安装/UI/E4/完整目标验收 |

测试前后 native executable 哈希相同；App 原 EXE 不被 manifest 修改。
Node 原生协议/图像复核和独立 Python 文件/结果复核以本目录 receipt.json 为准。
Windows session 37837 和 Linux session 43106 均从原句柄确认 exit 0，无观察超时重启。

## 仍须继续的完整范围

App native consent/factory、GNOME compositor/portal permission/物理接管/恢复与 installed native GNOME；
Windows x64、macOS arm64/Intel、Linux X11/native Wayland 的完整功能矩阵；Desktop/managed browser/
existing Chrome+Edge/App WebView 经 App/ACP/MCP；完整输入/中文 IME/clipboard/取消恢复；
签名安装/更新/修复/回滚/卸载；窄窗口/DPI/权限 UX；真实 Grok E4；
**同一最终冻结候选 12h active soak**。

Ubuntu 22.04 发行基线和包内运行时仍未证明：当前私有 SDK 的 GLIBC_2.38 需求不能冒充兼容。
更早 Windows publication/即时 PW 节点快照/残留 PID 身份问题的未证根因状态不变。
没有 commit/push/tag/publish/install，native_wayland=false；不调用 complete/blocked/paused。
