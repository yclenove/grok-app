# U04：原 reservation 下的 ManagedBrowser 资源准备

状态：**Core resource constructor + default App/ACP native publication verified；生产选择器/UI 尚未接入 — not releasable**。
目标 active / partial，完整 U00–U09 / D00–D31、四 surface、三平台和完整 UI/UX 不缩减。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer；全部原 dirty/untracked/
`fix.patch` 保留。未 commit/push/PR/发布，不启动 Grok/子代理，不改凭据、代理、共享 ~/.grok
或日常浏览器，不自动批准真实系统权限。

## 实现边界

沿[方案](2026-10-04-computer-use-task-consent-u04-managed-producer-next.md)，拆开 Resources 与 Bound：

1. Host-only `reserve_managed_access_approval` 不接受 caller/model profile 或 run。原 reservation
   冻结唯一 run、随机 task profile、selection revision；重复 Preparing 手势拒绝，不重复 spawn。
   资源票据不满足 commit/publication/execution；Desktop 的真实 Bound 门槛不放宽。
2. 原 reservation 锁内插入 Pending run、记录 opened_run，然后锁外打开独立 blank tab、查询真实
   worker/context/page proof。原资源 run 从未先成为 Legacy，proof owner/profile 必须与原票据相同。
   返回后检查 generation、Stop、paused、reservation/feature/deadline，再由同一票据转 Bound。
3. `authorize_prepared_managed` 不走 legacy profile provision、页面发现或 ownership 转移。
   复用已完成的锁外 native prepare、原 App/session/ACP/RequestTransport 短原子 publication。
4. Stop、取消、feature-off、重连、agent replacement、expiry 的迟到结果不产生 grant/continuation。
   Resources 阶段的 expiry 也交付原 Pending owner 的 cleanup；满队列不能遗忘仍需收尾的 run。
5. 没有已显示并实现的 task new-tab/popup policy，因此 Bound 任务拒绝旧 NewTab/Popup 扩范围。
   旧 browser inventory/open 只返回全部已批准成员；旧 observe/navigate 在 native 工作前拒绝
   未批准 tab，返回前复验 authority。Legacy 正向合同由全量回归保持，不全局禁用浏览器工具。

Core 新回归 13 条，包含 open/proof × 五种终态，以及 reserved/bound/publication × 七种终态。
worker slot replacement 后，原任务的 query/capture/cancel 仍落在原 Arc；replacement 计数为0。
失败 retry 使用全新 run/profile；missing worker/executor、foreign transport、同名 context replacement
均覆盖。Recording 决定/证明明确 synthetic，不替代下面的真实 native 证据。

## 当前验证与冻结

证据目录 `tools/computer-use-probe/.run/codex-u04-managed-producer-20261004/`。

| 项目 | 原始证据与结果 |
| --- | --- |
| Core 默认 | `core-default-2.txt`：1002 = 798 unit + 204 integration，0 failed/ignored/filtered |
| Core test-support | `core-full-2.txt`：1018 = 798 unit + 16 driver + 204 contracts，0 failed/ignored/filtered |
| 默认 App build | `app-build-3.jsonl` build-finished success，artifact features=[] |
| 原 App/ACP publication | `app-publication-2.txt`：6/6；含真实 source Node/Chromium 原 run 与 session/transport replacement |
| Task approval / recovery | `app-task-access-2.txt` 8/8；`app-task-recovery-2.txt` 4/4 |
| Native managed constructor / scope | `app-native-managed-2.txt` 1/1；`app-native-scope-2.txt` 2/2 |
| 默认 App CU / ACP / routing | `app-computer-use-2.txt` 222；`app-acp-2.txt` 158；`app-routing-2.txt` 38；集合重叠不相加，live_handshake 排除 |
| 全前端 | `frontend-full-2.txt`：701 文件、7999 tests passed |
| Browser/MCP/probe Node | `node-contracts-3.txt`：37文件、296 passed/0 failed/ignored，开发Node24、明确的独立Chromium副本、串行 |
| Strict Clippy | Core all-targets+test-support、默认 App all-targets、probe all-targets：各 exit0 |
| fmt / type / lint / quality / UI build | 最终各 exit0；已有 large-chunk warning 保留 |

App acceptance attempt2 原 exec48961 已 exit0。native fixture 使用隔离 profile、实际 source
Node/Chromium 和默认 App Rust client/Core；Host 决定、ACP peer/session 为 synthetic，**不是
installed App、真人授权或真实 Grok**。成功路径和 replacement 路径均让原 supervisor shutdown/join。
产品 TaskScopeTarget、picker capability 和共享批准 UI **仍 Desktop-only**，没有开放 managed gate。

