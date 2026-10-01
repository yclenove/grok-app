# ExistingTab：Host 领取、撤销与完成回执检查点

日期：2026-09-20。工作区 `H:\aicoding\grok-app-computer-use`，
分支 `feat/computer-use-implementation`，HEAD `30757366`。
总状态仍为 **partial — not releasable**；本批没有开启生产动作能力。

## 已实现的边界

本批实现 [完成回执 ADR](2026-09-20-computer-use-existing-action-completion-adr.md)
的 Host registry 与 HTTP 部分。它不是 MV3 动作派发完成，也不是物理浏览器
结束的独立证据。扩展仍使用 Observe-only protocol 1，adapter 动作能力仍 NONE。

- `browser/extension_completion.rs`：offer → claim → settle。领取只成功一次；
  未领取项取消或 10 秒到期可以退休，已领取项即使到期仍保留占用，直到回执。
  completion key 为 256 位随机值，Host 只存完整身份的 HMAC；Debug 遮蔽 key。
  身份绑定 request/connection/session/run/tab/document/grant/snapshot，且版本有界。
- `browser/host/completion.rs`：配对、借用授权、动作领取及回执使用同一把 Host
  锁；旧文档的 observation.page_generation 不能用于新动作。观察请求与动作
  共享总容量 8，同一 tab 互斥；已领取动作不按时间淘汰。
- Pause、Stop、归还、精确释放授权、导航、断开、Unshare/重新 Share、撤销/替换
  配对均立即取消领取权。重新授权或 reconnect 不得越过旧占用；reconnect 的
  复验与修改现在在同一锁内。停止过的 run 不得重新借用标签页。
- 完成 tombstone 最多 64 条、保留 5 分钟；重复有效 settle 幂等，超时淘汰后
  拒绝旧回执。旧 settle 不影响新 owner；用户标签页不因清理而关闭。
- Broker 的 ExistingTab `is_idle` 已计入 claimed 占用。集成测试证明 Stop 清理
  返回后仍为 StopRequested，收到回执后才可变为 Stopped。

HTTP 新增三个 POST：`/cu/extension-completion/claim`、`/status`、`/settle`。
claim 要求当前配对 Bearer 和 feature gate；status/settle 仅凭该动作独立 proof
清理，feature-off 或旧 pairing key 删除后仍可使用，不续租或恢复任何授权。
没有 offer 的外部接口，也没有提供页面操作或任意脚本接口。

三个入口均执行精确安装扩展 Origin、loopback Host 和 forwarded-header 检查，
正文上限 4096 字节，strict serde，固定独立限流桶（40 次/800 ms）；观察 poll
不能占满清理的桶。重复 Origin/Host/Authorization 头被拒绝。JSON 解析失败只
返回固定错误，避免 serde 诊断回显提交的凭据。所有回包都没有完成 key。

## 首败与修复

证据根目录：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

1. `existing-completion-red.log`：真实断言复现同 tab 已有观察时仍可 offer 动作、
   Pause 未即时取消 completion token。补共用准入和生命周期撤销后通过。
2. `existing-completion-http-first.log`：负例发送了一个正确和一个错误 Origin，
   原入口只读首值。已禁止重复身份头。这不是单一错误 Origin 成功通过的证据。
   同时将测试误用的 localhost（合法 loopback）改为外部 Host 负例。
3. `existing-completion-generation-red.log`：新增测试初次编译的导入/方法名错误；
   不是断言证据。修正后 `existing-completion-generation-assertion-red.log` 真正
   复现旧 observation 文档版本可用于 offer。补 page_generation 复验后通过。

## 已通过的验证

| 验证 | 结果与证据 |
| --- | --- |
| Core 完整回归 | 423 passed、0 failed、0 ignored；`existing-completion-core-final-all.log` |
| Driver | 同一日志 12 passed、0 failed、0 ignored |
| 本批定向 | 22 passed，包含新增 18 项及原有 4 项；`existing-completion-core-final-targeted.log` |
| Core 严格 Clippy | all-targets/all-features、`-D warnings`；`existing-completion-core-clippy.log` |
| App 严格 Clippy | lib + computer-use-probe、`-D warnings`；`existing-completion-app-clippy.log` |
| 代码质量门禁 | final PASS，千行文件 80/80；`existing-completion-quality.log` |
| 格式与词条 | cargo fmt --all --check、diff --check、15 locale 生成一致性通过 |

新增测试覆盖所有 proof 身份字段和 key 篡改、重复/并发领取、领取响应丢失后
不重新领取、claim/Pause 竞争、claimed 超时仍 busy、混合队列容量、tombstone
TTL/容量、全套 Host 撤销入口、替换配对后新 owner 互斥、Broker Stop 状态，
以及真实 loopback HTTP 的 feature-off 清理、来源/正文/回显/限流负例。
这些测试没有派发浏览器动作，不能替代原始 executeScript Promise 的收束证据。

