# Computer Use：真实 GNOME 46 错误恢复与 Host 管理回读

目标仍为 **active / partial — not releasable**，不是最终版。本轮以实际工作区和
原进程结果复核前一轮为 **progress**，没有用旧报告当作新验收。
`20261001` 是本地运行目录标签，不代表本文件的完成日期或发布版本。

## 实际发现并修复的问题

在隔离环境运行真实 Ubuntu GNOME Shell **46.0 / Mutter 46.0** 后，发现
`DisableExtension` 可以成功将 `enabled` 设为 false，但扩展的 `ERROR=3`
仍保留。旧 Host 只接受状态 2/6/99，导致启用失败后停用始终显示未确认，修复也被卡住。
`error-recovery-before.log` 同时保留了生产客户端失败和原始 D-Bus 的
`state=3, enabled=false`；不是根据 mock 推测出来的问题。

- `gnome_control.rs` 严格读取真实 boolean `enabled`，不缺省为 false。
- 只有关闭设置、允许的非活动状态以及 helper endpoint 确认缺席同时满足，才确认停用。
  活动/转换中、设置仍开启、Blocked/Unknown 健康状态都不被当作停用。
- 停用后的 ERROR 可以显式 Repair，但 ERROR/OUT_OF_DATE/UNINSTALLED 仍要求新
  Shell 进程。不能直接 Enable，不能用一次设置回复、文件修复或恢复健康复活输入权限。
- Host 状态和实际操作使用一致的资格检查。现有 file/operation/feature locks、
  原 native owners 的清理、默认关闭及 separate portal consent 不变。
- 增加真实固定 UUID 管理探针，以及调用原 Host manager 的三个 opt-in 手工集成测试。
  常规测试中的 ignore 有意保留；本轮另外逐个显式执行，不能把 ignore 算作通过。

## 真实运行证明的边界

运行器隔离 PID、mount、network、`/tmp`、`/run` 和 D-Bus，使用 owned headless
Wayland compositor；没有连接用户登录桌面的 Shell/system bus，没有向用户安装扩展。
管理测试在 root 的私有 namespace 中运行，以保持宿主 `/` 的 UID 0 对生产路径检查
有意义；它不是非 root 登录用户验收。负例客户端在额外的 user namespace 中运行。
所有本轮原 Shell 子进程在退出后回收，不能用新建进程替代旧进程收尾。

实际 App Host 测试依次证明：

1. 原 Shell 中缺失 → 显式 Install → 磁盘 Current，但要求重启，旧进程不能 Enable。
2. 原 Shell 退出且 join，新进程发现文件；旧 pinned owner 客户端明确拒绝继续操作。
3. 缺少 ScreenShield 的启用失败 → 真实 ERROR → 显式 Disable 成功 → 允许 Repair，
   不允许 Enable；故意破坏 owned fixture 的 `policy.js` 后，原 Host 修复并回读 Current。
4. 再次退出/join/新进程启动，修复后 bundle 被重新发现，要求的重启标记已满足，
   **仍未启用、不拥有桌面输入权限**。

隔离总线没有 GDM。实际 binary gresource 中 `loginManager.canLock()` 依赖 GDM
Version，缺失时 `Main.screenShield` 不创建；helper 正确 fail closed。
**没有伪造 GDM/login1，没有放宽 ScreenShield 检查，没有证明成功启用。**
这不是 installed Ubuntu 24.04、原生 App WebView IPC/界面、系统授权或物理接管验收。

SDK 是解包的 Ubuntu noble GNOME46/GJS/Mutter46 加 Debian13/其他 staged 依赖；
包来自官方 HTTPS archive，逐包核对 Packages 索引 SHA256，但本轮未独立验证索引
签名，也未证明 Ubuntu22.04 发布 ABI、已安装发行版兼容性或签名包完整性。

## 最终本轮快照的检查

