# Computer Use：Grok 下一批执行书（R1.3–R3）

> [!CAUTION]
> 本文件保留 R1.3–R3 的历史执行要求，但不再是当前启动入口。最新状态见
> [Grok 剩余工作总路线](2026-09-10-computer-use-grok-remaining-roadmap.md)，当前只使用
> [B1 发行 Browser Runtime 执行书](2026-09-10-computer-use-grok-runtime-pack-execution.md)
> 和对应的[长任务提示词](2026-09-10-computer-use-grok-runtime-pack-prompt.md)。

日期：2026-09-10  
工作区：`H:\aicoding\grok-app-computer-use`  
分支：`feat/computer-use-implementation`  
基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`  
本批范围：只完成 R1.3、R2、R3；R3 全绿后停止并交付报告，不进入 R4  

## 1. 本批目标

本批不是重做 Computer Use，也不是继续扩 UI。目标只有三个：

1. 把中断的 Rust 参数对象化重构收尾，恢复可编译、可格式化、可测试、clippy 全绿的基线。
2. 把 Browser 的身份、结果和错误变成明确的跨 Rust/Node 契约，禁止靠字符串或 HTTP 状态猜动作是否执行。
3. 让 App-owned Playwright worker 的 observe/typed act/fixture 形成可重复验证的真实页面闭环。

本批成功只代表 Browser 达到 E2/E3 基础，不代表 Existing Tabs、Windows 安装版、macOS、Linux、发行 runtime 或完整 Computer Use 已完成。

## 2. 开始前状态

### 已完成，不要返工

- R0.1 工作区快照。
- R1.1：三条过期测试已按 managed/user-owned 边界修正；当时 core lib 177/177 通过。
- R1.2：`computer_stop` 已位于模型工具集合，不在 Host-only；MCP golden 4/4、TypeScript 15/15 通过。
- UI/i18n/settings 当前定向测试 117/117 通过。

### 当前中断点

R1.3 已引入强类型请求，但仍有旧调用。当前事实：

- core `cargo check` 首先在 `broker/gates.rs` 的 10 参数 `share_and_grant` 失败。
- `tests_lease_schema.rs` 另有一处旧调用。
- `extension_pair.rs` 另有两处旧调用。
- `cargo fmt --all -- --check` 仍有三处机械格式差异。
- `browser.rs` 仍有一个 `needless_bool_assign`。
- `run-fixture.mjs` 没有 Bearer token，属于 R3.1，不属于 R1.3。

## 3. 固定执行规则

每个原子项执行：Read → State → Red → Implement → Targeted test → Review → Gate → Record。

- 每次只做下列一个编号；当前编号未绿，不进入下一个。
- 每项开始和结束都追加 `2026-09-09-computer-use-execution-state.md`。
- 保留全部未提交修改；禁止 reset、clean、checkout --、stash、强切分支。
- 不 commit、push、PR、merge、tag、release。
- 不碰用户账号、Token、Cookie、代理、正式安装或真实 Chrome/Edge profile。
- 不加 `allow`、skip、ignore，不降低 lint，不用宽松测试替代真实后置条件。
- 根目录只用 pnpm；Rust 继续使用专用 `target-cu-review` 和 `--offline`。

## 4. R1.3a：收尾参数对象化并恢复编译

### 读取

- `src-tauri/computer-use-core/src/browser.rs`
- `src-tauri/computer-use-core/src/broker/gates.rs`
- `src-tauri/computer-use-core/src/broker/tests_lease_schema.rs`
- `src-tauri/src/computer_use/extension_pair.rs`
- `src-tauri/src/computer_use/playwright_worker.rs`

### 修改

将剩余四处旧式 `share_and_grant(...)` 改为 `share_and_grant(TabAttachment { ... })`。字段必须显式填写：

```rust
TabAttachment {
    session,
    run_id,
    tab_id,
    title,
    url,
    origin,
    extension_id,
    pairing_token: Some(pairing_token),
    home_index,
    document_generation: Some(1),
    connection_generation: Some(1),
    focused,
}
```

保持原测试数据与行为，不趁机修改 pairing、授权或导航语义。`extension_pair.rs` 已导入 `TabAttachment`，其余测试模块可通过现有 `super::*`/crate 路径引用；若编译器要求，增加最窄 import。

把 disconnect 分支的冗余布尔赋值化为：

```rust
rec.info.closed = !rec.info.user_owned;
```

然后执行 `cargo fmt --all`。这一步允许格式化当前 Rust WIP，但不得格式化整个前端或批量改换行。

### 搜索检查

```powershell
rg -n "\.(offer_shared_tab|picker_grant|share_and_grant|act_managed_tab|act_page|upload_file)\(" `
  -g "*.rs" -g "!tools/computer-use-cua/**"
```

