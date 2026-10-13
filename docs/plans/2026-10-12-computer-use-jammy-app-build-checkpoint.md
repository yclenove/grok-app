# 完整 Jammy App 构建续行检查点

2026-10-12，`feat/computer-use-implementation`。整体 active / partial，未新增 commit/push/PR/发布。
Windows 当前唯一普通预览仍 app144/PID19768；本轮实看首页、A/B和上传页均正常，验收草稿未发送，零新授权/文件选择/提交。

## 实际失败与处理

1. 原 session72293 exit1：完整 App build-script 拒绝缺少 Linux seed manifest。原冻结源/失败日志保留在 app-01。
2. session69470 exit1：原生产 `cu-prepare-runtime --prepare` 请求官方 Node 下载失败，主 App 编译未开始。chroot curl另行证实 DNS超时；WSL提示NAT不能镜像localhost代理。Windows官方地址两现有回环出口的诊断也超时。本轮没有改系统/WSL DNS、代理或VPN。
3. 新 session54275 exit1：在相同 lock 下复核历史正式源包的长度/SHA并复制到 task cache；原生产 prepare/check均成功，实际 Node importProbe=passed。随后完整 App编译出现 Linux 专属 E0432/E0599：`webkit2gtk 2.0.2` 直接导出 `WebViewExt`，不存在 `prelude`。原五错误/unused-variable warning均保留在 app-03。
4. 产品修正只改 Linux trait 导入；下载 callback 所需的 instance 声明明确 cfg(windows)，不改变 Windows callback。没有删除导航/Wayland或跳过验证来通过构建。
5. session69871 已 exit1，不再 pending：冻结 product-app-02 的完整 release/custom-protocol 主程序 **构建成功**（10m18s），随后 release all-targets Clippy 因编译范围问题失败，导航测试未执行。主程序 SHA256 `09b064c32ee465e53853b0ca1abe5f328daa5141cab6e4bffaf5a13bb1fc7b42`；保留 `app-04/grok-app-source02.elf`。真实 ELF 的最高 GLIBC 需求2.34，DT_RPATH包含产品私有资源路径；NEEDED包含PipeWire、Wayland、GTK/WebKit，没有动态libei。这不是实际启动、安装或portal通过。
6. 已按真实调用范围修正：`revoke / bind_side_browser` 只供 debug harness；`ScriptOperation::child` 供 Windows及跨平台测试、`is_cancelled`只供Windows；task_picker六个全局验收辅助函数只供Windows测试。正常产品picker、跨平台隔离单测、权限和Stop路径保留；没有全局allow/dead-code豁免。

## 当前新候选

- 新冻结树 `/work/product-app-03`，以 product-app-02 为基础，仅上述四个文件的cfg/注释修正通过字节白名单进入；原失败树不修改。
- 运行时 seed 普通文件共享不可变数据，没有目录链接；仅 `--check`，不重新 prepare共享seed。前后 SHA复验为必须步骤。
- Cargo target复用原任务依赖缓存；完整主 App明确 `--features tauri/custom-protocol --locked --release`，私有PW/EI版本检查仍在。
- 原 exec session23735 已终态 exit1；证据 `tools/computer-use-probe/.run/linux-jammy-product-app-20261012-05/app-release-cfg.log`。03完整release/custom-protocol构建通过（10m40s），SHA256 `ef76f1d9996b6ed9e59e9bf1de65e6c8932931d1342dc3dbc32edf89bb1aa0e2`；release all-targets `-D warnings` Clippy通过（53.04s，无警告豁免）。release导航测试实际完成编译（11m29s），**9 passed / 0 failed / 0 ignored**。外层exit1来自生成锁owner的全树指纹，不是编译、strict或导航断言失败；原日志/exit不覆盖。该ELF保留为 `app-05/grok-app-source03.elf`。
- 对该03实际完整ELF，Jammy系统ld.so `--list` 在干净环境及显式系统LD_LIBRARY_PATH两种情况下均exit0，全部依赖可解析，PipeWire均来自生成的产品资源而非系统旧库。两条原日志 `app-source03-resolve-{clean,host-priority}.txt` 保留；这是动态解析验证，不是普通App/桌面/portal运行或dladdr原生调用验收。
- 原全树指纹包含了 `.cu-mutation.lock.owner.json`，原生产 `--check` 会更新这条生成的锁归属记录。02/03候选全清单复验因此失败，不能写成“全树完全未变”。新增 `check-jammy-app-inputs.sh` 只排除一个精确路径的这条记录，保留原清单与失败；02、03其余冻结内容及原seed分别实际复验通过。该sidecar不在产品资源映射中；不排除任何源码、seed组件或未知文件。原23735终态后才修未来 `run-jammy-app-release-cfg.sh` 使用独立checker，没有重启或重建相同App来刷绿。
- 本机重新 fmt、Node target/before-build8项、输入指纹6个正反例、git diff check和final代码质量门禁通过；App+Workbench13573/≥1000文件80未增长。输入oracles证明只有精确锁owner变化允许通过，源码改动/额外文件/缺失文件/原seed变更均拒绝；临时fixture原subprocess退出后精确删除。FINAL: PASS仅代码质量，不是交付证明。

