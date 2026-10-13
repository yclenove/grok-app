# U04：ManagedBrowser 原 worker/context/page 身份基础

状态：**identity foundation implemented / production task-scope producer pending — not releasable**。
整体目标仍 active / partial；完整 U00–U09 / D00–D31、三平台和四类 surface 不缩减。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer，原 dirty/untracked
和 `fix.patch` 保留；没有 commit/push/PR/发布、真实批准或代理/凭据/日常浏览器修改。

本批接续 [U05 UI checkpoint](2026-10-04-computer-use-task-consent-u05-ui-checkpoint.md)，
先实现 ManagedBrowser 生产身份合同；**没有解开 Desktop-only task UI/批准 gate**。

## 实现

1. Node worker Host-only `/scope-identity` 必须通过 bearer、Host header 和原 owner/runRevision
   admission；只查询已存在、已发布、仍属于原 BrowserContext 的精确 page。
   不创建 run/profile/page，不执行输入，不读取 URL/title/ARIA/正文/截图/Cookie。
2. 返回严格六字段：ok/owner/profile/workerInstanceId/contextInstanceId/pageId。
   worker 和实际 BrowserContext 各有 canonical v4 UUID；普通导航不改变 tab 实例证明，
   但既有文档 generation/ref/snapshot 权限仍单独退休。暂停、Stop、关闭或失效页面拒绝。
   同名 profile 重建不能继承 context 身份。
3. Rust 严格 typed parser 拒绝缺失、额外、正文、无界/非法 segment 以及非 canonical v4 UUID。
   `NativeScopeIdentity::ManagedPage` 完整携带原 owner/profile/worker/context/page 身份。
4. Host 查询 pin 原 managed worker Arc 和原 run revision，锁外工作，返回后复验原
   session/run/profile/tab/generation/fence。ManagedBrowserAdapter 转发真实身份；另一 run
   的 metadata 请求拒绝，不靠 profile 名称/标题/PID/worker 在线替代实例证明。
5. App LoopbackPlaywrightWorker 的接口 typed、可取消且必须绑定原 admission；旧 worker 或
   未绑定 client 明确 unavailable，无 legacy/synthetic fallback。
6. runtime prepare 的真实 `WORKER_FILES` 包含 `scope-identity.mjs`，实际源码打包及 hash
   列表回归通过。旧 immutable seed 不替换；新能力的 source-worker 与打包测试分别留证。

## 最终验证

证据目录 `tools/computer-use-probe/.run/codex-u04-managed-scope-20261004/`。
以下最终结果均在 `source-files-3.sha256` 冻结的产品源码上完成，文档和 ignored runner 不在清单内。

| 项目 | 实际结果 |
| --- | --- |
| 实际 Node/Chromium HTTP | `worker-native-full-1.txt` 2/2，含原 observation contract；非 mock worker |
| Node 定向六文件 | `worker-unit-1.txt` 51/51，0 skipped/cancelled |
| Core scope 定向 | 9/9，含8新增及1已有 owned 转发 |
| 默认 feature Core | `core-full-3.txt` 983 = 779 unit + 204 integration，0 failed/ignored/filtered |
| test-support Core | `core-test-support-1.txt` 1015 = 779 unit + 16 driver + 220 contracts，0 failed/ignored/filtered |
| 默认 App 构建 | `app-build-4.jsonl` build-finished success，真实 artifact features=[] |
| 默认 App 实际 native scope | `app-native-scope-2.txt` 2/2，source Node/Chromium → Rust client/Host/adapter |
| 默认 App CU / ACP / routing | `app-computer-use-2.txt` 214、`app-acp-2.txt` 158、`app-routing-2.txt` 38；集合重叠不相加，live_handshake 排除 |
| 前端 CU/API/hooks/i18n | `frontend-cu-2.txt` 33文件333项通过；不是全 frontend suite |
| 类型/lint/UI build/MCP golden | exit0；UI既有large-chunk warning保留 |
| strict Clippy | Core all-targets+test-support、默认 App all-targets、probe all-targets均exit0，无新豁免 |
| 最终 fmt / quality / diff | `fmt-final-1.txt`、`quality-final-1.txt`、`diff-final-1.txt` exit0 |