逐条检查搜索结果：所有相关调用必须使用当前强类型对象；不得仅凭搜索数量判断完成。

### 门禁

```powershell
cd H:\aicoding\grok-app-computer-use\src-tauri
cargo fmt --all -- --check
cargo check -p grok-computer-use-core --features test-support --offline --target-dir target-cu-review
cargo check -p grok-app --offline --target-dir target-cu-review
cargo test -p grok-computer-use-core --lib --features test-support --offline --target-dir target-cu-review -- --test-threads=1
cargo clippy -p grok-computer-use-core --all-targets --features test-support --offline --target-dir target-cu-review -- -D warnings
```

退出条件：全部 exit 0；core 测试至少保持 177 项；没有新增 lint allow。若 App check 仍只有审计中已有 warning，逐条记录，不把 warning 写成 passed。

## 5. R1.3b：跨语言基线复核

R1.3a 绿后重跑，确认 Rust 格式改动没有破坏其他层：

```powershell
cd H:\aicoding\grok-app-computer-use
node --test tools\computer-use-mcp\protocol.golden.test.mjs
pnpm exec vitest run src\lib\computer-use
node --check tools\computer-use-browser\server.mjs
node --test tools\computer-use-browser\profile.test.mjs
```

退出条件：4/4、15/15、syntax exit 0、2/2。完成后把 R1 总体标为 passed（E1/E2 混合），而不是“产品完成”。

## 6. R2.1：定义 typed Browser 错误契约

### Red 测试先行

新增 Rust 与 Node 契约测试，至少证明：

1. 401/403/409/422/500 的 HTTP status 不丢失。
2. error code 不靠英文 message 解析。
3. `completion` 只接受 `not_started` 或 `unknown`。
4. 连接拒绝、读超时、worker 退出默认是 `unknown`，除非能证明请求未派发。
5. 4xx 也不能自动等于 `not_started`；以显式 envelope 为准。
6. 未知字段可以兼容读取，但缺少必需字段 fail-closed。

### 实现约束

- Rust 定义权威 `WorkerError`/`WorkerCompletion` 类型。
- `playwright_worker.rs::post_headers` 返回结构化成功或错误，保留 status、code、completion、message、可选 current page generation。
- Node worker 所有错误走同一 JSON envelope；不得一部分 `throw Error`、一部分裸字符串、一部分自定义 JSON。
- 日志可以保留脱敏 message，但 Broker 决策只能看 typed 字段。
- 不能因这一项顺手实现 R4 的 Broker ledger。

### 退出条件

- Rust/Node round-trip 和负向测试全绿。
- 现有 core、MCP golden、Node tests 不回退。
- 人工 diff review 确认没有 URL query、表单值、token、cookie 或本地 profile path 进入错误日志。

## 7. R2.2：页面、观察与节点身份

### 权威身份

- `tabId`：Host 暴露给模型的稳定 ID。
- `pageId`：worker 私有路由 ID，模型不得提供或替换。
- `pageGeneration`：主 frame 导航、reload、进程重建后变化。
- `snapshotId`：每次成功 observe 新生成。
- `elementRef`：只在 `{tabId,pageGeneration,snapshotId}` 内有效的 opaque ID。

### Red 测试

- 两个 tab 的 elementRef 不互通。
- 同 tab 新 observe 后旧 elementRef 失效。
- 导航后旧 pageGeneration/snapshot/elementRef 全拒绝且零副作用。
- tab close 后不能自动落到“当前 tab”。
- popup 有独立 pageId/tabId/generation。
- 模型传入 selector/pageId/profile 被 schema 拒绝。

### 实现

在 Rust 形成单一 `BrowserObservation` 与 `BrowserActRequest`；Node 响应同步。节点提取必须有数量、深度、文本和总字节上限，并带 `truncated` 标记。selector 只存在 worker 私有映射，不能出现在模型 schema 或日志。

## 8. R2.3：actionId 状态机

### 状态与 fingerprint

fingerprint 至少覆盖 tab、page generation、snapshot、action、target 和 parameters。

- 新 actionId：进入 pending，至多执行一次。
- 相同 actionId + 相同 fingerprint + done/rejected/unknown：回放原结果。
- 相同 actionId + 不同 fingerprint：rejected，零执行。
- pending 重试：返回 in-flight，不启动第二次。
- unknown：同 actionId 永远保持 unknown；新写动作前必须重新 observe。

### 测试

使用真实计数器/表单后置条件，不只断言返回 JSON。worker 重启后的跨进程幂等属于 R4 Host ledger，本项只锁 worker 契约，不能冒充 R4 完成。

