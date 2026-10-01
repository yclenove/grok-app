# Computer Use：真实受管浏览器 Wait 检查点

日期：2026-09-20；工作区：`H:\aicoding\grok-app-computer-use`。
分支：`feat/computer-use-implementation`，HEAD：`30757366`。
总状态仍为 **partial — not releasable**；本批不是最终版、安装版或跨平台验收。

承接 [观察/动作事务检查点](2026-09-20-computer-use-observation-transactions-checkpoint.md)。
本批改的是产品实现、回归和探针，不是只更新状态标签；未提交、推送或提 PR。

## 首败与根因

真实 source worker + 私有 Node 20 + seed Chromium 的 Broker Wait 首败：
`managed-wait-real-red.log`，exit 1，已进入断言：
`managed wait must match the original observed element: Some("wait timed out")`。
独立页面计数器仍为 1。Fake 的稳定 n1 ref 不能代表真实浏览器。

原因是 Broker 原来截获 Wait，重新观察后拿新 snapshot 的 opaque refs 与旧
elementRef 比字符串；即使页面上原元素已匹配也找不到。已有 worker 能持有
精确 DOM handle，但其 Wait 旁路没有进入动作账本/取消/互斥。

## 已改实现

1. 非 Desktop 的 typed Wait 交给对应 adapter，managed 使用原始 DOM handle。
   每轮前后检查 snapshot、文档身份、元素连接/签名、可见性与取消；同名兄弟、
   同 ID 替换节点都不能顶替原元素。使用单调期限，过期不接受迟到匹配。
2. worker Wait 进入统一 actionId 去重、互斥和取消，但不调用页面 mutation
   失效逻辑。成功/未匹配均不主动推进模型 snapshot；导航/Stop 的失效仍有效。
3. `computer_act(wait)`、独立 `computer_wait` 和旧 `browser_act(kind=wait)`
   共用 run admission、动作预算与模型观察预算。一次准入记一次观察尝试，
   超时也记；重放同 actionId 不重复轮询、不重复扣费，忙碌拒绝不计。
4. `computer_wait` 新建 typed `tools_wait.rs`，要求显式完整 observation 身份，
   不能用 preview/Host 当前值补身份。未知字段、跨 run、缺字段、null timeout
   拒绝；更新 MCP schema。字段和升级约束见 Computer Use wiki。
5. managed worker `/health` 宣告 `page.waitCondition=1`。Host 先检查版本能力；
   不支持的 worker 在 `/act` 之前拒绝。Wait 返回的新 page generation 也拒绝。
6. 修复 native action 的线程退出顺序：物理操作结束后先释放 worker 的 guard，
   再向 caller 发结果；caller 的 guard 仍覆盖账本提交。否则结果已经返回时
   worker 退出尾声仍可能占用 admission，让紧接着的合法动作偶发被拒。

Desktop 仍是原始 native ref 的有界文本轮询；不是浏览器通道，也不会派发桌面输入。
ExistingTab 的 typed actions（包括 Wait）仍未实现，不用 managed 的成功冒充它。

## 全量回归发现的问题（首败保留）

- `managed-wait-core-full-01.log`：371 passed / 3 failed。
  同目标连续动作的 busy 失败由上述 guard 顺序修复；unknown 测试原来在 30 ms
  后推断线程已执行，改为显式 adapter 进入信号和释放门，保留零第二次执行、
  未物理完成不可 observe、完成后仍需 observe、重复 actionId 不重放全部断言。
  第三项是 source worker 已变而 seed 尚未生成，导致 sibling 缺失测试先报
  hash_mismatch；通过标准 prepare 生成，不手改 manifest/放宽哈希检查。
- `managed-wait-browser-full.log`：91 passed / 6 failed。
  6 项均在 observation-extract 单元 fixture：它按函数源码包含 isConnected
  就返回 bool，新完整 descriptor 也含该字段，被错误识别。fixture 现在先区分
  完整 descriptor，再处理独立连接检查，未知 evaluate 明确抛错；原断言不变。

## 当前证据

证据目录：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。
以下都是本批实际执行，不继承上一批计数。

| 门禁 | 结果 | 日志 |
| --- | --- | --- |
| 确定性 Broker 事务竞争 | 10 passed | `managed-wait-admission-02.log` |
| 完整 Core + Driver | 376 + 12 passed，0 ignored，exit 0 | `managed-wait-core-full-02.log` |
| 完整 browser Node | 97 passed，0 skipped，exit 0，42.16s | `managed-wait-browser-full-02.log` |
| Extension/MCP | 54 passed，0 skipped，exit 0 | `managed-wait-extension-mcp.log` |
| Core 严格 Clippy | all-targets/all-features，exit 0 | `managed-wait-clippy-core.log` |
| App 严格 Clippy | default + probe 两种 all-targets，exit 0 | `managed-wait-clippy-app.log` / `managed-wait-clippy-probe.log` |
| Code quality final | PASS，千行文件 80/80 | `managed-wait-quality.log` |
| fmt / diff whitespace | exit 0；未改全局换行设置 | 本轮工具输出 |
| 重建 cu_probe | exit 0，1m02s；保留既有链接器 stdout 警告 | `managed-wait-probe-green-build.log` |
| 实际 source managed | exit 0；原 ref Wait + `computer_wait` alias 匹配，随后旧 ref 真点击 | `managed-wait-source-preview.log` |
| 实际 seed managed | exit 0；明确运行生成 worker，相同后置条件 | `managed-wait-packaged-preview.log` |
| 私有 Node/MCP + App Broker/IPC + MV3 | 34 passed，exit 0 | `managed-wait-existing.log` |
| Computer 前端/API/domain | 12 files / 59 tests passed，exit 0 | `managed-wait-ui.log` |
| Typecheck | exit 0 | `managed-wait-typecheck.log` |
| Computer 前端定向 ESLint | `--max-warnings 0`，exit 0 | `managed-wait-eslint.log` |
| seed 完整 check（实际探针之后） | exit 0，manifest/tree 未变 | `managed-wait-seed-check.log` |