## 同一03正式程序实际启动/正常关闭

新增独立 `run-jammy-app-startup.sh` / `jammy-app-startup.py` / `jammy-app-window.c`；工具输入置于单独probe目录，不写冻结source03。原 session81936 exit0，Jammy私有PID/mount namespace、Xvfb和DBus、全新独立App home，Computer Use关闭，未安装CLI、未登录、未发送模型任务、未关闭WebKit安全沙箱。

- 在 **系统LD_LIBRARY_PATH优先** 的挑战下，实际进程 `/proc/26/maps` 唯一PW路径为 `target/release/resources/computer-use/native-pipewire/runtime/lib/libpipewire-0.3.so.0`。正式启动先执行原 `dladdr + pw_get_library_version` 校验，不借独立探针当作主程序通过。
- 实际1200×800窗口截图 `app-05/app-source03-window.png` 已亲自查看：生产前端正常显示“Welcome to Grok / Install Grok Build”首次设置页，不是devUrl连接失败、空白或mock页。没有CLI的隔离环境准确停在安装步骤；没有绕过首次设置门槛。
- 只对该私有X server中的原PID/准确标题/可见尺寸唯一主窗口发一次正常 `WM_DELETE_WINDOW`。App exit0，224ms，日志无frontend quit timeout fallback；原App、WebKitNetworkProcess、WebKitWebProcess三件已观察owner的原pidfd全部signalled，私有X/DBus父链正常结束。没有kill作为验收成功条件。
- `app-05/app-source03-startup.json`、`app-startup.log`与chroot中的原maps/stderr/home保留，运行前后source03内容/原seed独立复验通过。

范围仅为Cargo布局的同一正式App启动/窗口/私有库身份/空闲正常退出，不是installed、正常聊天、CU任务、portal采集、Windows/macOS或12h通过。当前源码的后续runtime pin合同修改没有编入该03ELF，不能将不同候选证据拼成最终冻结包。

## CI与维护运行时准备

CI新增 `linux-production-jammy`（Ubuntu22/glibc2.35），依赖frontend上传的真实dist；读取 `tauri.linux.conf.json`，准备私有SDK及原browser seed后实际完整release/custom-protocol构建、release all-targets strict和导航回归。重复手写TAURI_CONFIG已删除，新增门禁也在frontend job运行。本地YAML解析及8项源合同通过；**远端CI/release未运行**，不能记远端绿。

官方metadata读取成功：Node当前所见LTS候选24.21.0、Playwright latest所见1.64.0。已下载四目标Node源包，分别核对官方SHASUMS256；Playwright tgz SHA256 `90c8b6b19da0a1b1e9a004cd6ff3e49e82668a0856e01efcc0e2d9c2b4466bb6`、3171930 bytes，匹配npm SHA512 SRI。官方1.64的browsers.json/registry绑定156.0.8078.4和新的Chrome-for-Testing布局；四目标browser下载原session54079/59979/34006/40379均exit0。存放 `.cache/computer-use/platform-source-20261012-maintenance`，尚未执行或替换当前seed/四lock。TLS/校验和匹配不冒称发布者签名或四架构运行通过。

