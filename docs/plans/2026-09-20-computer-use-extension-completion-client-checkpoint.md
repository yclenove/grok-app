# ExistingTab：扩展清理回执与实际浏览器收束检查点

日期：2026-09-20。工作区 `H:\aicoding\grok-app-computer-use`，
分支 `feat/computer-use-implementation`，HEAD `30757366`。
上一轮是有实质进展的 Host 合同增量；本轮继续实现扩展端。
总状态仍为 **partial — not releasable**，默认关闭，未 commit/push/PR。

## 实现

`completion-scope.mjs` 提供独立清理对象。它只复制原 endpoint、严格验证的
binding 和 completion key，私有字段不持有 PairingClient/PairingSession 或旧
sessionKey。POST status/settle 没有 Bearer、Cookie 或重定向；响应限 1024 字节，
超时 5 秒，状态 shape 严格校验，错误不回显原始异常/正文。并发同类请求合并，
settle 确认后清除私有正文，重复 settle 不再发网络请求。

`PairingClient` 在 claim 之前捕获此对象；claim 要求原 epoch 和完整 connection，
换配对后的迟到成功也被拒绝。claim 失败不自动重试。原 loopback 地址校验提取
为 `loopback-endpoint.mjs`，继续由 PairingClient 原路径导出。

`action-completions.mjs` 是动作所有权控制器，当前仅由测试/探针调用，尚未接入
生产 `ExtensionTransport` 或 `sw.js`。内部 v2 请求形状绑定完成 proof 与完整
session/run/tab/Chrome documentId/grant/snapshot，仅接受已实现的固定语义
click/set_value/type_text/scroll/wait。Host typed action 队列/命令绑定尚未实现，
这份内部结构不能被宣称为已经协商的生产动作 wire protocol。

- 先保存清理 scope，再单次 claim；没确实收到有效成功就不调用浏览器动作。
- 原动作 Promise、状态 watcher、取消请求和回执各有所有者。reset 只取消，
  不删除记录；原调用、取消调用和 watcher 都结束后才考虑发送 settle。
- 原生完成没有得到证明时保留 unknown 占用，不能凭 HTTP 超时或异常释放。
  控制器最多 8 个 pending，完成派发 ID 记忆最多 64 个；同 tab 互斥。
- 清理 HTTP 失败只重送 settle，单一计时器从 1 秒退避至最多 30 秒，仍占用槽；
  不重复 claim/点击/输入。重配对或 unpair 后该清理对象仍可完成旧占用。
- 原动作已返回但 watcher/回执尚在收尾时发生取消，也不能把结果发布为成功。
- `SharedTabs.act` 现在必须匹配输入请求与分享时捕获的 Chrome documentId；
  DOM snapshot 和 document generation 仍同时校验。

## 实际链路证据与边界

新增命令：`cu_probe existing-tab-completion`。使用本批重建 App 的 process-wide
Broker 和生产 HTTP claim/status/settle、真实 Chromium、MV3 isolated-world
固定脚本、实际 DOM 计数器/字段值。配对客户端、SharedTabs、观察 transport 和
动作所有权控制器从源码加载在可信扩展页；MV3 service worker 存在，但动作
协调本身在该可信页执行，不能冒充 SW 重启验收。

观察走真实扩展轮询。grant/offer 由探针私有父子进程管道进行，完成 key 只经
此管道和 HTTP 正文流转，不进入日志/文件/URL。没有公开调试 HTTP 授权接口。
它不是生产 MCP `computer_act` 派发，也不证明用户点 Tauri UI 或 toolbar。

真实 8 项全部通过：

1. 实际配对及 Host/扩展观察；
2. 单次点击，独立页面计数器增加一次，Host 回到 idle；
3. 同 request 重复输入为零派发，计数器不再增加；
4. 中文与 emoji 输入由独立字段值确认；
5. Host 已接收 claim，但响应被故障注入丢弃：零动作，随后有效 settle；
6. Host 已完成 settle，但响应丢失：只重送清理，点击不再增加；
7. Host 取消真实 Wait，等待原 DOM 操作退出后才释放占用；
8. 延迟真实原脚本和取消脚本的回包，然后 unpair：旧 pairing key 已删除，
   仅释放取消回包仍 busy；两个都完成后用独立 scope 结算，用户 tab 保持打开。

## 首败与修复

证据根目录：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

