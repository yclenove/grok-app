# Computer Use：运行时种子发布、回滚与精确恢复

完整目标仍是“接手 grok 的工作，完成 computer use 所有功能开发，推进到最终版”。
本检查点不把目标缩成运行时测试通过；默认关闭、Host broker、禁止桌面兜底等产品约束未变。
记录使用本机 Asia/Shanghai 日期。没有提交、推送、发布安装包、替换用户 App/运行时或授予权限。

## 1. 真实缺陷与先失败证据

接续 [观察/输入竞态检查点](2026-09-27-computer-use-browser-observation-mutation-checkpoint.md) 的 H: 发布拒绝访问、清理共享冲突及 Windows PID 发现偶发 null。
检查 `runtime_prepare.rs` 后发现三个独立、可复现的恢复缺陷：

- seed 存在时，仅凭 `.seed-staging-*` / `.seed-backup-*` 前缀删除所有同级匹配项，包括不属于当前发布的目录。
- seed 缺失时，随意选择首个前缀 backup 并提升成请求的 seed，没有目标归属证明。
- 多个无 journal 的 backup 也被猜选，而不是保留现场并要求明确恢复。

`ownership-red.log`：3 项全部失败，exit 101，分别观察到误删 staging、外来 backup 被提升以及歧义状态未拒绝。
这证明旧恢复策略有误，**不证明它们就是之前 AccessDenied 5 / sharing violation 32 的根因**。
旧发布流程还忽略 rollback 与 backup cleanup 的错误，本轮不再吞掉这些错误。

## 2. 当前实现与恢复边界

`src-tauri/computer-use-core/src/runtime_prepare.rs` 保留现有父目录 mutation lock、来源校验、完整 staging 校验和 Node import probe，再调用新的私有 `runtime_publish.rs`。

- 发布前记录并 `sync_all` 一个 `create_new` journal：精确目标名、UUID staging/backup、准备包与旧包的完整树摘要，以及实际存在的 staging owner marker 摘要。
- journal 文件有大小上限、严格字段/路径/摘要校验；不覆盖 pending journal，拒绝 symlink/reparse 路径，不根据目录前缀或 PID 消失推断所有权。
- 旧 seed 只移动到该 transaction 的 backup；准备好的 staging 只尝试发布一次。失败时仅尝试恢复精确旧 backup，不重试失败的原生发布。
- 发布与回滚都失败时返回 `publication_rollback_failed`，同时保留两项错误及 journal。回滚成功但 journal 清理失败也显式报错。
- 发布后的新树再次核对成功，才清理该 journal 精确指定的旧 backup。backup 清理失败不能报告整次发布成功，后续可以完成部分清理。
- 恢复前提交状态时，缺失的旧 seed 只能从摘要匹配的精确 backup 恢复；不把尚未提交的 staging 偷当成成功发布。
- seed/backup/marker 被替换、丢失或存在歧义时保留证据并返回 `publication_recovery_required`。捕获过的 marker 必须仍匹配才删除；未捕获的 marker 不删除。
- 无 journal 时不清理前缀目录。seed 缺失且存在 legacy backup 时拒绝猜选；需检查现场后明确处理，而不是自动丢弃可能唯一的旧副本。

这是**合作式父目录锁下的种子发布事务**，不是敌对同用户任意文件系统写入下的原子保证，也未声称完整断电持久性或签名安装器更新/回滚已经验收。
正常准备 API/CLI 没有增加测试控制入口。rename 失败注入仅为私有函数参数，生产始终调用真实 `fs::rename`。

## 3. 回归与 Windows 原生共享冲突

新增 `runtime_publish_tests.rs`。覆盖初次发布、替换、提交前/后的恢复、部分 backup 清理、旧副本丢失/修改/占位、journal 越界/跨 seed/不可覆盖、marker 变化，以及同时保留发布/回滚错误。

Windows 两项测试持有测试自建文件的真实不共享删除句柄：

