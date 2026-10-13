# ExistingTab：原首次观察断连诊断与输入拒绝补齐

本合同的诊断在source49执行：原观察复测1通过，27.58s，仅non-reproduction，
source48根因仍OPEN。source50补十二项直接schema-valid input，focused12通过；
完整native36通过/1项startup首败（0checks），exit101，整套未绿。
完整结果及清理以[原owner输入检查点](2026-10-04-computer-use-task-consent-u04-existing-owner-input-checkpoint.md)为准。
最新source52及五项原矩阵首败见[启动检查点](2026-10-04-computer-use-task-consent-u04-existing-bootstrap-checkpoint.md)：
dormant1/browser focused3通过，完整native33通过/5失败、exit101；原owners已退出，精准清理完成。
下一项[原control诊断与Cancel/Stop/Drop](2026-10-05-computer-use-task-consent-u04-existing-control-terminals-next.md)。
以下为执行前的原合同，不能把其中“尚未直接调用input”视为当前状态或重复开工。
单Codex writer，不启动Grok/子代理、不提交发布，全部dirty/untracked/fix.patch与首败保留。

## 先查首次观察，不改正常恢复或退休规则

原失败场景为 `active-connection-repair`。已经完成首次 MCP、App 确认、真实
Pair/Share、原 TaskGrant publication 与未选 C 的拒绝，共六项 checks；在
`original-grant-fresh-observation-no-input` 断言失败，尚未进入 repair-challenge。
startup complete，原 worker 未关闭，replacement not-started；失败诊断看到
unpaired 和一次成功的 extension-disconnect。`detail=share-host-ready` 是上一
阶段残留值，不能拿它判断 Share timeout。现有证据尚不能区分 MCP error 与
正文断言失败，也不能确定哪个生产路径退休了连接。

1. 先回读 source48 原 native 日志及精确浏览器根的 curated joint-failure.json，
   核对 hash；不读取 Cookie、Token、配对记录或任意 browser storage。
2. 只补私有诊断：在这次原 observe 调用前刷新固定 detail，分别标明
   observe-request、observe-response、expected-text 和 document-identity。
   原 reply 只提取 isError、合法固定 outcome 分类、是否有 text/image 及预期
   fixture 文本存在与否；不保存原 response、身份、错误 text/stack 或页面内容。
3. 失败时读取现有 trusted-worker `readConnectionDiagnostics()` 的固定 route /
   reason / HTTP status 有限列表。它已在生产模块内存中记录 retire_status /
   retire_share / retire_observation / retire_action / retire_transport；复用现有
   allowlist，不新建模型诊断工具、不对任意 URL/异常做原样转储。
4. source/artifact 从49起，确认没有另一 writer、证据名不存在。保留 source47
   compile 首败与 source48 runtime 首败；源变化后重新冻结并构建。最多先跑
   一次带新诊断的原失败场景，不重复成功场景刷绿。未复现只记 non-reproduction，
   未取得原反例原因前不称根因修复。
5. 有明确原因后才改对应生产路径；保留连接身份复验、原5秒请求/30秒lease、
   timeout/unknown 不重放和 failed/ambiguous 退休规则。不得用 sleep/retry、
   扩大错误允许集合、提高预算或吞错规避首败。

原失败根须一直保留：

- App/store：`C:/Users/Administrator/AppData/Local/Temp/grok-cu-managed-task-0f988835-c63b-43e1-8cc2-e6507f8250e4`。
- Browser：`C:/Users/Administrator/AppData/Local/Temp/grok-cu-pairing-owned-025e2dcd-e4c0-4890-ad01-769372d85c01`。
- 原 curated JSON SHA256：`90c7450804af574f9296782639be36615b72ad9cb340e376eefdd67bb81e3844`。

## 补直接 MCP input 拒绝，不能把独立 DOM 零效果冒称调用证明

当前 owner negative 行实际调用 read/screenshot/list，并独立检查 A/B/C DOM、
script 与 capture 零效果；尚未直接发出 schema-valid computer_act input。
需对 choose/reservation/publication 的十二项固定 owner 行补此缺口，不新增授权。

使用 schema 合法的 type_text 或 set_value、parameters.text 和完整的
version/actionId/runId/targetId/targetGeneration/snapshotId/geometryRevision/
elementRef 字段。选择前尚无模型观察，不能偷偷 observe 或制造有效授权/ref；
固定合成身份只用于证明无 authority 的请求在派发前被拒绝。记录拒绝发生于
权限而非缺字段/解析错误，断言零新增 page script/input/capture 和 A/B/C DOM
保持原值。调用后原 feature/store=false、零 grant/continuation、原 Pair/document/
三个 Share 保留；有原 run 的行仍实际等待 Stopped，零 run 行不得伪造见证。

每个新断言必须能区分“无权限却派发”的反例，不写仅镜像实现的测试；保留各
切点原准确错误和实际 join，不把原 process/agent slot 合成替换冒称 OS 死亡。

## 验证、清理及后续顺序

诊断或断言源改动后使用新候选。先原反例有限诊断和受影响 focused，再跑同
候选完整 native 矩阵、App CU/ACP/routing、两种 strict Clippy、Node/MCP 与
fmt/quality/diff。Core/前端未改时明确未全量重跑，不搬用历史总数。真实 App /
真人 toolbar / Grok E2E 未运行的门槛仍 not_run。

原 build/test/browser owners 实际退出后，按 AGENTS 第9条生成新清单、回读所有
精确 target/retained paths 和 preserved hashes，再执行。保留当前可用构建、
依赖、不可变 seed、源码和全部失败证据；不复用旧清单或删除整个 target /
node_modules。分别记录逻辑删除量和磁盘空闲实际变化，未知 owner/文件 deferred。

然后继续原计划中的 Cancel/Stop/Drop、原 worker 物理 join、ACP stdin 前的 queued
continuation 撤销，以及恢复并发和迟到正文/截图/input。正常恢复/lease/re-Pair
已验部分不重新开发。完整 E5、U04–U09、D00–D31、四 surface、Windows/macOS/
Linux、双 Mac 架构、全部输入/恢复、签名安装升级回滚、真人 Grok 和最终同候选
12h 范围不缩减。整体仍 active / partial — not releasable。
