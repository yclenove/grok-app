# Windows 下载 buffer 联合收尾检查点

整体仍为 active / partial — not releasable。三平台（含两种 Mac 架构、X11/Wayland）、四 surface、installed 升级修复回滚、UI UX、真人模型矩阵、冻结 12h、D00–D31 不缩减。本批没有 commit/push/PR/tag/Release。

## source136：真实 complete private file / 原 callback / 原八秒 grace

- source136 manifest：`7405458a3b7c38d8df4c5d6c5a8af551d660920d563c8ece17423e5ae37e479f`，3422件；变化只增加debug probe验收路径和runner退出诊断，产品下载行为仍是source135修复。不把probe当用户包。
- fresh Cargo App probe SHA256：`25c090d78420a1c668f216f974ae8279e63223d7f8cdc21df5dac896e87f38c1`。原 build/native runner58181 exit0；实际App58332 exit0、12 observed owners全退出，原30秒gate通过。保留完整build/native/owners日志。
- `app-download-buffer-retirement-20261012-01` 两项均实际下载完整1048576 bytes并核内容，Broker UTF16路径324/322、native104，确实private buffered。第一原native终态到达时Wry0 processed，文件和占用仍保留。
- callback-first：真实Wry释放后原caller成功复制到Broker路径，内容完整；caller-first：原Stop后 unchanged grace8013ms，caller返回native-terminal-timeout/unknown并join，文件/原registry/占用仍保持，旧action重放拒绝。随后只释放准确原callback一次，才清理private文件和空目录并退休原占用。
- 两项原close main IPC、原native resources退出、staging空、无迟到发布/重放通过；各自同label fresh owner37 bytes/hash正确，依然private buffered（325/323字符），keeper可用。
- 独立 `app-download-buffer-before-start-20261012-02` 同构建五项业务及原owners gate通过，runner39777 exit0 / App1044 exit0，11 observed owners全退出；原grace8005ms，Close5012ms失败后显式重试成功。原App join后约1.20秒的CIM回查已无这些owners。
- runner新增每个准确PID/birth/path的first/last observed live时间与browser/renderer/gpu/utility/crashpad角色，不输出完整command line。**没有放宽原30秒gate或停止任何浏览器进程。** 首次source135超时仍保留、根因未确认，不因复查绿而标已修复。
- 1412 class unregister警告在这两次正常退出也出现，因此不能将该警告直接当作首败原因。
- fresh默认libtest副本嵌入/回读manifest，21 download+49 WebView+66 ACP=136 passed/0 failed/0 ignored（在上述filters内），原runner16979 exit0。副本hash `226a1c94a6a78e4967b7052ea130eb5f426f01326ee762039560c06556bdd958`，非完整App套件。
- 三strict Clippy、fmt/diff/final quality与source前后Verify通过，原runner48810 exit0；App+Workbench13573、≥1000文件80保持。
- 两native副本与一green libtest副本在原runners/owners终态后准确清理，逻辑266036736 bytes；H空闲观察增266039296 bytes（约254MiB），没有独占磁盘归因。报告在 `download-buffer-retirement-cleanup-20261012-01.json`，记录生成方法。源程序、所有reports/logs/home、当前app133和首败保留；无journal旧Temp和不确定cache继续暂缓。

## source134 真实 red → source135 修复

- source134 manifest：`b498e2f87cf0f8cc9024ab67e3defb86adfc4dbfba9239f74e3a3cc4ff240db8`，3421 件。
- 默认 Cargo libtest 的独立副本嵌入并回读 manifest 后实际进入断言：2 passed / 2 failed，libtest exit101；原 runner13663 exit1。失败为提前删除文件的 `NotFound`，不是程序启动失败。红色副本和日志保留。
- source135 manifest：`3634ba1650d99509438d6cf2edbd12b795cdf4c10d11e103455c60637c777ea0`，3421 件。
- `complete` / `abandon` / 原 browser exit 只记录状态；原 caller 和 callbacks 联合收束后，实际移除准确 Arc registry entry 的那一次退休才清理并释放占用。
- NativeDestination 成功清理有对象级 latch，重复旧 callback / Drop 不再删除后来占据原路径的文件；失败仍可重试。此 latch 不是持久 crash journal。
- 六项真实文件回归覆盖 native-first、caller-first、callbacks-first、原准确退出、重复 callback 和 Drop replacement。
- fresh 默认 manifested libtest21 download +49 WebView adapter +66 ACP =136 passed /0 failed，runner62726 exit0；不等于完整 App 套件或真人 picker。
- 默认 lib、默认 tests、probe tests 三 strict Clippy、fmt/diff/final quality 全通过，原 runner91882 exit0。App+Workbench13573、≥1000 行文件80未增长；`FINAL: PASS` 仅质量。