App 重建与真实观察链回归的最终结果见末节，使用本批重建的二进制。

## 下一批接线顺序（仍属原 C3，不是缩减目标）

1. 定义 version 2 动作包与显式能力握手。当前 completion binding 尚未绑定
   typed action/elementRef/参数；新增 enqueue 时必须在同一 Host 锁内校验原
   snapshot/ref 并绑定实际命令，不能把现有 offer helper 当模型操作 API。
   actionId 由 Broker 去重，requestId 是单次派发身份，不混用。
2. 扩展先取得独立 completion scope，再尝试 claim；只有确实收到成功且当时
   授权仍有效才执行。claim 响应丢失/取消时禁止执行或重试 claim，只能清理。
   scope 只复制 endpoint、binding、completion key，不保留旧 PairingSession
   或旧 sessionKey；reset/换配对不能丢弃其原脚本与取消脚本的完成等待。
3. `ExtensionTransport` 接入 `SharedTabs.act`，保留原脚本、取消 watcher 和回执
   的全部所有权。先停止 watcher 并等待已发请求收束，确认原脚本及取消脚本
   完成，再 settle。业务结果只能在当前授权下提交，清理回执可以幂等重送。
   状态 HTTP 失败不能被当作 settled；容量耗尽明确拒绝，不能丢弃 unknown。
4. 单独设计 App 崩溃/SW 重启后的旧动作隔离与原文档退休证明。当前 registry
   仅在内存中，重启后清空并不证明旧脚本完成。此项未闭合前不开放完整动作
   能力，不能用新的 session 或新 observation 绕过旧操作。
5. 真实 MCP → App/Host → MV3 → 页面副作用 oracle：执行一次、Wait 取消、
   脚本排队中 Stop、回包丢失、换配对、导航/权限失效、SW/App 重启，以及两
   session 同 tab 争抢。通过后按实际能力连接 adapter.act，再补 key/navigation
   和全部动作 parity；synthetic KeyboardEvent 不能冒充真实按键默认行为。

其他未完成门禁完整保留：真实 toolbar activeTab/截图成功、原生启动内部取消、
App/ACP 生命周期与交互、安装修复升级回滚卸载、Chrome/Edge、macOS arm64/x64、
Linux X11/GNOME native Wayland、真实模型、同一最终候选至少 12h 主动长稳。
历史 Windows IPC 10053 的根因仍未关闭，绿色复测不代表解释了它。

## 本批最终构建、真链与指纹

`cargo build -p grok-computer-use-probe --bin cu_probe` 通过；日志
`existing-completion-build.log`。仍有 Windows 链接器“正在创建库”的 stdout
warning，未通过 lint 豁免掩盖；严格 Clippy 已单独通过。

本批重建 `cu_probe.exe existing-tab-extension` 在既有 owner 标记的隔离 App
home 中通过全部 34 项检查，日志 `existing-completion-observation-app.log`。
这是实际 process-wide App Broker、私有 Node/MCP、MV3、Chromium 的观察链，
覆盖配对/租约/共享、observe/preview、旧结果拒绝、Stop 与截图权限拒绝。
本次 Stop 取消观察 6 ms，10000 元素扫描的 Host 往返 282 ms；都是 fixture
单次观测，不作性能承诺。它没有执行新增动作回执或真实 toolbar 截图成功链。

源码局部指纹（不是发布冻结）：290 文件，
`860228271C908EB4EC578AD57C2CEB8AE199DB830CD65F0793A3829D4C505F22`。
cu_probe SHA256：`E5F473EAC51AB2C9F70ED07CD08983CB549573B2A1C6C782AC71E2C2B85D52FF`。
证据 `existing-completion-fingerprint.log`。范围：Core src、App computer_use、
browser/extension/MCP/probe tools、Computer Use components/lib、computerUse API
及其 authorize/pairing 测试；rg 枚举 rs/mjs/json/html/css/ts/tsx、遵循 Git ignore，
路径转 `/` 并以 PowerShell Sort-Object -Unique 排序。逐行“路径 空格 大写文件
SHA256”，LF 拼接、无末尾 LF、UTF-8 后再次 SHA256。文档及构建产物不在源码摘要内。

结束时只读检查未发现匹配本轮 owned probe/private Node/临时 Chrome 的存活
进程。探针按 owner marker 清理临时 profile，证据与隔离 App home 保留。
HEAD 未变，index 空；未 commit/push/PR，未操作日常浏览器资料、账号或代理。