实际 native scope 用独立 profile，无 Broker execution run、执行 grant、真实任务批准或输入。
证明原 worker/context/page、不同tab、另一run拒绝、pause/stop fence及原 supervisor shutdown/join。
成功后只删自己新建的 profile/runtime。不是 installed App、ACP批准链、真人手势或其它平台 native验收。

## 失败与夹具修复（原日志保留）

- `worker-red-1.txt`：旧端点实际404而非200；新增身份端点后转绿。
- `worker-green-1.txt`：测试误以为整 context 的 `/close` 已同步完成；改验真实
  pause/resume、Host clear及同名重开。失败日志保留，不修改产品去伪造同步关闭。
- `app-build-1` 测试调用 private run_exists 导致编译失败；改用公共 require_owner 的
  RunNotFound验证后最终默认 App编译/实际链通过。
- `core-full-1` 778 pass/1 fail：旧 missing-sibling夹具先命中 immutable seed 的旧worker hash。
  refresh helper第一版遗漏 detach根 manifest硬链接，意外改动了共享种子的清单元数据。
  精确原owned Cargo/test停止后，仅修复清单为归档证实的原2031 bytes，未改worker/module本体。
  SHA256恢复为 `001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec`。
  helper现在也 detach manifest，并断言原清单与每个原module逐字节前后不变。
  定向 `core-seed-isolation-2.txt`及两种feature完整suite通过；详见`seed-restoration.md`。
  三个历史隔离clone和该事故证据未删除，不以恢复后绿隐去事故。
- `core-clippy-1`要求 unwrap_or_default，已修，无抑制。
- App acceptance attempt1 的 --nocapture 在 Windows PowerShell将预期stderr当作
  NativeCommandError；native2项绿，但广选CU无终态。attempt1不算通过/产品失败，
  修runner后 attempt2四集合实际终态均exit0。
- 本轮 probe-clippy-1 用错package ID，Cargo101未进入检查；按实际Cargo.toml的
  grok-computer-use-probe运行 `probe-clippy-2.txt` strict all-targets通过。

## 最终身份与主动清理

3059 source files，`source-files-3.sha256` manifest SHA256
`09370b2e063253884bce8d63cee1e782e3526282114886367885d923352d10b9`。
清理后 `source-verify-after-cleanup-1.txt` 3059/3059 unchanged。
默认 App harness记录 `app-artifact-2.json`，SHA256
`7d215a7c35c4976ff6c928c8c471159ae61251bc54f4f6efb5d02538c44cf1ab`。
只对测试harness嵌CI同款Windows manifest，未改产品linker；同路径旧hash属于历史构建。

四个续接build/test原exec均exit0，补充Core/App/probe原exec也exit0。
确认原owner退出后，按明确old/current pair清五代旧incremental session目录；没有删整crate
leaf/target/node_modules、immutable seed、当前包、源码、失败记录、日志/hash/report。
`cleanup-obsolete-generations.ps1`先inspect再Execute，拒绝reparse/live owner/目录代次变化，
清理前后核最新代所有文件及App/Core/seed hashes不变，精确target见`cache-cleanup.json`。
删除逻辑体积 **4,324,483,970 bytes（4.03GiB）**；H盘实际空闲观察增加
**2,791,989,248 bytes（2.60GiB）**。硬链接未重复计作物理释放；与前批清理分开记。
删除缓存不在回收站，可用 Cargo重建。用户要求的每批收尾规则已在AGENTS第9条。

## 下批入口

见 [Managed task资源准备方案](2026-10-04-computer-use-task-consent-u04-managed-producer-next.md)。
当前 App TaskScopeTarget::parse 和 begin_task_scope_picker仍 Desktop-only；scope端点可用
不等于 managed任务范围已支持。先解决“Bound要求真实tab，而owned tab需要原reservation
run”的先后依赖，再接真实生产批准、最终原ACP/session原子pin和共享UI。
其它surface、关联dialog、增量/即时缩小、恢复、三平台真实App/native/normalGrok、全CI、
签名安装/12h和完整UI/UX仍待完成，不据此宣称整体可交付。