证据根：`tools/computer-use-probe/.run/gnome-shell-live-20261001/`，最终复跑在 `final/`。
构建前固定 **413 条相关源记录**，相比前一快照为 6 条修改、2 条新增；不是完整仓库、
全部依赖或最终发布候选冻结。复跑后逐字节复核，保留所选源码压缩包和独立 ELF 副本。
ELF 和大体积原生产物位于 WSL `/var/tmp/grok-cu-shell46-live-20261001/`，
由收据逐文件记录散列；避免挤占几乎满的 H:，没有清除其他工作。

| 检查 | 本轮最终结果及范围 |
|---|---|
| 真实 GNOME 生产 control 负例 | 开启失败、ERROR 停用、新进程前禁止再开启；通过 |
| 真实 GNOME + 原 App Host manager | 三项显式手工测试各 **1 pass / 0 ignored**；三次 Shell incarnation，旧 owner 拒绝 |
| Linux preview helper 定向单测 | **16 pass / 3 manual ignored**；三个手工项另行执行如上 |
| Linux preview 直接依赖 | commands **15**、portal **6**、feature lifecycle **1**；全部通过 |
| Linux default 独立 executable | helper **17 pass / 0 ignored**，不进行真实 helper mutation |
| 完整 Wayland crate | **171 pass / 0 fail / 0 ignored / 0 filtered**，线程 4 |
| 原生产物独立回读 | **65** 精确 owned fixture 目录，**34** EI peer 最后事件 disconnect，**18** PNG 实际解码；原子进程全部 join |
| 静态检查 | Linux preview App/Wayland、default App all-target strict Clippy；定向 rustfmt、diff、quality final 通过 |
| 散列 | 413 源复核；preview/default App、control probe、Wayland ELF 的前后散列核对通过 |

Wayland suite 中的私有 labwc/PW/EIS/GTK 证明归属、输入事件、parent/unmap、策略撤销和
PNG，不证明真实 GNOME 物理事件或最终应用效果。它与真实 Shell 的管理测试是两类
不同证据。App 数字是定向 filter，日志保留其 filtered 数量，不声称完整 App suite。
Windows、TS、15 locale 和浏览器 UI 本轮没有重跑；上一收据中的结果只能作为历史证据。

## 诊断与保留

早期缺少 staged GI/共享库/EGL 资产、探针首轮 Rust 编译错误、未映射 `/` 的 UID
导致 Host 路径检查拒绝，以及缺少 GDM 的正向启动失败，均保留诊断日志。
没有为了测试通过放宽路径权限、启用允许全部的策略或把负例写成正向成功。
部分早期 PowerShell 包装的 exit code 没有传播内部失败，故以原日志而非外层零退出
判断那些诊断；最终复跑显式传播退出状态，并另行解析业务记录/真实测试结果。

新 `receipt.json` 链接前一收据 SHA256
`160fef04e434b138eea58e7bd2d81394242c2d1d1082a45137c6c4769fde1378`。
原收据、原始失败和历史产物不修改，不把收据自身当成完成证明。

## 完整目标仍未完成

1. installed Ubuntu24.04 GNOME/GDM/native Wayland 下 helper 成功启用、实际用户
   安装/修复/重新登录、App 原生授权选择、物理输入分类、锁屏边界、接管/恢复、
   session/PID/focus/topology 和 native consent/UI 全链路。
2. Windows x64、macOS arm64/Intel、Linux X11/native Wayland；Desktop、managed
   browser、existing Chrome/Edge、App WebView，经 App/ACP/MCP 的完整功能矩阵。
3. 所有输入、中文 IME、剪贴板、取消/异常恢复、OS 权限、窄窗/DPI/确认 UX。
4. 签名 clean install/update/repair/rollback/uninstall、实际 AppImage/deb/rpm、
   Linux 依赖/许可、Ubuntu22.04 baseline；不是 staging 目录中的 ELF 检查。
5. real Grok E4、**同一最终冻结候选 12h active soak**，随后原需求逐项完成审计。

下一步仍优先推进实际登录 GNOME/App 的正向授权与安全撤销闭环；不以更多 mock、
私有 fixture、审计文案或较小平台集合替代上述要求。没有 commit/push/tag/签名/发布，
没有改用户安装、默认开关或 `native_wayland=false`；没有调用 goal 完成/阻塞/暂停。
