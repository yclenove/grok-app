# source144：真实 App 下载 journal 异常组合

## 结论与边界

`app-download-journal-faults-20261012-02` 原工具 session1536 exit0，九次真实 Host 维护及文件后置条件通过。原 App/Wry 下载真实 1MiB 后外部 fixture 仅按已持有的原 App handle 制造 exit99；新 App 使用同一独立 home 产生另一笔真实下载，并在原20秒 bounded hold 后正常 unwind/exit2，没有强杀新 App 或浏览器。

这关闭了 D22 Windows 私有下载 journal 的下述局部异常验收，不关闭 D22 全项或完整交付。Host approval synthetic，ACP/MCP/model/真人文件 picker/freshOrdinaryFrontend/fullDeliveryAcceptance 均 false。用户唯一预览仍为已实看普通入口的 app144/PID19768；两种证据不拼成真人 CU 验收。

## 候选身份

- source144：3432 源文件，前后 Verify 一致；manifest SHA256 `cae93221eb14ba196722b6fce39065f654302b637bd21cca3590f0c4e96b9fd7`。
- 使用保留的 `app-normal-exit-before-start-20261012-02` 原候选副本，不使用后来被普通打包重建的 target/debug probe。
- native EXE SHA256 `bd94cc9728ee037e21b0812abb458b160d7798f2936d1786cd9f23cfd691ba46`，95181312 bytes。
- 实际原 App72884；真实下载 ready 中 browser41916。原 Wry nativeTerminal=1/wryFinished=1/wryProcessed=0、downloadRequests=1、brokerStagingEmpty=true。
- 原 App crash 后原30秒 owners gate 272ms；新 App正常 unwind 后原30秒 gate 130ms。共39个已观察原 kernel handles 在最终报告均 signalled；不重开PID冒充原handle，不以后来缺失回填原门禁。

证据：`src-tauri/target/codex-test-runtime/app-download-journal-faults-20261012-02/native-faults.json` 及 home 下九个 `maintenance-*.json`、ready/native result 与stdout/stderr。runner 位于 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/run-native-journal-faults-144.ps1`。

## 实际场景

| 场景 | Host 结果与后置条件 |
| --- | --- |
| 同home新 App/run仍live | exit2，filesystem refusal；新旧数据/记录hash不变、无retirement receipt；新 App仍live并随后独立退休自己的记录 |
| 空/半写 record | exit2；破损记录、原data/marker不改，不准备任何删除 |
| 非法JSON record | exit2；同上；原文件原位恢复后FileID不变 |
| 内容相同、FileID不同的 record | exit2，record changed；原备份与新占位对象均保留，恢复准确原record |
| readonly data | exit2，真实Windows删除失败；原data/marker/record不变，但真实生产retirement receipt已发布 |
| receipt后data路径新占位 | exit2，original data replaced；占位hash、原备份hash/FileID、receipt hash不变 |
| receipt后buffer目录新占位 | exit2，buffer directory replaced；新目录内容、原目录FileID/data、receipt不变 |
| 恢复准确原对象 | exit0，buffer/record/receipt准确删除，journal只余owner/store.lock |
| 重复维护 | exit0，无再次副作用 |

readonly 是真实删除拒绝，不冒充独立 ACL、查询权限 unavailable 或 PID replacement 验收。文件/目录替换用同卷 rename 保留原FileID；data backup 是准确 Temp parent 下 transaction-specific sibling，不跨 C:/H: copy/delete，也不递归移动宽目录。

## 首败与 runner 修正

`app-download-journal-faults-20261012-01` 原 session6625 exit1：维护新live记录返回 sanitized filesystem refusal，runner却只允许“still live”，因预期过窄而停止。原报告保留，其31 owners最终报告当时未全退出，不能改称该批通过；独立后来检查原记录PID/路径/出生对应的owners已缺失，新 App的自身report显示原bounded hold正常unwind。

源码 `safety::write_new` 保留写入/DELETE访问且仅共享READ，`candidate`以DELETE访问打开record。因此live记录可在owner.retired校验之前因共享规则拒绝；具体底层Win32错误没有记录，不声称已捕获错误码。runner现在只接受这两个明确拒绝类，仍要求live身份、所有字节不变与零receipt；没有放宽误删断言或产品锁。

另在执行前修正跨卷data备份缺陷、无用参数，并独立验证PowerShell parse、显式空byte数组/SHA256及原位写FileID不变。没有产品Rust修改、没有默认启用功能、没有自动授权。

## 批末清理

39原owners最终退出后，以及首败owners独立后查已缺失后，准确删除两份byte-identical native fixture EXE与15-byte helper输入。保留原native候选、144 libtest、用户144包/ZIP、133已知可用回退、所有home/profile/log/report/失败记录。

逻辑190362639 bytes（约181.5MiB）。原清理在文件删除后发生ordered-dictionary `Measure-Object bytes`统计错误，exit1；报告已透明恢复，未重试删除。原before/after空闲值未及时持久化，**实际空闲增量不可得，不虚报回收量**。runner已改为先写preflight与逐项removal，再直接累加dictionary bytes。证据 `native-journal-faults-cleanup-20261012-01.json` 和 `...-recovery.md`。

首败的旧journal/1MiB private file暂作失败证据；未知cache/PDB、其他worktree和active用户包不清理。没有递归删除workspace/target/shared cache，没有停止无关进程。

## 下一项

1. 普通 app144 原ACP/MCP→新意图→真人范围选择/批准→原生文件picker→实际上传/下载 receiver 后置条件，以及Pause/Continue/Stop/Quit全链；原132请求已过期，不复活。
2. D22其余worker/ACP/watchers/sleep-resume、PID replacement/查询 unavailable、独立ACL与其他平台项目仍需对应证据；138/139退出延迟根因仍开放。
3. 三平台、两Mac架构、四surface、安装升级修复回滚、UI/UX、模型矩阵、冻结同候选12h及D00–D31继续。整体 active / partial，无commit/push/PR/发布。