97 项包含真实浏览器 HTTP：原节点名称改变、同 ID 节点替换、隐藏、snapshot
替换、真实导航、cancel-run；等待中重复请求和竞争点击拒绝，独立 oracle=0，
成功重放取旧结果。同名兄弟从一开始就已是期望名称，不能让等待提前匹配。
源码 unit fixture 不代替这些真实 DOM 后置条件。

source/seed 探针的两个 Wait 均保持独立计数器为 1，随后原模型 ref 点击后为 2；
`computer_wait` alias 使用生产 tools dispatch，本项不是完整 stdio MCP/真实模型。
探针同时验证三种旧 worker 缺能力时在 observe/act 之前拒绝。运行的是私有 Node
20 + seed Chromium，不是系统 Node，不代替安装版或 Edge。

ExistingTab 的 34 项仍只证明配对/租约/分享/观察/Stop 和截图权限拒绝，不包含
真实 toolbar activeTab 截图成功或 ExistingTab typed actions。此次 MCP Stop
取消 pending 观察测得 5 ms、tabOpen=true，只是隔离 fixture 的结果，非通用 SLA。
前端是 jsdom/typed invoke 回归，不能冒充用户点击 Tauri UI 或完整真实模型闭环。

运行时通过 `node scripts/prepare-computer-use-runtime.mjs --prepare --target
x86_64-windows` 生成（命令单行执行），exit 0；日志 `managed-wait-seed-prepare.log`。
manifest：`6aaa08c640ef8bd65606dd789f7973e98d9115c7313357bdae5961e75be47531`。
tree：`6fd12589c34085c3907de65fda8a4fe349bb48835f137132557cded45796232e`。
Chromium tree 仍为 `63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3`。

cu_probe SHA256：`ED67A1475D69821EFD35E4649E242DEAC05ABBAA8E9A8A30FCA92ADFE435EBC6`。
244 文件局部源码指纹：`A89338CAD409A61D690F36978A37A8E6E1C52CCC5D5EC2FABA6AFFD5989CACDE`；
日志 `managed-wait-source-fingerprint.log`。范围为 `rg --files` 枚举：
`src-tauri/computer-use-core/src`、`src-tauri/src/computer_use`、
`tools/computer-use-browser`、`tools/computer-use-extension`、`tools/computer-use-mcp`、
`src/components/computer-use`、`src/lib/computer-use`、`src/lib/api/computerUse.ts`、
`src/lib/api/computerUse.authorize.test.ts`、`src/lib/api/computerUse.pairing.test.ts`。
路径统一 `/` 后用 PowerShell Sort-Object 排序，每行 `path + 空格 + 大写 SHA256`，
以 LF 拼接、无末尾 LF、UTF-8 后计算 SHA256。包含本批相关 fixture/tests，
不包含全仓依赖、构建配置及所有入口，因此**不是 C6 候选冻结**。

最后 read-only 检查：分支与 HEAD 未变，index 空，未发现 cu_probe、本工作区
私有 Node 或本轮 managed-contract/pairing-owned profile Chrome 的进程残留。
本批没有全仓 pnpm test/build:ui/安装测试，不把定向 59 项写成全仓前端验收。

## 后续不豁免

1. managed 旧 model handles 的 action-aware 回收：失效立即撤销身份，但已准备
   尚未完成的操作不能被提前 dispose；成功/失败/取消/替换均应释放。
2. managed loopback HTTP 进行中的取消仍有现有 15 秒等待边界，worker 的
   cancel-run 成功不等于完整 App HTTP 立即结束；后续补实际 Stop 竞争证据。
3. 继续完整授权竞争、owned profile/process 生命周期和 ExistingTab typed actions。
4. 保留真实 toolbar activeTab/截图成功缺口；C4 两轮 App-shell/ACP 生命周期，
   C5 Windows/Edge/安装/macOS arm64+x64/Linux X11/GNOME native Wayland，
   C6 真实模型及同一候选至少 12 小时主动长稳，均不能由本批局部测试替代。

仍只操作隔离 fixture，不读取真实账号/浏览器资料，不改代理，feature 默认关闭。