只读源包清点输出 `app-05/maintained-chromium-source-inventory.json`：四包完整CRC读取及文件/树SHA/链接清点完成，体积在现有768MiB树配额内；两Mac框架仍各五个精确链接，但名称/Current目标已变化。Windows包没有文件形式CREDITS/NOTICE，Linux/Mac只发现Widevine LICENSE；**不能继续把新包冒称旧Chromium BSD-3/CREDITS布局，也不能据此推断它没有内嵌credits或不可分发**。晋升前必须核对CfT上游分发条款、内嵌第三方通知及所有新增组件许可，形成正确NOTICE/SBOM；不为让旧许可门禁通过而选未经维护审计的旧browser。

更新不能仅改lock：[runtime编译合同检查点](2026-10-12-computer-use-runtime-pins-checkpoint.md)已经集中当前版本pin、消除生产tgz路径/manifest版本硬编码，修复browser fallback只找Windows归档的问题，并拒绝四目标不匹配的归档/可执行文件/树布局；当前版本仍1.48/130。须按真实新布局/来源更新完整准备、修复、copy/manifest/导入及包门禁；保留链接/path traversal/解压限额拒绝，核对官方系统底线，完成四目标与实际主机回归后再晋升。当前完整App依旧使用旧seed，只是既有候选的兼容性验证。

离线源包逐件匹配原锁：Node46879641、Playwright1957232、Chromium173675209 bytes；对应 SHA及冻结源在 app-03/app-04证据中。没有换源、改锁或关闭哈希门禁。

## 交付前仍必须完成

当前锁 Node20.18.0 / Playwright1.48 / Chromium130 已旧；本批仅验证既有候选的兼容性，**不是维护/安全版本审计通过，不可直接据此发布**。须更新四目标锁、实际模块/API/语义/截图/生命周期与安装回归。

Cargo布局的完整App实际库身份、嵌入前端与空闲正常退出已验；仍须正常聊天/所有CU lifecycle、安装布局和AppImage/deb/rpm资源/NOTICE/依赖、安装升级修复回滚和portal。
远端CI/release未运行；`linux_webkit.rs`未在产品入口注册，外部wrapper/helper/挂载生命周期未验证，不能宣称已生效。
Windows真人ACP/MCP/Grok/picker，三平台/两Mac架构/四surface、UI UX、真实模型矩阵、同最终候选12h、D00–D31完整范围全部保留。

## 清理

上批原owner退出后已清loader-01/02/03复制品：738文件/逻辑192469688 bytes，WSL可用观察增197791744 bytes；Windows物理VHDX回收未知。12个故意链接/FIFO case暂缓。
本轮原构建/启动owners已正常终态；SDK、工具链、当前App/依赖缓存、source/seed、维护版源包和原失败证据为后续必需输入，保留；Windows当前用户预览/home也保留。
输入oracles临时fixture原subprocess全部退出后精确清理，最新根 `/var/tmp/grok-cu-jammy-sdk-input-oracle-yefvtse2`、逻辑540 bytes；实际回收未测。重建命令 `python3 tools/computer-use-probe/test-jammy-app-inputs.py`。正常退出与当前root owner/exe核验后，精确删除已完成的 `/var/tmp/grok-cu-jammy-sdk-20261012-01/work/app-startup-source03-01/app-window` 单一compiled helper：逻辑17040 bytes、WSL空闲观察增20480 bytes，Windows物理VHDX回收未知；准确SHA/重建命令/保留项见 `app-05/app-helper-cleanup.json`。没有递归删除workspace/target或其它worktree，当前原App与全部截图/log/maps/home、SDK/依赖/维护源包保留。