`existing-completion-client-live-first.log` 与 `-second.log` 均在清理回执响应丢失
场景失败。`-diagnostic.log` 使用安全布尔值确认 `illegalInvocation=true`。
原因是默认 `setTimeout`/`clearTimeout` 被保存后以控制器的方法接收者调用；
Node 测试及注入计时器允许此用法，Chrome 拒绝。现改成显式 globalThis 包装。
`existing-completion-client-live-fixed.log` 在同场景及剩余场景全部通过。
所有首败证据保留，没有通过缩减断言、改变超时或删除故障场景规避。

## 验证

| 检查 | 结果 / 日志 |
| --- | --- |
| 扩展 + MCP Node | 91 passed、0 skipped；`existing-completion-client-all-final.log` |
| 新真实回执/动作链 | 8 passed；`existing-completion-client-live-fixed.log` |
| 原 DOM/SharedTabs 真浏览器 | 13 passed；`existing-completion-client-kernel-live.log` |
| 原生产 App/MCP/扩展观察链 | 34 passed；`existing-completion-client-observation-app.log` |
| App 严格 Clippy | lib + computer-use-probe、`-D warnings`；`existing-completion-client-app-clippy.log` |
| cu_probe 重建 | passed；`existing-completion-client-build.log`，仍有已记录 Windows linker stdout warning |
| 质量门禁 | final PASS，千行文件 80/80；`existing-completion-client-quality.log` |
| 格式 / 词条 | Rust fmt、diff whitespace、JS syntax、15 locale 校验通过 |

本轮没有修改 Core 源码，不重复将上轮 423 Core + 12 Driver 算作本轮新增验证。
原生产观察链使用本轮重建的 App 与当前扩展源码，结果见末节。

## 下一步仍属于完整 C3

1. Host 中增加实际 typed action 队列和命令/ref 绑定，并定义 Rust/JS 同步的
   version 2 包、单次派发、结果合同和显式能力握手。当前 proof 只绑定动作
   身份，尚不绑定实际 typed command；私有探针能操作不意味着生产队列完成。
   claim/结果提交均须同次原授权、原 snapshot 和原动作预算，不能由模型自批。
2. 将控制器接到 ExtensionTransport/sw.js，换配对保留旧 cleanup owner，
   当前模型结果和旧回执分别处理；协商失败/旧 v1 必须明确只观察。扩大文本
   响应上限前先按严格 typed packet 计算边界，不能直接无限放大读取。
3. 先解决 App/SW 重启持久占用与原文档退休证明，再公开动作 capability。
   目前所有权与 receipt 仍在内存里，重启不证明浏览器旧脚本结束；64 条/5 分钟
   tombstone 淘汰后的清理恢复 UX 也要纳入此设计。不得为了恢复可用而把 unknown
   直接解释为 idle，不能导出旧 pairing key 或关闭用户 tab 来清理。
4. 通过真实生产 MCP 动作闭环后补齐 key、navigation、contenteditable、页面
   滚动等 parity；只派发 KeyboardEvent 不算真实按键默认行为。

保留全部后续范围：原生 toolbar activeTab/截图成功、原生启动内部取消、App/ACP
生命周期和 UI、安装修复升级回滚卸载、Chrome/Edge、macOS arm64/x64、Linux
X11/GNOME native Wayland、真实模型、全候选至少 12h 主动长稳。历史 IPC Windows
10053 的根因仍未关闭。本批通过不等于最终版完成。

## 最终观察回归、指纹与资源

本批源码范围与上一份 Host 检查点相同：297 文件，
`86B15B6CD7D18B4135D7500CDB150038D518A971DE6DFA0658DB49631A0BB531`。
cu_probe SHA256：`2722013B749A41C91384753068569F8242B2D2516A02A1DD9427DABDDE7854F2`。
日志 `existing-completion-client-fingerprint.log`；这是局部摘要，不是 C6 发布冻结。

`cu_probe existing-tab-extension` 全部 34 项通过，包括配对/心跳失效/共享、
真实私有 Node/MCP 观察、预览隔离、Stop 与迟到结果拒绝。此次 fixture Stop
取消观察为 5 ms，10000 元素截断观察往返为 284 ms；单次数据不作性能承诺。
仍不包含 toolbar 原生授权后的截图成功或生产 MCP 动作。

最后只读进程检查没有匹配本轮 owned cu_probe/private Node/临时 Chromium 的
存活进程。两类探针都按 owner marker 核验后释放临时浏览器 profile；证据与
隔离 App home 保留。HEAD 未变、index 空；没有操作日常 profile、账号或代理。
