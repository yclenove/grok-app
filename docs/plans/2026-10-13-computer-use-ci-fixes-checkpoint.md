# Computer Use：远端 CI 修复与同步

当前为第八批：Ubuntu22旧ATK的GetActions崩溃已在独立探针与原产品夹具复现，
产品改读有界NActions/GetName；Jammy X11测试52项、原生输入19/可访问性41/
剪贴板66项通过。`9309a6bb`远端Windows仍为plugin auth超时（2406通过/1失败/
87 ignored），ARM64产品target仍失败；X11新修复待远端复验。第七批Windows
完整App本机2407项等证据保留；完整维护运行时与R1–R4仍未交付。

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

## 第六批：Windows 夹具临时目录一致性

第五批已提交/push `22e6dfb7`，没有创建PR/tag/Release。再次fetch两边main
无新增，当前分支已包含它们。Windows继续使用上一批准确Cargo产物的相同
SHA256/manifested owned副本，没有更换Rust测试内容来取得不同结果。

本机控制实验把 `TMP` 与 `TEMP` 指向两个独立目录：Rust临时路径按Windows
GetTempPath优先使用TMP，Node `os.tmpdir()`优先使用TEMP。原fixture要求Rust
创建的UUID目录直接位于Node临时目录，入口因此拒绝，同样的八项失败完整
复现：**2 passed / 8 failed / 1 ignored**。没有修改本机全局TEMP/TMP或用户home。
这证明了一个能造成原CI八项症状的具体机制；尚未捕获远端runner的实际变量，
不能宣称远端故障的唯一根因已经证明或整个Windows job已经恢复。

新fixture helper按Windows TMP/TEMP/USERPROFILE/Windows目录优先顺序校验
原Rust目录，并用native realpath与Rust canonicalize保持物理路径一致。原UUID
命名、直接临时父目录、ownership marker与拒绝linked root全部保留。Playwright
artifact观察继续检查Node实际临时目录，支持别名parent，缺失parent只跳过
诊断，保留原rm(force)行为。helper只由源码测试fixture使用，不进入产品pack；
未修改产品worker、env allowlist、启动/审批/cleanup时钟或用户浏览器配置。

实际验证：

- 同一manifested App test artifact：TMP/TEMP不同与相同两组各 **10 passed /
  0 failed / 1 ignored**，原八项全部通过。最后artifact observer修正后的最终
  不同目录候选再次 **10 passed / 0 failed / 1 ignored**，原contexts/Job owners
  join，三次fixture根目录均无残留。人工十分钟deadline仍ignored，不算通过。
- 新路径合同：固定产品Node20在Windows **7 passed / 0 failed / 0 skipped**；
  Linux Node也 **7 passed / 0 failed / 0 skipped**。覆盖变量优先级/fallback、
  丢marker/错误名字/linked root、Node artifact别名与missing-path行为。
- 首次扩大Node source-worker测试因未传显式测试Chromium参数，**212 passed /
  7 failed / 0 skipped**，首败说明保留。为五个准确失败文件提供固定Node20/
  Chromium seed与原source worker后，完整native重跑 **15 passed / 0 failed /
  0 skipped**（含嵌套合同），原workers/browser退出且profiles删除。不是原始
  invocation全套一次通过，也不是installed或真人模型验收。
- Node语法、diff与final质量门禁通过；没有Rust或前端产品修改，既有workspace
  fmt/依赖检查保持，App+Workbench13552行与产品≥1000行80文件未增长。

证据、准确artifact/hash与再生命令：
`tools/computer-use-probe/.run/ci-windows-startup-20261013-01/`。
首个PowerShell5验证入口因缺ProcessStartInfo.ArgumentList未启动测试，后续
PowerShell7运行才计验证；不把这个夹具环境错误列为产品故障。

已确认四次原test owners退出，并核验准确targets/reparse points与CIM进程身份，
保留原八个失败store/marker/hash后，删除两份manifested EXE副本和五个原测试
temp根。逻辑 **148095811 bytes**，H盘可用空间观测增加 **148172800 bytes**；
精确物理回收未知。主Cargo产物、不可变seed、依赖、原失败/全部正例证据及
用户包/home保持；第五批05/06 native upper树仍暂缓。

