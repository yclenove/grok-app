# U04：独立任务浏览器的完整 Host 地址与 App 批准入口

本批为接线增量；完整目标仍 **active / partial — not releasable**。
单 Codex writer，保留原 dirty/untracked/`fix.patch`；没有启动 Grok、子代理或自动化，未提交、push、PR、发布。

## 已实现的边界

- Core 新增 `reserve_host_managed_access_approval(token, key)`：完整 connection/task/request/revision 在原 Broker 锁内验证，再建立原资源 reservation。不是先 readback 后仅校验 request/revision，不接受 model/caller profile/run/target。
- 新的 ACP managed approval 模块保留原 `Arc<RequestTransport>`。metadata reservation 不启动 worker、profile、native 查询或输入；之后复用原 `task_access` owned worker 和原 App/session/ACP publication pin。
- SessionManager 的明确 managed 批准和 `computer_use_task_managed_approve` 命令已接通；仍拒绝 remote/unowned/replaced endpoint，命令复用主 WebView 检查。没有放宽 Desktop `TaskScopeTarget` 字符串入口，也没有伪造 managed picker candidate。
- 默认 App/native publication 的既有夹具已改为从这个 ACP start 入口取得真实资源票据；实际 source Node/Chromium → 默认 App/ACP pin → Core，而不是直接调用 Core producer。
- 修复 resources abort 的 readback 残留：仅原 managed selection revision 可以返回 Awaiting，并清除 selecting/bound。取消、waiter Drop、原 transport replacement 在 native 资源启动前不再把卡片永久留在 Preparing；新 retry 的 retired handle 不影响当前票据。
- 核实生产 `ensure_request_endpoint`/RequestTransport 共用 App Broker，现有 supervisor 第二次不重新 attach 在该路径不构成缺 executor 的缺陷；未根据独立 synthetic Broker 作推测性修改。跨独立 Broker 的基础设施复用不在本批开放。

## 保留的红色证据

证据目录：`tools/computer-use-probe/.run/codex-u04-managed-entry-20261004/`。

- `core-host-key-red-1.txt`：只传 request/revision 的初版完整地址入口接受 foreign connection，实际断言失败。
- `app-build-1`：新测试误用仅 Core 单测可见的 census，以及要求 Debug 的 unwrap_err；默认 App 实际编译失败。改用生产 Host phase readback，不扩大 production instrumentation。
- `app-managed-entry-1.txt`：实际 3 pass/3 fail，取消/Drop/replacement 后 Host 仍 Preparing；同一问题也在 App CU/ACP 集合出现。
- `core-retired-drop-red-1.txt`：初稿在原 approving owner 未退出时提前 retry，被生产 selectable_request 正确拒绝。它不是 stale Drop 漏洞的证据；修订成真实 abort 完成后的 fresh retry/retired handle 合同。
- `core-abort-state-red-1.txt`：实际 abort 完成后 Preparing ≠ Pending，准确复现状态残留。
- Rust attempt1 在新增红回归期间完成，fmt exit1；不冒称该轮完整全绿或最终冻结源。

## 最终验证与冻结

最终候选为 `app-build-3` 的默认 features=[] App harness；verification attempt2 已实际通过：

- Core 默认 1004 = 800 unit + 204 integration；test-support 1020 = 800 unit + 16 driver + 204 integration。集合包含既有回归，不宣称全为新增。
- 默认 App managed entry6、publication6、task approval8、recovery4、native managed1、native scope2、CU228、ACP164、routing38，均0 failed/ignored；集合重叠，不相加，排除既有 live_handshake。
- Core、默认 App、probe 三套 strict Clippy及 fmt 通过。
- 实际 source Node/Chromium 的 native publication 已经过新 ACP start 入口；Host/session/ACP peer 仍 synthetic，不冒称真人、installed App 或真实 Grok。
前端未改；本批已实际重跑全部 701 文件/7999 tests、typecheck、lint、UI build、quality、diff，全部通过。UI build 保留既有 chunk size warning。
`source-files-2.sha256` 冻结 3071 源，SHA256 `5f94aaf81cf1c212030ca3fc2559fba5912730a053a8e452b38ae6cfd8f5cd95`；`source-files-1` 仅为修复前候选，不覆盖新源码。

`app-artifact-2.json`：默认 App SHA256 `92f3abb22a823ff2c28ddda83447891bf414bb77f195defad1487c3d10825eae`。
没有修改 Node worker/protocol 源，本批没有冒领上一批完整 Node 37文件296 的结果；本批 native 集合确实运行 source worker。

## 下一步与不能外推的范围

生产 task card/picker descriptor 仍 Desktop-only，尚未向 UI 暴露 managed 选项。后端命令编译与 synthetic/native 测试不等于真人主窗口点击、真实 Grok、installed App、三平台或最终发布验收。

1. 接 `computerTaskPicker` API 和共享 task controller：明确“本任务独立浏览器”的一次确认，busy/Stop、失败 retry、owner/key replacement、挂载不启动、receipt 等 Host 事实。
2. 复用项目 Select/GlassModal/chrome，简洁范围说明、键盘/焦点恢复及 15 locale；卡片和面板共享一次请求，不新增逐工具授权，不长 App/AppWorkbench。
3. 从实际 production UI → 新命令 → owned runtime → 默认 App/ACP 的隔离联合验收，用户实际系统授权仍由用户批准。
4. 完整 U04 其它 surface/dialog/跨 surface/增减范围、U05其它恢复、U06–U09 与 D00–D31，三平台/真实 App/Grok/签名安装/12h/UI UX 均继续，不以本批入口替代。

## 构建清理约定

遵守 AGENTS 第9条：仅在原 build/test/browser owners 退出后，检查精确旧/新缓存代及 reparse/活 owner，清本批已确认废弃的代；当前程序、最新缓存、依赖、immutable seed、源码和所有红/绿证据保留。
本批已记录 13 组精确旧/新 App/Core 缓存代，`cache-cleanup-plan.json`；首次 inspect 因另一个 session-import 任务在独立 Temp target 编译，被保守全局 Rust guard 拒绝，删除前停止，未动该任务/进程/目录。
随后无关 Rust build 退出，重新只读 inspect 全部13组路径后执行 `cleanup-obsolete-generations.ps1 -Execute`，实际 exit0。仅删除这些旧代，不删除整个 target/node_modules、共享缓存或另一个任务的目录。

- 删除逻辑 6,532,657,628 bytes（6.08 GiB，存在 hardlink，不能当作实际释放量）。
- H 空闲观察增加 3,689,705,472 bytes（3.44 GiB），清理后观察空闲324.16 GiB。报告 `cache-cleanup.json` / `cache-cleanup-before.json`。
- 最新缓存逐文件 hash、最终默认App/Core artifacts和 immutable seed hash 均保持不变；清理后3071源码逐项复验不变，diff check通过。
- 原本批所有 build/test/browser owners 已退出。没有停止无关任务；原失败/红色证据、依赖、当前程序/包、seed/源码保留。
- 删除不在回收站，属于可以重新 cargo build 生成的缓存。此前5.11 GiB属于producer批次，不再次计入本批3.44 GiB。
