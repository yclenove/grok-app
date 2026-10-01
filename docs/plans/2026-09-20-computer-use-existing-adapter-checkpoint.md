# Computer Use：ExistingTab 生产观察适配器检查点

日期：2026-09-20；工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`；HEAD `30757366`；未提交/推送/PR。
总状态仍为 **partial — not releasable**。本批是 C3 的观察增量，不是动作闭环完成。

## 已实现

- App 注册真实 `ExistingBrowserAdapter`，只通过现有 MV3 typed transport 读取
  用户明确分享并由 App 授权的 tab。原始 Chrome 数字 ID 转为 `tab:数字`，
  不再被 SurfaceRouter 当作桌面窗口；没有 Desktop/managed fallback。
- 通用 `Observation` 增加有界 `text`，managed 的可见 aria 文本和 existing
  的 DOM 正文都保留到 MCP JSON。上限 32,000 Unicode 字符，超过标 truncated。
- `computer_observe` 支持布尔 `screenshot`，默认 true；existing 分支的
  `screenshot:false` 不调用截图 API。非布尔/未知观察选项拒绝。
  其他 adapter 的默认 capture 只会剥离返回 PNG，不能据此声称所有后端都跳过截图。
- `CaptureOptions` 分开模型观察和 UI preview，并携带 run cancellation。
  existing 扩展 preview 不替换 isolated-world WeakRef snapshot，Host preview
  不替换当前模型 refs/snapshot，Broker preview 也不修改模型动作依据。
- Stop 的同步取消标志传入待处理观察；队列 sweep 和 receive 都复验。
  不必等慢清理/网络期限，迟到 extension result 不能返回给模型。
- surface 授权入口先核对真实 session owner 和 run 状态。existing picker
  授权事务串行，Host 在同一锁内识别「新借用」与「复用旧借用」；失败只回收
  本次新取得的精确 session/run/tab/grant generation。旧清理不能释放新 grant，
  重复清理幂等；成功切换 picker 目标后释放原目标，不关闭/导航用户 tab。
- 已有 tab 的所有动作能力仍明确为 NONE，包括中文输入/IME；没有假开关。
  worker 连通状态使用无秘密布尔查询，不把 test-only session-key accessor 用进产品。

## 证据与首败

证据目录：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

- `preview-refs-red.log`：真实 Node 断言，原 preview 覆盖模型 WeakRef snapshot。
- `observation-text-red.log`：真实 Rust 断言，managed 转换后 JSON 的 text 为 null。
- `existing-authorization-red.log`：2 个真实失败断言，跨会话授权成功，以及
  stopped run 授权失败后遗留借用。修复后 `existing-authorization-02.log` 5 项通过。
  `existing-authorization-01.log` 是修改时匹配到相邻 desktop 代码导致的编译错误，
  未到断言，已修正；不计为产品回归通过。
- 第一次 transport 定向重跑有 1 项 Windows reqwest 10053 connection-aborted
  （oversized HTTP 用例），未保存独立日志；同源码重跑 11 项通过。当前完整 Core
  再次全部通过，仍保留该瞬时失败记录，若再次出现需独立定位。
- `app-adapter-01.log`：真实 MCP 页面观察之后，探针试图为第二个未授权 run
  签发 MCP 凭据，被产品拒绝，整体 exit 1。测试已改为断言禁止签发；另启动
  真实 MCP 子进程，用 A 的 token + B 的 session 验证 HTTP 会话绑定拒绝。
  没有放宽产品凭据发放规则，也没有伪造成功凭据。
- `app-adapter-02.log`：App 私有 Node + 生产 MCP `server.mjs` + process-wide
  Broker/IPC + MV3 + 隔离 Chrome，最终 34 项通过、exit 0。
  其中新增 4 项是生产 adapter 观察、会话绑定隔离、preview 保留模型 refs、
  MCP Stop 取消真实 pending observation；其余 30 项复跑原 C2 检查。

这 4 项的边界必须保留：

1. App 授权由探针私有控制管道显式授予自建 fixture，不是用户点击 Tauri UI；
   后续 MCP/HTTP/adapter/Chrome 调用全部是实际产品路径，不是 scripted tool result。
2. 正文包含 `Visible fixture 你好`，可见按钮 refs 到达 MCP；输入值、密码、隐藏
   内容、textarea 默认文本未进入结果。这里只是 fixture 的隐私用例，不是全面审计。
3. 第二会话检查是未授权会话不签发凭据及跨会话冒用拒绝，不是两个分别授权
   tab 的并发操作矩阵。后者仍需 C3/C4 补齐。
4. 真 preview 经过 DOM 观察后因缺 toolbar activeTab 而被 Chrome 拒绝截图，
   拒绝前后 isolated-world snapshot 和 Broker model snapshot 保持不变。
   成功带图 preview 的 Host/Broker 路径只有 Core synthetic PNG 证据，不能冒充
   真实截图成功。真实 toolbar 正向截图仍 not_run。

MCP Stop 用例只延迟实际 `executeScript` 的返回；未替换 Chrome 结果、权限或
时钟。MCP stop 让待处理 observe 返回错误，Host idle/borrowed=0/stopped，
用户 tab 仍打开；本轮测得约 5ms，不是 SLA。恢复延迟返回后旧操作不能复活。
此用例尚未单独断言迟到真实 HTTP result 的状态码，Core 有明确迟到结果拒绝断言。

| 检查 | 当前结果 |
| --- | --- |
| Core all-features | 361 passed，0 ignored，31.17s；`core-adapter-final.log` |
| Driver 集成 | 12 passed，0 ignored，0.18s；同上 |
| 扩展 + MCP golden | 54 passed，0 skipped；`extension-adapter-final.log` |
| 实际私有 Node/MCP + MV3 + App | 34 passed；`app-adapter-02.log` |
| Core strict Clippy | all-targets/all-features，`-D warnings`，exit 0 |
| App strict Clippy | default / computer-use-probe，all-targets，`-D warnings`；完成日志无警告 |
| Rust fmt / git diff whitespace | passed；未更改 Git 换行配置掩盖已有 CRLF 提示 |
| locale / code quality | 15 locale 一致；final PASS，千行文件仍 80/80 |

未改前端源码，没有把上批前端通过结果写成本轮新执行。App build 的链接器
stdout「正在创建库」仍是既有构建警告；不关闭 lint，strict Clippy 单独执行。
本批结束检查未发现 cu_probe、本轮 private Node 或 owned-profile Chrome 残留。

探针二进制 SHA256：
`8DF1CB16062194B98DF249937FB1885637AACB5D6A8B02F4E877A2B12E15697E`。

局部源代码指纹（不是候选 freeze）：对下列目录 `rg --files` 的 rs/mjs/json/
html/css 文件按路径排序，对每行 `路径（/分隔） + 空格 + 文件 SHA256` 以 LF
拼接、UTF-8 再 SHA256；194 文件：
`E288CC2F184F721341FBC38908C8AC4C82B5069BF521A5F6326422082D0FCD34`。
目录：Core src、App computer_use、extension、MCP、probe。未包含完整 App、
依赖锁文件/运行时/安装工件，不能用作 C6 发布指纹。

## 接续顺序与未完成项

1. 先收齐观察一致性：managed preview 的默认 capture 仍调用 observe，worker
   和 Host 都会替换模型快照；需要传 purpose 到实际 worker、释放 preview 临时
   handles，真实 preview → 旧 model ref action 后置条件。不能说全后端已修复。
2. 检查 Broker 并行 observe 的提交顺序、observe/preview/action 互斥和预算。
   当前 extension 队列限制每 tab 一个 pending，但从 Host 交付到 Broker 提交
   之间不等于完整事务互斥；不要以单请求队列直接证明并发一致性。
3. C3 typed actions：固定 click/fill/key/scroll/wait/navigation，snapshot/ref/
   document/grant/cancel fences，已用 actionId 不重放，unknown 重新 observe。
   真键盘能力须说明 DOM 合成事件与可信输入的差别，不把 dispatchEvent 成功
   当作键盘默认行为已发生。独立页面计数器/表单/CJK/emoji 后置条件验收。
4. `browser_*` 观察/动作和 navigate/download 仍是 managed 语义；与 ExistingTab
   的模型工具路径尚未统一。不要用 raw numeric tabId 绕过 surface 或授权。
5. C1/C2 真实 toolbar Share → screenshot happy path、缩放/DPI/窗口切换与
   capture 期间的导航/撤销竞争；不扩大到 all_urls/debugger/Cookie 权限来做绿。
6. C4 完整 App-shell 与 ACP/删除/恢复/fork/换模型/退出/更新矩阵、两轮隔离 UI；
   C5 Chrome/Edge 与安装/macOS arm64+x64/X11/GNOME native Wayland；C6
   全量 freeze、真实模型闭环、至少 12h 主动长稳。均未被本批局部证据替代。

继续单 writer、重测试串行、不碰账号/日常浏览器资料/代理、不提交推送。
上一轮为有代码与真实验证证据的 **progress**，不是被工具栏缺口阻塞的空转。
