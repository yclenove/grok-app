# 原任务暂停后的准确收尾状态

2026-10-05；单 Codex writer；`feat/computer-use-implementation`。
本批修改生产 Core 与 UI，不改权限范围；未 commit / push / PR / 发布。
完整四 surface、三平台、两 Mac 架构、E5/U04–U09/D00–D31 的交付范围保持。

## 修复与证据范围

source111 的真实反例是：原 native PNG return 仍 held，Host 已显示
`cleanupPending=false`，但原 extension observation/watch/settle 尚未 join。
一次 Continue 等待和 Stop 的执行逻辑已通过，状态显示提前宣告收尾结束。

source112 从原 ExistingTabHost 保留的 action/observation queues 与 completion
registries 读取原 run 的本地占用。Host 选择 task/generation 与该同步读取使用原
Broker→tabs 锁顺序，不调用 adapter、OS、browser worker、续租或启动运行时。
Desktop/Managed Browser 保留各自已有状态，不据此宣称全部平台收尾缺口关闭。

卡片与电脑面板共用状态：暂停仍有原工作时显示“已暂停 · 正在收尾…”，一次
Continue 显示恢复中；Stop 始终可达。所有 stopped 但 cleanup pending 的执行
仍显示停止中，不显示任务完成。15 种语言补齐；没有新增 picker/授权动作。

## source112 实际验收

证据根 `E=tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。

| 门禁 | 实际结果 | 范围 |
| --- | --- | --- |
| Core default / test-support all-targets | 1104 / 1120 passed，0 failed / ignored，原 runner exit0 | retained occupancy 生命周期与 Core 回归 |
| 前端 CU / lib / i18n | 31 files / 401 tests passed，原 runner exit0 | 两 UI、一次 Continue、Stop、准确完成终态、15 locale |
| 原 native capture 定点 | 4 passed / 0 failed，exit0，79.22s | Pause、Chat Stop、early Continue、waiting Stop |
| Rust strict | Core / Core support / App / probe 均 exit0 | `-D warnings` |
| 其它检查 | typecheck、targeted eslint、quality、diff、Rust fmt 均 exit0 | 本批相关代码 |

四项定点均使用原 session MCP、认证 HTTP、原 App/ACP publication、source MV3
worker 与真实 native PNG。Host 手势/ACP peer 为 synthetic；owned CDP action
不是真人 installed toolbar。它们不替代实际 App、真实 Grok 或 U07 三轮验收。

每项原 capture held 时 Host cleanupPending=true、idle=false；原 join 后 idle=true。
旧图 decode=0、旧 image/text 成功不发布。提前 Continue 共用一个 reservation，
原 join 后仅一次 fresh recovery read、一次 continuation、一次 click；等待中 Stop
立即 fence，释放旧 PNG 后新 read/action/continuation 都为0。原 owners 已退出。

source112：3325 件；manifest SHA-256
`e1933babe32b21387aee93f40f061cfb93fafb96a29c36d4ee3c0372a2944d25`。
默认 lib 测试 EXE SHA-256
`34885b81791384af127ba92beb1cefbe38cb620783eb60de08668df72afed3eb`。
原 CFT154 SHA-256
`f0b204ce7239e221f84c4030cd38e4b688ad05320acc8373da166f548bf57eb5`。
最终核验 `E/verify-cleanup-readback-112.ps1`，结果/四 witness/门禁 hashes
保存在 `E/cleanup-readback-112-result.json`。没有重复未变化的完整81场景矩阵。
source109/110/82 等原首败日志、profiles/PNG 与所有原记录保持不变。

## 实际应用与下一步

source108/app109 的真人批准、中文备注、恰好3次计数、18工具/0错误、一次明确
完成与浏览器退出证据保持有效，见 [app109 检查点](2026-10-05-computer-use-completion-app109-checkpoint.md)。
它不包含本批新状态修复，不能当作 source112 的真实恢复验收。

新 app113 已以 source112 实际构建，fresh frontend 39.30s、Rust应用1m03s，
dev identifier、完整便携运行时与450文件 ZIP 逐件解包校验均 exit0。
MSVC 的创建库/对象 stdout 被 rustc 报为 linker_messages warning，未含实际 LNK 错误。
源在构建前后与包装后复验一致；构建/启动/业务结果分独立记录，不回改原 artifact 字段。

- EXE：`dist-portable/Grok_CU_20261005_app113/Grok.exe`，92957696 bytes；
  SHA-256 `e293476852d50ec496980d39e19fdf5ab8a1194bb0f70ee9d2e94dcf836faa0b`。
- ZIP：`dist-portable/Grok_CU_20261005_app113.zip`，208408116 bytes；
  SHA-256 `1fdf07e206f45dbf6d8ca1c317a833b61980a6c732b76b91089db0e3f1251560`。
- 文件清单 SHA-256 `9b1a2427f6a1b561cd799b98cb30a57a59531ebe98bbffc4bc33791705ae94f0`。
- 启动入口：`E/delivery-app-113/launch-preview.ps1`，VerifyOnly 已通过，无重复预览。

本机已完成该包的真实Grok一次暂停/继续后的重新观察与明确完成，卡片完成/原browser退出。
三次计数均在暂停前；恢复后新业务操作与进行中的主动Stop尚未验。
另发现聊天取消标记排在最终答复下方，已接入[source114修复与实测](2026-10-05-computer-use-transcript-recovery-checkpoint.md)。
app113原App/CLI/MCP/worker/browser/fixture均已退出，原home与业务证据保留。
复用原独立 home、登录和 sessions，最多保留一个当前预览；不修改日常浏览器。

安装态 Chrome/Edge 和真实 Share/Host 手势、其它 surface/platform、原生窄窗/DPI/
键盘/overlay、安装升级回滚、模型矩阵和冻结候选12h主动长稳仍开放。
继续 [观察v2后续计划](2026-10-05-computer-use-task-consent-u04-existing-observation-v2-next.md)，
总体 active / partial — not releasable，不将局部定点测试作为全产品完成。

## 构建收尾

原build/package/test/browser owners终态后已执行source112/artifact112的fresh exact清理：
17组旧incremental generations，逻辑10099996805 bytes（9.41GiB）、
H空闲观察增6016630784 bytes（5.60GiB）。清单 `E/cleanup-readback-cache-112-cleanup.json`。
保留最新代、当前包、依赖/seed、原登录/sessions、源码/dirty/fix.patch和全部有效/首败证据，hash复验一致。
不在回收站，可cargo重建；未知working/跨variant/失败现场继续延期。下一候选用fresh身份再审，不复用112清单。