## 9. R3.1：修复 browser fixture 基础协议

### 修改 `run-fixture.mjs`

- 为每次运行生成随机 Bearer token并传给 worker。
- 所有请求发送 `Authorization: Bearer ...`；任何输出不得打印 token。
- `/open` 后保存真实 pageId/pageGeneration。
- 所有定向请求使用返回 identity；所有写请求使用唯一 actionId。
- navigation 后更新 generation。
- shutdown 后等待 worker 与 Chromium 子孙退出。
- 临时文件只写 `tools/computer-use-browser/.run/` 或系统临时目录。

先让旧 fixture 在当前协议上恢复，再添加新行为；不要通过关闭 token 校验让测试通过。

## 10. R3.2：observe screenshot、ARIA 与 opaque refs

- 调用受限 `page.screenshot({ type: "png" })`。
- 同时限制像素尺寸和编码字节；超限时缩放/拒绝要有明确契约。
- 返回 PNG base64、width、height、page identity、snapshotId、truncated 标记。
- 提取有界 ARIA/可交互节点，并用私有 selector map 生成 opaque elementRef。
- iframe 按 frame identity 组织；cross-origin 能力不足时明确报告，禁止绕过同源限制。
- screenshot 不可用时 observation 明确为 text-only；后续坐标动作必须拒绝。

至少验证：PNG signature、width/height、像素非空、ARIA 文本、截断、旧 refs 失效、无 base64 泄入文本日志。

## 11. R3.3：typed actions

顺序实现并逐项测试：click → fill/set_value → type_text → select → key → scroll → drag → wait → navigate。

每个写动作都必须：

1. 在副作用前验证 owner/profile/page/generation/snapshot/elementRef/actionId。
2. 在副作用前检查 AbortSignal。
3. 只通过 opaque ref 找 worker 私有 locator；不接受任意 selector/evaluate/CDP。
4. 执行后读取真实页面状态作为后置条件。
5. 再检查 cancellation 和 generation。
6. 返回 typed outcome 与当前 identity。

文件上传和下载仍只使用 run staging；模型不能传任意本地路径。

## 12. R3.4：真实 fixture 矩阵

必须覆盖并分别命名：

1. 双 tab 定向隔离。
2. popup 独立 identity。
3. iframe 与 cross-origin 边界。
4. navigation 使旧 identity 失效。
5. actionId same/same、same/different、different/same。
6. pending 与 unknown 不重放。
7. stale snapshot/elementRef 零副作用。
8. slow action cancel 后无提交后置条件，或明确 unknown。
9. run A staging 不能被 run B 上传。
10. 下载重定向、保留名、超限文件不落盘。
11. traversal owner/profile 通过真实 HTTP 请求拒绝。
12. shutdown/crash 后 worker 与 Chromium 无残留。

最终门禁：

```powershell
cd H:\aicoding\grok-app-computer-use
node --check tools\computer-use-browser\server.mjs
node --test tools\computer-use-browser\*.test.mjs
node tools\computer-use-browser\run-fixture.mjs

cd H:\aicoding\grok-app-computer-use\src-tauri
cargo fmt --all -- --check
cargo clippy -p grok-computer-use-core --all-targets --features test-support --offline --target-dir target-cu-review -- -D warnings
cargo check -p grok-computer-use-core --features test-support --offline --target-dir target-cu-review
cargo test -p grok-computer-use-core --lib --features test-support --offline --target-dir target-cu-review -- --test-threads=1
cargo test -p grok-computer-use-core --test driver --features test-support --offline --target-dir target-cu-review -- --test-threads=1
cargo check -p grok-app --offline --target-dir target-cu-review
```

Browser fixture 至少连续运行 3 次。每次必须记录退出码、耗时、页面计数/文本、下载字节、worker/Chromium 进程退出后置条件。一次 flake 后重跑通过不能抹去 flake，必须记录并定位。

## 13. 本批停止条件与交付

R3 全绿后停止，不进入 R4。交付报告必须包含：

- R1.3、R2、R3 每个原子项的实现状态与证据等级。
- 所有命令、exit code、测试数量和真实后置条件。
- 失败/flake 的首次记录与最终状态。
- 安全复核：身份、授权、取消、重放、staging、日志泄漏。
- 完整 `git status --short --branch` 和 `git diff --check`。
- 修改文件清单和每个文件的职责。
- 明确列出仍为 not_run：R4–R9、Windows 安装版 E4、Existing Tabs E4、WebView E4、macOS、Linux、四 target packaging/E5。

不得写“Computer Use 完成”。只允许写“R1.3–R3 在某证据等级通过”。
