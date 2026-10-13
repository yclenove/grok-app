# U04 App 精确范围批准接线 checkpoint

状态：implementation in progress，整体目标 active / partial — not releasable。
后续增量：[U03 typed Host 回读与原任务决定](2026-10-03-computer-use-task-consent-u03-host-readback-checkpoint.md)。
以下保留本批历史范围与证据；后续新增接口不能当作本页当时已完成的 UI 功能。
分支 `feat/computer-use-implementation`，HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。
本页接续 `2026-10-03-computer-use-task-consent-u04-checkpoint.md`，不覆盖历史失败或验证结论。
单 Codex writer；保留原 dirty/untracked 文件；没有 commit/push/PR/发布。

## 本次产品修改

1. `computer_use_task_approve` 接收 Host UI 明确选择的 `selectedTargets`，沿原
   SessionManager endpoint / ACP RequestTransport 调用，不从 legacy current run 猜任务身份。
   DTO 只接受 canonical surface 与 targetId，拒绝额外授权字段、重复/空/过长目标及超过
   Core 上限的集合。当前生产接线支持 Desktop；其它 surface 没有真实 producer 时明确
   unavailable，不能拿配置名、标题或 model scopeIntent 充当身份。
2. 原 ACP 锁下只核 owner、request/revision，并生成不可序列化的选择预约；原生工作在锁外。
   全程保留原 transport Arc、agent 与选择代次，准备后/最终批准后均复验，拒绝 successor
   transport。重复提交相同已批准成员返回原 outcome；变动成员拒绝，不静默扩大/缩小。
3. Core `HostScopeSelectionCall` / `HostScopeSelectionCancel` 先预约权威身份再查询窗口。
   每个精确 witness 前后、reservation 各成员之间及最终绑定都复验。取消只 fence 原选择
   代次及它的 preparation，不撤销 newer picker 或已经 commit 的 grant。失效回包不继续读取
   下一成员。选择代次溢出按终止处理，不饱和复用原代次。
4. `task_scope` 使用现有有界 OwnedTasks 保留原工作句柄。Tauri/ACP waiter 被取消时立即
   fence；原 blocking query 返回后才完成原 join。进程退出和 normal CLI probe 的原工作
   join 也接入 scope workers，不丢弃 native query 的原句柄。
5. 生产 `HostOwnedAdapter` 原来没有转发新 `scope_identity` / `scope_target_info`，底层
   Windows/Fake 测试通过仍不能说明 App 包装器可用。本次转发到原 inner，无枚举/liveness
   fallback；保留原 executor generation 与 native witness 检查。
6. Desktop descriptor 与 driver 准备拆开。明确选择只注册同一稳定 HostOwnedAdapter Arc，
   不启动 driver/browser/profile、不获取截图或输入权限；已有首用 preference/publication
   事务通过后才走 selected-surface 执行准备。legacy 显式 Host runtime 保留 driver 准备；
   probe-only 的独立 platform_adapter factory 改为对应 feature 下编译，未 suppress warning。

聊天请求卡/领域状态订阅/用户真正 picker 手势尚未接入；这批是后端合同和接线，不能说
用户已经能在聊天里完成批准。没有自动批准真实 App/CU/OS 权限，也没有读取凭据。

## 实际回归与失败记录

日志根：`tools/computer-use-probe/.run/codex-u01r2-20261003/`；本批前缀
`u04-app-selection-`。所有用户决定与 Fake witness 都明确 synthetic。

- `...owned-red-1.txt`：实际 0 pass / 1 fail、exit101，生产 HostOwnedAdapter 返回
  `exact task scope identity is unavailable for this backend`。加转发后由全量回归验证。
- `...app-build-1.jsonl/.stderr.txt`：exit101，两处新增测试的可见性/trait import 错误；
  使用已有 OwnedTasks 类型并补 trait 导入修复，不改生产权限边界。
- `...app-clippy-1.txt`：exit101，default feature 下独立 factory dead_code，以及测试中
  parking mutex 跨 await 的 lint。factory 限定到实际 probe callers，测试锁改 lexical block。
- `...app-build-2/3/4` 与 `...scope-tests-1` / `...acp-tests-1` 属各自中间源码；
  不当作最终源码验收。最终源下又构建 `...app-build-5` 并执行新旧业务测试。

新增 Core 7 项：真实包装器转发/无 fallback，查询前取消，查询中取消不读后续成员，旧
picker 取消不影响 fresh/committed，取消已预约批准并可重试，相同范围重试与变化拒绝，
reservation proof 中取消不读下一成员。新增 App 10 项：三处首用状态、零预批准效果、
原 worker join、启动前取消、DTO 严格解析、Stop/feature-off/transport retirement、foreign
owner/request/revision、双点击、原 ACP 锁外查询/重连迟到拒绝、真实共用异步协调入口的
幂等提交及取消。取消测试不手工补 fence 掩盖生产 guard 行为。

最终默认 App harness 使用 invocation 唯一 artifact，等构建实际结束后才用
`windows-test-manifest.xml` / mt.exe 嵌入 manifest。features=[]；不是 probe feature 或
真人 App 安装包。最终构建 `...app-build-5.jsonl/.stderr.txt` exit0：

