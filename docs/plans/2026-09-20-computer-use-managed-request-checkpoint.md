# Computer Use：受管请求身份与应用暂停接线

日期：2026-09-20。工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`，HEAD `30757366`。
总状态仍为 **partial — not releasable**。

## 实现

- Broker 同次准入捕获 owner、worker revision、取消 token 和具体 worker Arc。
  `ManagedRequest` 不含模型可写的身份字段，不以 targetGeneration 代替 revision。
  观察、预览、普通动作、Wait，以及 browser dispatch 的导航、下载、上传、
  open/new-tab/popup 共用这份身份。动作参数中的 runRevision 被客户端的准入
  revision 覆盖，owner 不符在发送前拒绝。控制交换使用独立取消上下文。
- `ExistingTabHost::for_managed_request` 是请求局部视图，共享原 Host registry，
  仅固定本次 worker/client；不是新进程、全局 current-run 或 thread-local。
  worker 插槽换新不会将旧请求、旧 run 的暂停、Stop、profile clear 转给新实例。
  scoped 入口和结果发布复验请求，暂停后迟到 open/page/observation 不恢复授权。
- Playwright 业务请求真正使用可取消 HTTP，含 capability health 请求。
  `ManagedBrowserAdapter::abort/is_idle` 已接入 pause/status；恢复只有 worker
  确认 paused、物理请求归零才推进 revision。socket 退出仍不算远端结束，
  terminal Stop 仍须 cancel-run 确认 context 清理和本地 opening reservation 收束。
- Broker managed 授权将 profile open 与最终 grant 发布纳入同一个 admission。
  中途 Pause/Stop 可以先行 fence；恢复/其他授权不能抢入；普通重新授权同样
  走该入口。`commit_authorize` 换 token 不会让旧请求借用新 token。
- 恢复准入还捕获具体 pause epoch。等待状态期间的新 Pause 会使旧恢复票据
  失效，不能晚到后又恢复 worker。恢复交换已经发出时若被另一 Pause/Stop
  覆盖，或回复丢失/非法，保留 uncertain，**不自动重发恢复**，需要 Stop 收尾。
  即使旧 revision 已无法查询，成功的终态 cancel 仍能证明清理并完成 Stop。
- browser dispatch 在返回时再次检查原 run generation；迟到成功改记 unknown，
  不返回可重放的成功结果。

未改变默认关闭策略、UI 文案、账号、日常浏览器 profile 或代理配置。

## 首败与验证

日志根：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

`request-binding-first-red.log` 是实际 Chromium 首败：旧 App Broker pause 返回
成功，但独立 worker run-status 仍是 running，探针明确失败于
`Broker pause did not pause the actual worker`。原来的普通恢复探针缺少这条
远端后置条件，不能据此判断取消协议已接通。

初次完整 Core `request-binding-core.log` 为 398 pass / 1 fail：原来仅测试桌面
租约隔离的 CountingSurfaceAdapter 没有 worker，新的 managed 准入正确拒绝。
为该 fixture 显式提供 Recording worker，保留租约/动作断言。
`request-binding-core-02.log` 为 401 pass / 1 fail：暴露 managed authorize
在检查缺失 executor 前尝试取得 worker。调整生产检查顺序，原断言保持不变。
`request-binding-core-03.log` 为 403 Core + 12 Driver passed。

新增局部回归覆盖：准入后替换 worker、跨 owner 拒绝、仅 revision 失效即可
拒绝旧请求、预取消零 open、恢复回复丢失不重发且可 Stop、新 Pause 撤销旧
恢复票据、授权 open 期间 Pause 阻止发布/提前恢复。Recording 回归只证明
Host/Broker 事务，不代替真实浏览器物理动作验收。

`request-binding-source-01.log` 已通过 App Broker → source worker → Chromium
普通 pause/resume/重新授权/观察，worker revision 1 → 2，独立页面点击仍为 2。
该次尚未包含随后增加的 pending Wait gate。

最终完整检查与源码/seed 的 pending Wait 结果在本文件末节记录；未记录的
门禁不能推定通过。worker JS/seed 本批没有修改。

## 下一项与保留缺口

1. 真链原生 click、下载、慢截图和 Chromium 启动内部的 Pause/Stop 竞争，
   以及 pending resume 被 Pause/Stop 覆盖的真实控制回复断连；不能用 Wait 或
   mock activeOperations=0 替代。补齐每个 browser 业务路由的独立 wire 身份
   和后置条件检查，尤其 upload/download/popup。
2. 验证全部 App/ACP 入口的异步控制响应；本批同步 Core/客户端接线不能证明
   Tauri UI 不阻塞。恢复 uncertain 的用户恢复体验仍需结合现有 Stop UI 验收。
3. 继续 ExistingTab typed actions、真实扩展工具栏授权/截图、Edge/安装升级
   回滚/卸载、macOS arm64/x64、Linux X11/GNOME native Wayland、真实模型闭环。
4. 冻结后同候选至少 12 小时主动长稳；本批没有冻结整个候选或补齐发布矩阵。
   旧 IPC 10053 偶发失败根因仍未建立，后续通过不能自行销账。

保留全部既有用户/Grok 改动；未提交、推送或创建 PR。前两批审批拒绝删除的
空测试目录仍保留，本批不尝试重新删除。

## 最终实测

| 门禁 | 结果 | 日志（同上根目录） |
| --- | --- | --- |
| 完整 Core / Driver | 404 / 12 passed，0 ignored | `request-binding-core-final.log` |
| 严格 Core / App Clippy | 均 exit 0 | `request-binding-clippy-core.log` / `request-binding-clippy-app.log` |
| cu_probe 重建 | exit 0，保留既有 linker 创建库 stdout 警告 | `request-binding-probe-build-final.log` |
| source worker + 真 Chromium | exit 0，含 pending Wait gate | `request-binding-source-final.log` |
| seed worker + 真 Chromium | exit 0，同一套后置条件 | `request-binding-packaged-final.log` |
| 实测后 seed check | exit 0，manifest/tree 未漂移 | `request-binding-seed-check.log` |
| quality final | PASS，千行文件 80/80 | `request-binding-quality.log` |

新增真实 pending Wait gate：先由独立 run-status 看到 activeOperations=1，
再调用 App Broker pause，要求未匹配的 10 秒 Wait 在暂停后 3 秒界内返回且
不是 Applied/Verified，并确认远端 phase=paused/idle；恢复 revision 2 → 3、
重新授权、旧 Wait 拒绝、新观察成功。普通恢复 gate 验证 revision 1 → 2。
两套 worker 的独立页面点击 oracle 始终恰好 2，没有重复输入。
这不是“正在点击/下载/截图/原生启动时取消”的验收。

fmt check、diff whitespace 均 exit 0；HEAD 未变、index 为空。
只读进程检查未发现本批 cu_probe、私有 Node 或匹配本批 owner 的 Chrome 残留。
本批没有重跑完整 Node/扩展/MCP/前端/安装/其他 OS 矩阵；不挪用前批通过数。

262 文件局部源码指纹：
`1FFF5A45942F14EB29CB61694D15825AFB44DA0AA9EE276F7CE60B100CB73B20`。
日志 `request-binding-source-fingerprint.log`。范围与排序/UTF-8 LF/SHA256 算法
同上批 257 文件指纹，新增 5 文件；不包括全仓依赖与构建配置，**不是候选冻结**。
seed manifest `551f692ba530aca79d582605bc5e2b7d7196502b8ebb6207d2d15e2fe99eb1a8`；
seed tree `6877fdbac8252ac6437ac05ef92d2ce039d37ceef38f3f11dba5b3207af3ea57`。
cu_probe SHA256：`6AF8193D98ACE115E3457C2FB3BB018BB6C504CAA1D5385F1BB44526363B3E23`。