1. staging 的真实共享冲突导致发布失败，旧 seed 得以恢复；释放自己的句柄后，显式下一次发布才能成功。
2. old seed 移入 backup 后持有其文件，真实 backup 清理失败必须报错，保留新 seed、backup、journal；释放句柄后完成精确恢复清理。

Unix 另有 linked seed/staging/backup/journal 拒绝跟随及 foreign 目标保持不变的验证。
这些是实际文件系统与原生共享模式证据；构造中断后的文件布局不冒充真实断电或杀进程验收。

证据目录：`tools/computer-use-probe/.run/runtime-publication-20260927/`。

| 检查 | 已核实结果 | 日志 |
| --- | --- | --- |
| 旧恢复策略反例 | 0/3，exit 101 | `ownership-red.log` |
| Windows 发布边界 | 14/14，exit 0 | `publication-boundaries.log` |
| Linux 发布边界 | 13/13，exit 0 | `linux-publication-r2.log` |
| Windows Core 全部 lib 测试 | 561/561，exit 0 | `windows-core.log` |
| Linux Core 首轮 | 559/560，exit 101；传入旧 seed 导致 worker 来源摘要不匹配 | `linux-core.log` |
| Linux Core 当前生产新 seed | 560/560，exit 0 | `linux-core-current-seed.log` |
| 双宿主 Core strict Clippy | all-targets + test-support，`-D warnings`，均 exit 0 | `windows-core-clippy.log`、`linux-core-clippy.log` |
| Core 格式检查 | exit 0 | `core-format-check.log` |
| H: 新包观察竞态原生测试 | 5/5，exit 0，shutdown 200 / 原 worker exit 0 / owned browser 消失 | `h-native-packed-worker.log` |
| Windows 显式新包 Node/Chromium 五文件回归 | 31/31，exit 0，约 81.38 秒；worker/模块来源见下文 | `h-native-suite-explicit-chrome.log` |
| Linux 显式新包 Node/Chromium 五文件回归 | 31/31，exit 0，约 53.13 秒；worker/模块来源见下文 | `linux-native-suite-explicit-chrome.log` |
| Windows 原生回归后生产 check | exit 0，摘要不变，importProbe passed | `h-runtime-final-check.log` |
| Linux 同目标生产 replacement + 最终 check | 均 exit 0，摘要不变，importProbe passed | `linux-runtime-replace.log`、`linux-runtime-final-check.log` |

发布边界测试已包含在 Core 总数内，不重复累加。独立的 5 项竞态回归也包含在五文件 31 项之内，不累加成 36 项。
以上进程均已收取终态。没有将观察超时当作进程已停止，也没有把失败后无源码变化的成功重跑当作历史根因被修复。

## 4. 生产打包与测试环境纠正

H: 新独占测试目标的生产 prepare、同目标 replacement、check 都 exit 0 且 importProbe passed。路径保存在 `h-seed-path.txt`，不是用户安装目录。
Windows 包摘要与上轮成功的 C: 包一致，浏览器源码本轮未修改：

```text
manifest 001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec
tree     6fdd22b23b1798cde0aa8d2c82784e8abbcf0967741cbc5c355ffa8b93d102d8
chromium 63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3
```

Linux 使用已有 pinned 工具链/离线依赖和已缓存归档，通过生产 `cu-prepare-runtime` 创建全新独占目录；没有手拷 worker 修补旧 seed。新 seed prepare/check 均 exit 0，importProbe passed，路径在 `linux-seed-path.txt`。

```text
manifest 076c24fb07a1e77d558c697643ec24c79d644b8108e885039d5d2cb655985e59
tree     693ac00e9baafeea27fd6ecd93ecaf26bf8a0ee29c3d6d44f8dad1ec7b7b890f
chromium b203864ac3ee28fde712f33d13b90e7c67682f22207e36ff253b8b34ee2827bc
```

失败记录全部保留：

