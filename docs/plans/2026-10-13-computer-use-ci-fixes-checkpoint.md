# Computer Use：远端 CI 修复与同步

当前继续处理第四批：macOS只读资源修复已通过下述本机回归，尚待新远端验证。
`040c92d8` 的CI已暴露Windows八项原生启动时序失败与Linux私有namespace
读取runner工作区失败；二者仍开放，不能将之前局部通过当作整体CI成功。

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

## 第四批：macOS 只读资源与再次同步

再次成功fetch `origin` / `upstream`，`HEAD..origin/main` 与
`HEAD..upstream/main` 均为0；main分别仍为 `b58b97cd` / `9c5eb3b5`。
没有新main提交需要合入，不改变用户源码、包或隔离home。

`040c92d8` 的 [run 38385859129](https://github.com/yclenove/grok-app/actions/runs/38385859129)
中frontend、两个SDK与维护ARM64 browser诊断成功；Rust Mac/Windows/Linux/
ARM64失败，production Jammy的release检查仍在运行，未取消或重跑该run。
Linux App库 **2327 passed / 0 failed / 3 ignored**，但后续私有Wayland
入口在读取runner工作区脚本时permission denied，不能将整个Linux job记绿。
Windows App库 **2391 passed / 8 failed / 87 ignored**，八项仍待定位修复。

Mac失败发生在Tauri资源复制：两份固定Chromium1140的
`gpu_shader_cache.bin`（各968826 bytes）在原ZIP中均为 `0100444`。
原解包器逐位保留权限，Tauri的 `fs::copy` 又把只读权限保留到构建目录，
后续复制同一路径便不能覆盖。新解包器只给普通文件补owner读写，保持
可执行位，不增加group/other访问，并继续剥离setuid/setgid/sticky。
资源内容/hash与归档门禁保持；Rust CI缓存版本更新至v6，避免恢复旧只读
编译副本。已有本地旧编译副本不由只读 `--check` 变更：重新prepare后，
需在确认归属的旧Cargo资源副本上补owner写权限，或使用新的任务构建目录；
不要清整个target或共享cache来处理它。

本批真实验证：

- 原代码下新增Unix权限回归 **0 passed / 1 failed**（0444未补owner写权限）。
- 修复后Jammy ZIP全部 **13 passed / 0 failed / 0 ignored**；额外关闭
  `CAP_DAC_OVERRIDE` / `CAP_DAC_READ_SEARCH` 的原生进程中，新回归
  **1 passed**，实际连续两次 `fs::copy` 成功。私有chroot根目录0700，
  没有为切换UID放宽目录；两次runuser夹具环境错误原日志保留。
- Windows Core完整库 **1000 passed / 0 failed / 0 ignored**；两平台Core
  all-targets strict clippy、workspace fmt、依赖卫生与final质量门禁通过。
  App+Workbench13552行、产品≥1000行文件80，均未增长。
- workflow YAML与四Rust profile实例化bash语法检查通过。

证据与再生命令：`tools/computer-use-probe/.run/ci-resource-permissions-20261013-01/`。
本批不是原生Mac App/安装包或完整CU交付验收。Mac framework五条精确links
必须继续核对真实Tauri bundle输出；source seed中links有效不证明打包后有效。
下一批优先Windows原启动gate与Linux隔离runner路径，然后运行时CPU校验、
维护来源/五target产品集成及原R1–R4所有安装/模型/surface/UI UX/12h门禁。

原测试/编译owners退出后，仅删除新增首败测试的准确UUID临时目录，先核验
父级/子项没有链接、只有原22-byte payload、hash一致且没有live owner。
逻辑删除 **22 bytes**，WSL可用空间观测增加 **12288 bytes**；实际物理回收
未知，不代表VHDX缩小。当前编译产物/依赖、原包/日志/失败证据及用户包/home
保留；前批symlink/FIFO故障目录不在本批清理范围内。

## 第五批：sudo 原用户与私有目录权限

再次fetch `origin` / `upstream`，两边main均已在当前分支中，新增提交数均为0。
main仍分别为 `b58b97cd` / `9c5eb3b5`；没有为了同步重写或覆盖用户工作区。

原Linux CI不是Wayland断言失败：sudo将host root映射为namespace 1，原runner
用户没有映射。原用户的0700工作目录及SDK目录因此无法遍历，Python入口报
permission denied。私有namespace的capabilities不能代替未映射owner的访问权。

launcher现在仅在sudo提升路径中，用 `setpriv` 恢复原 `SUDO_UID` / `SUDO_GID`、
清空附加组与原继承/ambient capabilities，只将SYS_ADMIN传给紧接着的unshare，
以保留受限制runner的namespace创建能力。unshare在执行任何fixture代码前创建
新的user/mount/net/PID namespace，并只把原caller映射为1:1；namespace UID0
仍未映射。直接普通用户/直接root入口保持原单ID映射，缺失或非法sudo身份
在namespace创建前拒绝。没有chmod真实runner/home、改AppArmor/sysctl、开放
用户会话网络，或减少完整套件要求。Jammy unshare 2.37.2不支持多ID映射参数，
本实现不依赖新版 `--map-users` / `--map-groups` 或newuidmap/newgidmap。

实际验证：

- 原launcher在原用户1000:1000拥有的0700目录中失败；修复后能读取同类目录，
  不改变目录模式。使用真实Jammy util-linux 2.37.2的unshare/setpriv。
- 实际PID1、UID/GID1、唯一 `1 -> 1000` uid/gid map、只有lo的网络与私有tmp
  均已断言，原脚本/SDK文件可读。新runner合同合计 **27 passed**，无跳过。
- 同一原Jammy/glibc2.35 Wayland binary/compositor/SDK在私有overlay中两次运行
  全量套件，最终清空ambient继承后的候选 **199 passed / 0 failed / 0 ignored /
  0 filtered out**，GTK原UI-owner集成通过。原children均joined，live/errors为空；
  二进制与真实SDK/cache字节及权限未改，仅私有overlay复制上层目录metadata。
- Bash语法、四Rust profile实例化workflow语法、workspace fmt、依赖卫生与final
  质量门禁通过；App+Workbench13552行、产品≥1000行文件80，均未增长。

证据/再生命令：`tools/computer-use-probe/.run/ci-namespace-owner-20261013-01/`。
保留原拒绝日志及验证夹具的PID/sysfs/旧cache祖先权限错误；它们不是产品通过。
本批不等于安装态GNOME portal、真人输入或完整CU交付。

最新 `9eb01b84` 的 [run 38389828107](https://github.com/yclenove/grok-app/actions/runs/38389828107)
已通过frontend、两个SDK与维护ARM64 browser诊断；ARM64产品Rust job仍失败，
Windows/Mac/Linux Rust与production Jammy在本批最后观察时仍运行，未标绿。
上一run的production job已由workflow转为cancelled，本批没有主动取消/重跑。
继续Windows八项原启动故障（包括worker端口前退出）、ARM64产品运行时、CPU
校验、完整NOTICE/维护安全选择、五target与原R1–R4全部门禁。

原验证owners退出并确认native cleanup后，检查准确路径、mount与每个进程的
cwd/exe/open-fd，四个退休setup目录均无links/特殊文件，才删除它们。原失败
setup源码及hash清单先留存于证据目录；逻辑删除 **195970 bytes**，WSL文件
系统可用空间观测增加 **372736 bytes**。精确物理回收未知，未缩小VHDX。
05/06 native upper树包含fixture链接/残留socket，暂缓递归清理；当前SDK/cache、
源码/依赖、两次199项证据、用户包/home与前批故障证据保留。