最后观察 `22e6dfb7` 的 [run 38392655799](https://github.com/yclenove/grok-app/actions/runs/38392655799)：
frontend、两个SDK与维护ARM64 browser诊断成功；ARM64产品Rust失败，其余
Rust与production Jammy仍运行。9eb run随后由workflow转为cancelled，本批
没有主动取消/重跑。后续先回读最终CI，若启动仍失败则补有界安全诊断，
不能仅放宽gate时间。另保留production supervisor的已识别债务：stderr当前
在端口成功后才drain，端口前错误缺诊断，需另批修复/验证；不是本批已完成项。
继续CPU/header验证、维护来源/完整NOTICE、安全策略、五target产品集成及
所有原R1–R4安装/模型/四surface/UI UX/同最终候选12h门禁，整体active/partial。

## 第七批：启动输出 drain、有界诊断与原 owners 收束

提交前再次成功fetch两个remote，main仍分别为 `b58b97cd` / `9c5eb3b5`，
均为当前分支祖先。第六批 `c8217d08` 的
[run 38394968937](https://github.com/yclenove/grok-app/actions/runs/38394968937)
已结束，整体failure；原八项Windows managed启动测试已在远端通过。
Mac Rust、production Jammy完整release App/strict/navigation、frontend、
双架构SDK和维护ARM64 browser诊断成功。仍有三个实际缺项：

- Windows：`private_pipe_reaches_legacy_node_parser_on_this_host` 报
  `Plugin auth timed out`，2398 passed / 1 failed / 87 ignored。本机单独5项
  与全量均通过，尚未证明远端超时根因，不通过放宽deadline掩盖。
- Linux X11：Core1001项、X11库49项及第一段原生X11夹具通过；随后GTK
  可访问性进程报D-Bus `a(sss)`结构序列化fatal，观察报geometry changed。
  需要核查实际AT-SPI生产协议与夹具，不弱化身份/geometry检查。
- Linux ARM64：prepare/check仍拒绝 `aarch64-unknown-linux-gnu`；SDK及
  browser内部诊断绿不等于产品target、来源/通知与打包已完成。

### 修复与真实验证

旧supervisor在收到端口后才读取stderr，启动期大量错误输出可塞满pipe，
使Node无法继续发布端口。机制控制实验中，同一child写2MiB stderr，8秒
未drain没有port，随后drain便发布port；没有restart，也不称旧完整App复现。

新 `StartupOutput` 从spawn开始drain stderr，只保留饱和字节计数和七种
安全错误分类；固定匹配窗口，不保留/回显原日志、token或路径。首stdout
端口消息上限1024 bytes，端口必须为非0 u16；发布后stdout继续drain到sink。
原Child、Windows Job和两个reader JoinHandle保留到物理退出/join。清理
超时的原supervisor进入failed-startup registry；相同profile下次启动先重试
清理，产品shutdown也收束它。真正8秒启动deadline不变，2秒reader观察
期限不替代物理退出。Unix pre-port只验证root/readers，任意后代树合同仍开放。

- 最终Windows真实manifested App harness：**2407 passed / 0 failed /
  87 ignored**，200.44秒；ignored未算通过。supervisor15项、源码worker/
  Chromium任务10项（1 ignored）、plugin auth单独5项均通过。
- 最终Rust1.99 App all-targets strict Clippy、workspace fmt与final质量门禁
  通过。未修改前端；App+Workbench13552行与产品≥1000行文件80均未增长。
- Jammy独立小crate使用未改动的最终 `startup.rs`：**4 passed / 0 failed**；
  不是完整Linux App或一般Unix后代树验收。首轮私有root的环境失败保留，
  最终在私有mount namespace修正，没有改全局root/mount。

证据与再生命令：`tools/computer-use-probe/.run/ci-worker-startup-20261013-01/`，
包含完整App/strict/supervisor日志、原pipe控制实验、Linux source SHA与清理
receipt。两份manifested临时EXE的原owners退出后精确删除，逻辑
**148354048 bytes**，H空闲观测增加 **148357120 bytes**；Linux临时测试
binary逻辑 **10509280 bytes**，WSL空闲观测增 **10506240 bytes**。
精确物理回收未知，WSL差值不代表VHDX缩小；所有失败证据、原Cargo产物/
依赖/seed与用户App144/home保留。05/06含links/socket的upper树继续暂缓。

本批仅commit/push到fork，无PR/tag/Release；两份第三方研究另行文档提交。
继续上述三个CI缺项、CPU/header、维护运行时完整来源/NOTICE/安全策略与
五target产品集成，以及原R1–R4全部安装/模型/四surface/UI UX/12h门禁。

## 第八批：X11 AT-SPI 动作发现协议

Claude完整审阅基线为`5455eff9`，交接/六仓库研究已推`9309a6bb`。本批为其后
独立的产品修复。再次fetch两边remote后main无新增，远端feature与本地HEAD
一致；不修改用户App144、home、登录或未发送任务。

### 根因与对照

Jammy/glibc2.35私有overlay使用原libatk-bridge2.0-0 `2.38.0-3`、GTK
`3.24.33-1ubuntu2.2`及libdbus `1.12.20-2ubuntu4.1`。独立Gio探针对同一GTK
按钮读取NActions/GetName正常；调用GetActions则得到与远端相同的`a(sss)`
序列化错误、GTK原进程SIGABRT（exit -6）。另一次独立进程仅用机器名称读取
与DoAction，GTK自身回读clicks=1且进程仍活着。

实际[GNOME 2.38源码包](https://download.gnome.org/sources/at-spi2-atk/2.38/at-spi2-atk-2.38.0.tar.xz)
SHA256为`cfa008a5af822b36ae6287f18182c40c91dd699c55faa38605881ed175ca464f`。
`impl_GetActions`声明`(sss)`却写入name/localized_name/description/keybinding四个
字符串。新桥的[协议](https://github.com/GNOME/at-spi2-core/blob/AT_SPI2_CORE_2_52_0/xml/Action.xml)
则明确GetActions返回本地化名称，机器ID应从GetName读取。不是Rust三字段
反序列化导致应用中止，也不应删除geometry检查来掩盖后续失效。

原产品源码的真实cu-x11-native在同一私有Jammy/Xvfb下：输入与剪贴板通过，
可访问性复现同样的provider abort与geometry changed失败。没有改原GTK夹具
或native验收断言来取得之后的成功。首次私有bus缺DISPLAY和独立探针对GTK
动作数量的错误假设均保留在日志；它们不是产品故障或已通过的原始调用。

### 产品修改与验证

- 生产observe与click重新发现都改用NActions/GetName，最多64项，每个机器名
  最多128 bytes；共享观察deadline，点击发现有一秒预算，过期不继续发下一
  查询。每个D-Bus调用原一秒timeout保持，单个正在执行的读取仍可能跨过
  总预算。最终重新检查动作数量，拒绝变化、部分结果与歧义；只接受完整
  click/activate，不把press/release当完整点击，也不使用本地化显示名。
- CI先启动独立Xvfb再启动独立session bus，让激活的AT-SPI服务继承准确
  DISPLAY/XAUTHORITY；原timeout、身份、geometry、焦点、取消与unknown门禁保留。
- 最终Jammy X11库 **52 passed/0 failed/0 ignored**、Rust1.98 all-targets
  strict Clippy通过；真实Xvfb原生输入 **19**、GTK可访问性 **41**、剪贴板
  **66** 个独立检查全部通过。含中文/emoji、一次点击、替换控件、Stop、
  迟到原生回执与Broker不重放；不是完整App/安装态GNOME Wayland验收。
- Debian GTK3.24.49/bridge2.56.2的独立机器名探针也完成一次点击与GTK回读；
  不把它当另一个完整产品native矩阵。workspace fmt、diff、final质量门禁、
  YAML解析及实际X11 step的Bash语法通过。初次Node YAML入口缺少模块，后续
  使用现有PyYAML验证；无前端改动，App+Workbench13552行/≥1000行文件80未增。

证据/再生命令：`tools/computer-use-probe/.run/ci-x11-actions-20261013-01/`。
同原owners均退出、无残留task mount或进程root/exe/cwd/fd引用后，验证确切
路径与全部子项无links/特殊文件，清理一个已退休test binary和两个旧incremental
generation：逻辑文件 **255222800 bytes**；WSL可用空间观察增加
**226217984 bytes**。精确物理回收未知，不代表VHDX缩小。保留当前native候选、
当前generation/依赖层、原base、全部源码/失败与正例日志和用户数据；05/06旧
namespace upper含links/socket仍暂缓。按记录的cargo test/build可重建。

`9309a6bb` [run38410195410](https://github.com/yclenove/grok-app/actions/runs/38410195410)
的Windows job已终态failure：2406 passed/1 failed/87 ignored，仍只有plugin auth
10秒超时；启动输出相关测试通过。ARM64产品target仍失败，其他未终态job不
标绿。本批修复尚待新远端run，不把本机X11通过标为全CI或完整交付通过。
继续Windows私有stdin/EOF及并发进程诊断、ARM64产品运行时、CPU/header、完整
维护来源/NOTICE/安全策略、五target及原R1–R4全部安装/模型/四surface/UI UX/12h。