| 检查 | 结果 |
| --- | --- |
| 新 task_scope / ACP task_approval | `...scope-tests-2.txt` / `...acp-tests-2.txt` 各 5 pass，0 failed/ignored，exit0 |
| App CU / ACP / routing | `...app-computer_use-2.txt` 189、`...app-acp_client-2.txt` 135（skip live_handshake）、`...app-routing-2.txt` 38，四 threads，0 failed/ignored，exit0；filters 重叠不可加总 |
| Core 全量 | `...core-full-2.txt` 700 unit + 220 integration = 920 pass，0 failed/ignored，exit0 |
| 格式与质量 | `...fmt-1.txt`、`...diff-1.txt` exit0；`...quality-2.txt` final PASS |

最终源子集沿原算法（git cached + nonignored untracked；Core/src、App/src、cu-probe、MCP、
consent-fixture；Sort-Object -Unique；每行 lowercase SHA256 + 两空格 + repo path；LF 终行）：
**657 文件**，SHA256 `9863ad22250ac57c7e684d7dd374267c72f45ffac70e74d0e8007283f48e5c31`。
这不是全部产品/资源的发布清单。
默认 App harness `target/debug/deps/grok_app_lib-dc611c8f8bc4f678.exe` manifest 后 SHA256：
`6206b90ab8fcd764c98f6f8df5bd613ba6e8b0446ae23f34b89f585262a710e1`。

最终补充验证（上述源码 hash 后只改文档）：

| 检查 | 实际结果 |
| --- | --- |
| 最终 Core / App / probe strict Clippy | `...core-clippy-3.txt`、`...app-clippy-3.txt`、`...probe-clippy-3.txt`，offline/all-targets/-D warnings，均 exit0；Core/test-support，App 默认 features |
| 最终 probe build | `...probe-build-1.jsonl/.stderr.txt` exit0；实际完成后取 invocation 唯一 cu_probe artifact 再嵌 manifest |
| Windows native | `...native-1.txt/.stderr.txt`，30 PASS / 0 FAIL、exit0、stderr 空；本批两项组检查改为 App 同型 HostOwnedAdapter 包装器，真实隐藏根准确拒绝及 inactive 原 HWND stamp 退休拒绝，无所选/范围外 caption 读取或输入，原资源由 guard 收尾 |
| normal CLI | `...cli-1.jsonl/.stderr.txt` exit0 / oracle pass；未批 observe 拒绝一次，批准后 fresh observe 两次、零 actions；普通确认两次及固定效果两次，duplicate NotStarted、ledger empty；原 CLI exit0/treeKill=false，原 Host workers joined，stderr 空 |

最终 probe `target/debug/cu_probe.exe` manifest 后 SHA256：
`47df4e9982c2c98896415f937f69f76c1c84118e10213d71c8096c9c539755bd`。
normal CLI evidence 为新的直接子目录 `codex-u04-app-selection-cli-20261003-attempt1`，
CLI 1.0.46 / grok-4.7 / low / default-ask，leader/subagents off，无 always-approve。
这是生产 ACP/MCP + synthetic Host/FakeAdapter 单目标兼容检查，不是组/native/App 真人
批准。Windows 两项组证据仍是非交互 station 的真实负向检查，不是 visible native
commit、五次执行切换或 App/ACP/native 联合手势验收。没有借其它桌面或绕过生产 gate。
原 CLI PID46044、普通 fixture PID15080 已由原 owner wait/join，随后只读回查均不存在。
所有本批原测试/构建/Clippy/CLI 工具会话已终态并收集，没有未收的原工作句柄。

## 下一步与未交付项

先把 U03 聊天请求卡、Host pending/readback、明确 picker 候选与决定、取消/Stop 接入本批
冻结的 command；前端同领域状态源与 15 locale、简洁布局、键盘和窄栏验收一起完成。
复查发现当前 `ComputerTaskCardLive` 仍只轮询 legacy status/run，Core 的 latest request
model status 也不是已存在的 typed UI event/readback API；U03 文档提到的完整共享 Host
事件源尚不能直接调用，必须补真实实现，不能猜一个事件名字或把 model response 当执行状态。
显式打开 picker 才读取本地候选名称；打开卡不能 ambient discovery/capture，也不能把
所有候选发送模型。请求仍没有 legacy run 时也必须可取消/停止。

U04 尚欠可见原生窗口整组 commit/五次切换/fresh capture/input/真正 App 手势纵向链，
其它 OS 的精确 witness、关联 dialog、browser context/tab/view/document 合同、跨 surface
子 binding、原子增量及即时缩小。私有非交互 station 只能准确拒绝，不能为测试切 WinSta0
或绕过 visibility/secure-desktop gate。
U05–U09 / D00–D31 全部保留；App shutdown 同步 native cleanup 与 feature_lifecycle orphan
blocking worker join 的遗留项本批没有解决。全平台/安装签名/真人授权/Grok E4/同一最终
候选长稳仍未验，不能标记整体可交付。