## source135 实际 App/Wry/native：不能合并成全绿

原 Cargo probe SHA256：`a82893de4fce4d7f11b1b1842591466baa63db5f21549f18d5a0cdc2524dc8c8`。实际 App 库、Wry/native、synthetic Host approval；非 ACP/model、非用户发行包、非 fresh frontend。

1. `app-download-buffer-before-start-20261012-01`：Pause、Stop、Stop grace、Navigate、Close 五项业务后置条件通过。原 Broker UTF16 路径266/265/271/269/266、native104，均使用 private buffer；原 grace8012ms，原 Close5006ms失败后明确重试成功；每次同 label 新 owner37 bytes/hash正确、staging空、无重放。
2. 但原 owner gate **失败**：App34612 exit0，原 runner20520 exit1；两个原 WebView2 children21124/48624超出30秒退出门禁。原 `owners.json` 保留 `allObservedOwnersExited=false`。`2026-10-11T19:58:33.9606308Z` 的附加只读回查证明所有12原 owners随后自行退出，没有杀进程；它不反向改变原门禁结果。1412 class unregister 警告不足以证明根因。
3. 仅在全部原 owners 缺失后运行独立 `app-download-buffer-error-unwind-20261012-01`：App61528 expected exit2，直接 runner exit0，held真实 Wry callback0→1、原 caller join、资源/staging收束、同 label新 owner正常，所有原 owners在门禁内退出。实际先收到16384 bytes，error→retired16ms。
4. 此 error-unwind 的 Broker258/native254，**没有 private buffer**；不能当作长路径 held-file 的直接原生证明。下一增量补真实完整 private file、held Wry 和超过原8秒 grace 的 caller-first 验收。

## 清理与统计失败

- 成功绿色 libtest 副本已删：原记录逻辑76322304 bytes，H空闲观察增76337152 bytes；并发磁盘活动，不能独占归因。
- 两份 native probe 副本已删，保留的字节相同 Cargo源94,813,184 bytes/hash与两原owners reports一致。
- 原清理 wrapper 在删除后对 ordered hashtables 调 `Measure-Object logicalBytes` 失败，exit1，原 JSON未写出。恢复记录明确原 free-before/after/delta 缺失；189626368 bytes只是按保留源重建的逻辑总量，不是原删除观察或实际磁盘净回收。
- 原脚本修成 PSCustomObject，并在删除前完成数值汇总；没有在缺失目标上重跑删除。报告、home/login/history、源、失败现场都保留。
- 证据根：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`；red/green/strict reports和 `download-buffer-native-cleanup-20261012-01-recovery.json`；native reports在 `src-tauri/target/codex-test-runtime/` 两同名 run下。

## 用户预览与下一项

当前用户仍试用 app133 / PID10928，原独立home和历史保留。已再次实际查看主页面、设置/关于版本并返回主页面，只有一实例、不依赖1421服务；没有发新模型任务或自动授权。source135未打入用户包。

继续按 [D22 原归属与异常维护计划](2026-10-12-computer-use-native-buffer-recovery-next.md)：先强化原 callback/private-file 实机证据和退出延迟诊断，再接派发前不可变 journal、准确原内核 owner见证及显式维护。不能扫描 Temp 前缀清除无journal历史文件。app133真实新意图/真人批准/picker及完整交付门禁仍开放。