- Linux 最初启动器没有设置 pinned Cargo PATH，exit 127；不是测试通过。纠正工具链路径后发布边界测试通过。
- Linux 首轮 Core 使用旧 `seed-atspi-20260926`，精确报 `hash_mismatch` 而非测试预期的 `missing_sibling`。保留原失败，不放宽来源校验；用当前生产新包重跑后 560/560。
- 首次独立新包原生命令误将 worker 指向包根而非 `playwright/worker.mjs`；Windows/Linux 都因 preload 找不到模块而失败。只纠正运行命令，不修改包或测试预期。
- 首次五文件命令漏设 `GROK_CU_TEST_CHROME`：Windows 26/27（一个夹具要求显式路径，其他夹具可能选到了系统候选），Linux 7/21（没有可用候选）。这些不是隔离包合格证据。纠正命令同时显式设置 `GROK_CU_CHROME`、`GROK_CU_TEST_CHROME` 和 `GROK_CU_TEST_WORKER`。

五文件集合为 handle-lifetime-browser / observation-http / observation-mutation-http / typed-act-http / worker-error-http。
前两个使用当前源码 collector/worker；后三个支持并选择包内 worker。不能把这组全部描述为包内 worker 覆盖；需同时记录源码与打包模块摘要对应。

## 5. 源码、包内容与清理核对

- `source-before-native.json` / `source-after-acceptance.json` 冻结核对 241 项源码、测试、依赖与 CI 文件，零漂移；该清单不是整个仓库所有文件。
- `windows-packed-module-audit.json` / `linux-packed-module-audit.json`：每个平台 19 个生产 worker 模块与当前源码逐字节 SHA-256 一致，server 映射为包内 worker。两包均不含私有 capture barrier fixture。
- 双宿主新包 prepare、replacement、最后 check 的 manifest/tree/Chromium 摘要在各自平台内完全一致；Mac 包与签名安装器未在本轮运行。
- Windows scoped CIM 与 Linux scoped `/proc` 最终只读扫描未发现所选 owned seed/fixture 标识对应的残留进程。主要清理证据仍是测试自己的 shutdown、原进程退出及原 browser PID 消失断言，不把一次扫描当作全部系统进程证明。
- 两个包父目录未残留 `.seed-publication-*`、`.seed-backup-*`、`.seed-staging-*`；既有 cooperative mutation lock 文件可保留，不能把 owner 文件当作进程仍存活的证据。
- 首次命令错误留下的空/失败 fixture 目录及历史 H: 失败目录没有被前缀批量清理；不按进程名终止用户浏览器。
- `checkpoint-audit.json` 汇总实际退出码、通过数与包摘要；`source-receipt.json` 记录本次源码、三份文档及本目录证据文件的 SHA-256，生成后逐项重新读取核对。

## 6. 尚未关闭的根因与完整最终版门禁

- H: 本次发布/替换成功不等于历史 AccessDenied/共享冲突根因已确认。原失败目录和日志保留；新增树摘要读取也可能改变时序，不能据此假设外部锁持有者已解决。
- Windows 精确 owned Unicode/空格子进程 PID 诊断：生产 5/5、同查询诊断 5/5，原 2 秒期限未变，自己的子进程已退出并清理；没有复现先前 null，也没有修改 PID 发现生产实现，因此根因仍开放。
- 历史导航偶发 504、原生退出长尾未因本轮发布改动而解决。
- Windows/macOS arm64+x64/Linux X11 + GNOME native Wayland 的完整原生能力、输入/IME/剪贴板、自绘控件、生命周期/取消/未知结果恢复仍须逐项验收；FFI double 不等于 Mac 真机。
- Desktop、managed browser、existing tabs、App WebView 与 App/ACP/MCP 的完整权限及功能合同仍保留原范围。
- 签名安装/更新/回滚、当前源码重设计原生 UI 的窄窗/缩放/权限路径、真实 Grok E4、冻结 12 小时 active soak 全部仍开放。

未将 goal 标记 complete/paused/blocked。此前工具转接失败没有项目进展；本轮重新轮询同一已知 handle 收取终态，没有因观察失败重启原准备进程。
