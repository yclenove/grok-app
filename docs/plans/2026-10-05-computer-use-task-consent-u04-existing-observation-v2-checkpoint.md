# ExistingTab：观察 v2 整链与迟到截图修复

当前 source96 / artifact96，整体仍 **active / partial — not releasable**。
单 Codex writer；未启动 Grok / 子代理，未 commit / push / PR / 发布。
ExistingTab 模型 descriptor 继续关闭，raw parser 仍 Desktop-only，全部 dirty / untracked / fix.patch 保留。

## 候选身份

证据根：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`，下称 E。

| 项目 | 实际身份 |
| --- | --- |
| 工作区 / 分支 | `H:/aicoding/grok-app-computer-use` / `feat/computer-use-implementation` |
| HEAD | `8d8b71c262ab222748bca32412eea884d4b892e0` |
| source96 | 3316 files；`E/source-files-96.sha256` |
| manifest SHA256 | `1661be0046b1bf410391a2797791b140461475b77ac73f3f4fa9dbfe16b2d9e6` |
| 默认 features App test artifact96 | `src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe` |
| App SHA256 | `85b1426ac84658c51125bca46d1a309f37019839217bc81ca74480eda916a355` |
| 生产扩展 bundle | 48 件；SHA256 `65c8d8e73a850cffd6f21fef6da0a56d5c14ff14490b65e2db40015db0847946` |

build96 是 `cargo test -p grok-app --lib --no-run`，实际 exit0，并完成 Windows manifest 嵌入；
这是 App 测试 EXE，不是可安装发行包。source95→96 只有外部 probe
`existing-task-capture-success-live.mjs` 的 MCP 结果字段断言修正（`status`→实际 `kind`）。
已比较两个源 manifest；Rust / Core / 生产扩展均相同。Cargo 复用同一 EXE/hash，不能声称重新编译了 Rust。
下面标注 source95 的门禁是该生产源码的实际结果，不伪造为 source96 的新执行。

## 已实现的生产链

在 [source89 Host / IPC 基础](2026-10-05-computer-use-task-consent-u04-existing-observation-host-checkpoint.md)
之上接好 observation v2 negotiation、严格 request / proof / purpose / document / connection binding、
原每请求 scope / status watcher / signal / physical settle，以及独立持久 observation journal / recovery。
v1、缺协商字段、错用途或旧 epoch 明确拒绝；cleanup 不依赖当前 Pair Bearer / feature。

原 native read、capture、normalize / decoder、watcher 和持久 cleanup 共同归属原 owner。
deadline / reset / Pause / Stop 退休结果并请求取消，不能 race 掉原 Promise 而提前归还占用。
两个 journal 共用全局 8 slot 与同 tab 互斥；持久删除失败保留占用，只重试 cleanup，不重读或重放输入。
restored executing 不借 DOM 动作回执证明截图结束，直到可信原 browser cleanup 才释放未知占用。
journal 不存图片、正文、command、Pair Bearer 或 action nonce；扩展权限、key、minimumChrome、seed 和默认开关未改变。

source93 实测仍有竞争：250ms watcher 尚未看到取消，原 PNG 返回后会 decode1 / 尝试提交。
source95 在 native 返回后、decode 前，以及最终正文 / refs 返回前重新查询原 scope。
若已有在途 status，先 join 它，再启动新查询，不能借启动于 native 返回前的旧回复。
失效立即 abort，仍等待原操作和 status / settle 收尾。新增真实类集成单元覆盖轮询间取消与 stale 在途回复，decoder 均为0。

## 已完成验证

| 检查 | 实际结果 / 原证据 |
| --- | --- |
| source95 Node 扩展 | 210 passed；`E/node-95.txt` |
| source95 Core default / test-support | 1097 / 1113 passed；`E/observation-core-{default,support}-95.txt` |
| source95 四 strict | Core / support / App / probe 均 exit0；`E/clippy-*-95.txt` |
| source95 固定扩展 / MCP | 7 / 38 passed；`E/observation-gates-95.json` |
| source95 诊断 / changed-probe syntax / fmt | 诊断8 passed、syntax 和 fmt exit0；`E/observation-diagnostics-95.txt` |
| 原 Pause / Chat Stop 迟到 PNG | source95 两独立 CFT154 场景 2 passed，40.04s；`E/existing-late-capture-focused-95-1.{txt,json}` |
| 原截图权限拒绝 | source95 1 passed，16.91s；`E/observation-capture-denial-95.txt` |
| 正常模型截图 / App preview / 原模型 ref 点击 | source96 1 passed，19.43s；`E/observation-capture-success-96.txt` |
| 原 App embedded / export bundle | source96 12 passed，9.53s，48 件；`E/observation-bundle-96.txt` |
| source96 宽 CU / ACP / routing | 295 passed / 82 ignored；178 passed；38 passed，全部 exit0 |
| source96 quality / diff | exit0；`E/quality-96.txt`、`E/diff-96.txt` |
| source96 完整 native ExistingJoint | 原 session47034 已 exit0；81 passed /0 failed /0 ignored，1643.75s；`E/existing-late-actions-rest-existing-joint-96-1.txt` |
| source96 最终源复验 | 3316 files / manifest96不变；`E/existing-late-actions-rest-96-1.json` |

定点迟到截图：held 时 Host idle=false，native capture1 / settled1；释放后 decoder0，
signalAborted=true，原 read / status / settle / observer / owner 均 join；模型零旧 image / text / refs 成功。
Pause 原 owners 结束后显式一次 Continue，新 generation 与一次 ACP 续行；Chat Stop 真正 Stopped。
Pair、三 Share、原 document 保留，B/C 零访问。原 PNG764×429 /9375bytes，SHA256
`669a73566d78915664eba22439cb7dc37b8f21a0264aaafd032e6b47cec736b3`。

正常截图：原 scope / read / status / settle 全部 join 后模型收到图片；preview 用独立 snapshot，
原模型 ref 仍可真实点击 A 一次，B/C 零访问。输出 PNG764×429 /16146bytes，SHA256
`34aae30341089f2fc712bc0253890bc4a49d7f71c1128a604e9c8ddc521298ef`。
read3 / input1 / capture2 / decode2 / settled2 / result3 / pair1 / disconnect0。
权限拒绝独立 capture1 / denial1 / settled1 / decode0，没有自动正文降级，显式正文请求成功。

这些是实际 App/MCP/认证 HTTP/source MV3/CFT154 PNG 联测，但 Host 手势 / session / ACP peer
仍 synthetic，浏览器 action 由 owned CDP 驱动。没有真人工具栏、installed App 或真人 Grok 验收证据。
本批未改前端，未重跑前端全量；不能借 source79/80 的旧完整 native 数字。
质量预算仍 App+Workbench13573、≥1000行文件80；主 fixtures997 /985，不放宽。

## 首败与尚未证明的边界

- source82 /93 两轮原迟到 PNG 首败 log、profile、PNG 和 manifest/artifact hash 全部保留；95 定点修复有效。
- source95 正常截图首败来自新 probe 错读 MCP `status` 字段；96 改为真实 `kind`，未放松原业务断言。
  `E/capture-success-first-failure-95.json` 保留原 log 和5个 profile/PNG hash。
- Node94 新断言误写 `bitmap.counts.decoded`，95 改为 decoder 的真实 `bitmap.decoded`；`E/node-94.txt` 保留。
- observation-unit91 的新 fixture 漏 action 依赖，已补；90/91 的失败日志继续保留。
- Pause held 阶段原 Host idle=false，但 task row `cleanupPending=false`；Chat Stop 为true。
  原 Continue 后端 `await_original_idle` 仍等 adapter / run / browser 真正 idle 后才准备新观察，
  不能据此声称已验证 UI 对慢收尾的完整表达；后续单独覆盖 Continue 在释放前点击的状态与收尾。
- 其它历史配对 / B切换 / startup / cleanup 首败没有因为本批通过就自动销项。

## 例行清理与空间

用户要求以后每批清理无用构建。已固化于 AGENTS 第9条：大构建前查空间，原 owners 退出后
Plan→回读精确绝对 targets / 最新代 / hashes→Execute→复验；不杀无关进程、不删除整个 target。

source96 定点 / build / Core / strict / Node owners 已终态后，执行
`E/cleanup-batch-cache.ps1 -Name observation-extension-cache-96-1 -SourceAttempt 96 -ArtifactAttempt 96 -Execute`。
已先检查原 PLAN，删除6个完整同 variant 旧 generation（绝对清单见
`E/observation-extension-cache-96-1-cleanup.json`）。

逻辑删除 **5095069647 bytes（4.75 GiB）**；H freeBefore300682113024 / freeAfter304151756800，
空闲实际观察增加 **3469643776 bytes（3.23 GiB）**。两者分别报告；不把并发其他任务释放空间计入本批。
删除未进回收站，Cargo 重新编译可生成这些增量缓存。
source3316、App96、两 Core EXE、各 variant 最新 cache、CFT140/154、immutable seed、
依赖、dirty/untracked/fix.patch 和失败现场全部保留；脚本复验 source/current/preserved hash 一致。
working / 跨 variant / 归属不明 runtime / 失败目录延期，未终止无关 owner。

清后另实际复验：55–62的37hash/5原 absences；64–66的3log/12hash；
67/68的2log/6hash；71/72/74的3log/9hash；75–77的3log/9hash；
82/93的2log/20个 profile/PNG hash；95 success首败的1log/5hash，均未改变。
宽回归随后才启动，全部原 owners 退出后再按同一规则检查它的新增 disposable 产物。

原 session47034 现已实际终态：宽 CU295/82ignored、ACP178/routing38、完整 native81/0，
全部 exit0；最终 Verify3316 / manifest96不变。后续只读检查原6个删除目录仍不存在，
最新 cache533个文件和6个保留产物的hash一致；当前相关owner0、Cargo/rustc owner0。
没有新增的完整 old/current pair，追加删除0，不虚报额外回收量。
`grok_app_lib-28mpg1j4ghpco/s-hmvys28aps-0y4sbl2-working` 是旧失败留下的不完整代，
与它的前一代继续保留，不能把 working 当成当前可用代而删除旧代。
结果见 `E/observation-native-cache-inspection-96-1.json`；没有停止任何无关进程。

## 下一步

具体执行顺序见 [v2后续开发](2026-10-05-computer-use-task-consent-u04-existing-observation-v2-next.md)。

1. source96 原宽回归 /81项 native 矩阵、最终 Verify 与收尾检查已完成，勿重复运行。
   原 owner / phase / profile / hash及历史首败保留；不要为未改的代码继续增加重复门禁。
2. 补原 held PNG 下用户在释放前点击一次 Continue 的实际 App 状态：等待 / Stop可达 / 新观察与输入0；
   释放后只恢复一次。分清 task row 的 cleanup 表达与真实 scope occupancy；必要生产修改用 fresh97+。
3. 回到 U04/U05/D10–D11 的 installed Chrome/Edge、真实工具栏手势 / App批准 / Grok纵向链，
   以及未完成 journal worker恢复/Host退出相关原生证据，不能只扩大 synthetic fixture 数量替代。
4. 完整 E5、U04–U09/D00–D31、四 surface、Windows/macOS/Linux、两 Mac 架构、native输入恢复、
   签名安装/升级/回滚、真人 Grok、12h 与 UI/UX范围保持；具体剩余表以原 remaining-plan 为准。