3069 source files；最终 `source-files-2.sha256` manifest SHA256：
`5eeab06930b4bc91e3391db750c96765128f3084c77328542ac883409498ab0c`。
默认 App `app-artifact-2.json` SHA256：
`399abfb879e3d28fa128e913221fa3e5e99aabbd9cf4502fd7d22586ce4ce880`。
只嵌 CI 同款 Windows test manifest，不改产品 linker。
immutable seed manifest 保持 `001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec`。
与上批3063文件逐项比较，变化均属于本批已列产品/测试文件，没有无关 cargo fmt 修改。

计数更正：上批 publication 文档的 test-support 1021 把 driver16 重复计入 contracts220 后又加了一次；
原日志实际为785+16+204=1005。此前原日志不改写。本批1002/1018从各终态 result 行重新求和。

## 失败证据保留

- `app-red-build-1/2` 是夹具编译错误；build3 后 `app-native-red-1.txt` 实际0/1失败，
  `schema: scope is pending`，原 exec48806 exit101。它是 constructor 业务先红，区别于编译失败。
- `core-boundary-red-2.txt` 4/7：旧 NewTab 越界、expiry 未 fence 原 Pending owner、inventory 泄露
  unapproved profile。`core-compatibility-red-2.txt` 0/1：旧 open 泄露未批准 tab metadata。
  修后最终13/13绿。更早双 panic `0xc0000409` 是夹具未先释放/join；已加无论断言成败都收尾。
- `core-boundary-green-3.txt` 是目标检查误插到没有 tab_id 的 generic capture，E0425，非测试绿；
  修到真实 legacy observe 的采集前边界。green5 的一条失败是夹具错把 Stop 的 cancel_run 当作
  clear_profile；按实际合同改断言原 cancel 1、replacement 0，green6 13/13。
- `app-build-1` 是新增夹具的 visibility/Debug 编译错误。build2 后 publication/CU attempt1 失败
  来自夹具在 async context drop blocking HTTP runtime；改为同步 native owner、独立保留 Tokio
  ACP runtime。build3/acceptance2 实际转绿，不改产品 client 生命周期来迎合测试。
- frontend-full1 命令数组错误、rust-checks attempt1 把 Windows PowerShell native stderr progress
  当异常，均为 runner 错误；保留原记录，后续按原进程 exitCode 判定。
- Node contracts attempt1 缺显式隔离浏览器且与大构建并行，270 pass/18 fail，不作为最终绿。
  attempt2 使用独立 Chromium 副本、串行、bundled Node20：277 pass/1 fail；唯一失败是开发
  WebView jsdom 依赖需要新 Node 的 require(ESM)，不是浏览器资源合同。开发 Node24 单独19/19绿。
  attempt3 的37文件完整串行复核实际296/296、原exec52477 exit0。bundled Node20 的277正向
  worker/native合同与开发Node24完整套件分开记录，不能把attempt2总exit1说成全绿。

## 清理与下一步

遵守 AGENTS 第9条。原 build/test/browser owners 全部终态后，先 inspect 再 Execute，精确删除
13代旧增量缓存与本批自己复制的 disposable node-runtime-1；没有删除整个target/node_modules、
其它工作区、历史失败root/seed事故clone、源码、包或证据，没有kill其它进程。

- `cache-cleanup.json`：13代逻辑6,499,607,075 bytes；H空闲观察增加5,127,716,864 bytes。
- `node-runtime-cleanup.json`：84文件、361,230,888逻辑bytes；H空闲增加361,385,984 bytes。
- 顺序执行、测量区间不重叠，本批合计逻辑6,860,837,963 bytes（6.39GiB），空闲观察增加
  5,489,102,848 bytes（5.11GiB）。硬链接不按逻辑体积重复算物理释放；不重复计上批3.01GiB。
- 原cleanup exec97783 exit0；runtime cleanup也exit0。最新各增量代全文件、当前App/Core和
  immutable seed前后hash不变，清理后3069/3069冻结源码逐项不变，diff check通过。
- inspection曾因新增preserve path后旧seed数组索引、PowerShell5 JSON array嵌套误判而拒绝。
  都发生在任何删除前；seed实际hash一直正确。修runner后重新完整inspect再删，不改seed迎合检查。

删除不在回收站，但全为可重建产物：Cargo重建旧缓存，测试Chromium副本由只读seed重新复制。
当前可用程序、最新缓存、immutable seed、依赖、源码/dirty和全部报告/失败日志/hash仍保留。

随后接 App managed Host reservation、owned runtime 首用与取消、原 ACP/session publication 和共享
picker/批准 UI/15locale，补挂载无准备、busy Stop、失败重试和唯一续行；不得把页面挂载变成授权。
继续全部其它surface、关联dialog、增量/缩小与U05其它恢复、U06–U09、D00–D31、三平台真实
App/native/normal Grok、完整CI、签名安装/12h与完整UI/UX。本批constructor不是这些验收的替代。
