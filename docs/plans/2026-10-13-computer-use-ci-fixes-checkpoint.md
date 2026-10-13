# Computer Use：远端 CI 修复与同步

2026-10-13，`feat/computer-use-implementation`。

再次 fetch fork 与源仓库后，`origin/main` / `upstream/main` 均为当前 HEAD
祖先，没有新的 main 提交需要合入。前批同步仍有效，不覆盖用户运行中的
App144、隔离 home、登录、历史或未发送草稿。本批仅提交到 fork，不创建 PR、
tag 或 Release。

## 远端失败及修复

原 [CI run 38372012718](https://github.com/yclenove/grok-app/actions/runs/38372012718)
已结束，整体 failure，frontend 和两种架构的原生 SDK job 成功。

- Rust Windows/macOS/Linux：Rust1.99 新 lint 拒绝 `bounded_store_gates`
  六处 `map_err(&fail)`。改为 `map_err(fail)`，closure 捕获引用且可 Copy；
  失败清理、有界存储和隐私检查不变，不降低工具链或豁免 warning。
- production Jammy：`tempfile.mkdtemp` 的随机后缀可含下划线，违反 checker
  明确的 owner 目录命名合同，`unchanged-inputs` 在断言前 exit2。夹具改用
  UUID hex 后缀并以 mode0700 原子创建，不放宽产品路径校验。
- Rust Linux-arm64：受管浏览器 target/资源缺失，仍未修复，不跳过运行时
  prepare 或用 x64/EOL 浏览器伪装成功。

## 本批实际验证

- Windows 现有隐私门禁两项：2 passed / 0 failed / 0 ignored。
- Windows Core all-targets strict clippy 和 workspace fmt：exit0；本地为
  Rust1.98，Rust1.99 的全平台结论须由新 CI run 确认。
- Debian WSL 的现有输入夹具：6场景通过，包含正常输入、唯一生成 owner
  变动、源码改变、新增文件、原 seed 改变、源文件缺失；每个 bash owner
  已 joined，精确生成夹具已自行删除，逻辑612 bytes，物理回收量未知。
- 双架构原生 SDK 的实际远端证据见
  [SDK检查点](2026-10-13-computer-use-linux-sdk-targets-checkpoint.md)，不是本批重跑。

仍继续维护运行时、ARM64 browser、完整产品/安装/模型/四surface/UI UX及
同最终候选12h门禁；本批绿灯不代表 CU 已交付。

## 后续实际 CI 与第二批修复

`fb4309bf` 已推到 fork，新
[run 38374373979](https://github.com/yclenove/grok-app/actions/runs/38374373979)
中的 frontend 与双架构 SDK 再次成功；Jammy 的原输入夹具已成功，正式
runtime prepare/check 也已成功，完整 release App 检查仍运行中。
没有为观察超时取消它，或将 live job 记为成功。

这轮不再报原六处 closure borrow；继续暴露以下真实阻塞，已修复源码：

- Rust1.99 弃用 `Atomic::fetch_update`。八处改为相同语义的 `try_update`，
  checked overflow、比较/存储 memory ordering 和返回值不变；包括 X11/
  WebView epoch 与测试的故障计数。本机 Rust1.98 也实编通过该 API，未加
  warning 豁免或抬最低工具链。
- macOS 的 `libc::S_ISVTX` 是 u16，`MetadataExt::mode` 为 u32，原代码不能
  编译。使用固定 POSIX sticky bit `0o1000`，保留目录 owner/写权限/链接门禁。
- 普通 Linux Rust job 缺私有资源 manifest；改为在 App 编译前构建并审计
  维护 SDK、按 rustc host target stage 资源。x64 runner 固定 Jammy，与
  原 ABI2.35 合同一致；ARM64 同样 stage，不跳过仍缺失的浏览器门禁。

本批最终 Windows Rust1.99 App all-targets strict clippy、workspace fmt均
exit0；真实 manifested App harness **2399 passed / 0 failed / 87 ignored**。
87项 ignored 未算原生验收。Core 现有 surface/lifecycle16项及隐私2项全部
通过。Windows全量执行后新增的 sticky-bit修正仅在 Unix cfg内，最终 Windows
strict已再次通过；Mac/X11原生结果仍待新 CI。

来源/诊断工具62项、Node target/command15项、定向ESLint、源格式与完整
quality门禁通过。两份 YAML校验脚本实例化 shell后通过 bash语法检查，
确认双架构 SDK准备和 ARM64诊断只上传JSON/log/TAP。ARM64来源及后续真实
执行见[维护浏览器检查点](2026-10-13-computer-use-arm64-browser-checkpoint.md)。

证据：`tools/computer-use-probe/.run/ci-followup-20261013-01/`；保留两轮
远端错误日志及最初 Windows PowerShell5 在 native stderr重定向时提前中断
的本机编译输出，后续 pwsh7按真实Cargo artifact完成，未删除首败。

原测试 owner已退出；验证绝对路径/所有父级无reparse、测试copy/source SHA
和原日志后，只删除 `ci-followup-20261013-01/app-tests.exe`。
逻辑删除 **74,039,808 bytes**，H盘可用空间即时观测增加 **74,043,392 bytes**；
无法隔离并发磁盘活动，因此不将观测差值当作精确物理回收量。
原Cargo测试binary、源码、编译依赖、日志/manifest/hash与当前ARM64诊断均保留；
再生命令与暂缓项目记录在 `cleanup-receipt.json`，用户App144没有退出或替换。

## 第三批：macOS FFI、Jammy 原生截图与 Windows 测试依赖

提交前再次 fetch 两个 remote，`HEAD..origin/main` 与 `HEAD..upstream/main`
都为0。fork main为 `b58b97cd`，源main为 `9c5eb3b5`，均已合入当前分支。

原 [run 38378165553](https://github.com/yclenove/grok-app/actions/runs/38378165553)
已结束，整体failure。production Jammy的完整release App、strict cfg和
导航回归成功；frontend、双架构SDK、ARM64维护浏览器诊断也成功。
Rust macOS、Windows、Linux x64/ARM64仍失败；本批修复前三类，ARM64
产品运行时target仍开放，独立job成功不等于整个workflow成功。

### 修复内容

- macOS：`CFStringGetCString`返回CoreFoundation Boolean（u8），与
  `system_fonts`声明一致；调用处明确判断0。原selftest移入仅probe/
  FFI合同编译的模块，消除普通App dead-code warning。
- Jammy：无labwc apt包，使用固定commit/SHA的私有0.5.3测试compositor，
  针对wlroots0.15回移上游xdg-foreign v1/v2初始化。GPL工具仅用于测试，
  不进入产品资源。显式 `-c rc.xml`，私有namespace映射到有passwd条目的
  UID/GID1，保留隔离，不改系统sysctl/AppArmor或接入用户桌面。
- 截图：PipeWire1.6.9视频adapter必须加载videoconvert。原SDK关闭该
  插件，真实连接返回 `-22`，27项截图失败。启用插件并同步更新构建、
  17文件审计、搬移、产品manifest验证；loader也覆盖插件缺失/损坏。
- Windows：源码worker和preload从自身目录解析Playwright；CI只有seed
  依赖，本地额外工具依赖掩盖问题。CI按工具目录现有锁安装，使用
  `--ignore-workspace --frozen-lockfile --ignore-scripts`，避免pnpm退回根
  workspace。没有给产品加模块查找fallback或删除失败测试。

### 实际验证与边界

- 九组Mac FFI合同：**180 passed / 0 failed / 0 ignored**；Windows doubles，
  不是原生Mac权限/输入验收。Windows Core strict clippy及workspace fmt通过。
- 隔离Ubuntu22.04/glibc2.35：最终builder/audit/stage/compositor/runner合同
  **69 passed**；固定compositor真实构建、patch零fuzz成功。
- 同一私有compositor + 新SDK的完整Wayland suite：
  **199 passed / 0 failed / 0 ignored / 0 filtered out**。原27项失败恢复，
  GTK原UI owner合同通过；cleanup证明原children joined、live/errors为空。
- SDK原路径与搬移路径实际加载video plugin，9个实际loader故障按预期
  拒绝。实际产品loader模块10项合同/strict clippy通过；20个安装路径、
  搬移和故障场景全部通过，包括video缺失/损坏。不是完整App/portal验收。
- 干净源码worker `/open`：缺依赖500，锁安装后200，原context取消且worker
  退出。首次未加ignore-workspace的失败保留。最终manifested App定向回归
  **13 passed / 0 failed / 1 ignored**，含原Windows CI失败12项；人工十分钟
  deadline仍未执行。本批没有重新宣称全量App2399通过。
- 四Rust matrix的YAML/实例化shell、workspace fmt、依赖卫生、完整质量门禁
  通过；App+Workbench13552行、产品文件≥1000行计数80，未增长。

证据：`tools/computer-use-probe/.run/ci-jammy-compositor-20261013-01/`。
保留原CI错误、172/27原生首败、缺依赖/错误workspace结果，以及本机chroot
不完整dev/proc与compiler probe缓存的失败；环境修正后的真实通过才计验收。

### 清理与后续

原test/browser owners退出后删除Windows测试EXE副本、三份合成profile/
runtime副本，逻辑 **1,166,938,204 bytes**；H盘可用空间观测增加
**1,123,483,648 bytes**。WSL退休旧compositor编译输出及重复故障包60个
目标，逻辑 **237,897,030 bytes**；虚拟文件系统可用空间观测增加
**244,252,672 bytes**。观测不等于精确物理回收量，WSL不代表VHDX已缩小。
四个含故意symlink/FIFO的故障场景暂缓，未递归删除链接目标。

首次WSL参数转义错误产生的根目录副本已逐文件审计，与原HEAD/task snapshot
及已知后续修改对齐。精确将 `/scripts`、`/src-tauri`、`/native-inner.sh`、
`/native.log` 移到task内 `recovered-initial-copy-01`；所有hash保持，删除0，
未覆盖用户工作区。移动/清理receipt和再生命令保留；当前compositor03、
SDK169-03、positive runtime、源码/依赖/seed/日志与用户App144/home均保留。

继续完整维护运行时选型/许可及五target产品集成，特别是Linux ARM64；随后
原生Mac、各安装格式/升级回滚、实际模型/ACP/MCP、四surface/UI UX和同最终
候选12h门禁。本批只提交/push到fork，不创建PR、tag或Release，整体active/partial。
