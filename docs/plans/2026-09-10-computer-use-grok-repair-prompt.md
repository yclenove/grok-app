# 交给 Grok 的 Computer Use 下一批长任务提示词

> [!CAUTION]
> 本提示词的 R1.3 起点已经过期，禁止再次直接交给 Grok。当前使用
> [B1 发行 Browser Runtime 长任务提示词](2026-09-10-computer-use-grok-runtime-pack-prompt.md)。

下面正文用于 Grok App 的目标任务模式。若在 Grok Build 交互终端使用，才在正文前加 `/goal`。

```text
在 H:\aicoding\grok-app-computer-use 的 feat/computer-use-implementation 分支继续 Computer Use 开发。

你接手的是有大量未提交实现、并且正停在 R1.3 半成品重构中的工作树。不要初始化新项目，不要回到 S0，不要新建平行架构，不要清空、reset、stash 或覆盖现有代码。

开始前必须完整读取：
1. AGENTS.md
2. docs/llm-wiki/computer-use.md
3. docs/plans/2026-09-10-computer-use-codex-audit.md
4. docs/plans/2026-09-10-computer-use-grok-repair-plan.md
5. docs/plans/2026-09-10-computer-use-grok-next-batch.md
6. docs/plans/2026-09-09-computer-use-execution-state.md
7. docs/plans/2026-09-09-computer-use-grok-step-plan.md

当前可信 checkpoint：
- 分支 feat/computer-use-implementation，基线 HEAD 30757366a739ec9aaf0ccc95bbb3efe19a067aa9；全部 Computer Use 改动仍未提交。
- R0.1 已完成，不要重跑或重写。
- R1.1 已完成：旧 Browser 测试已按 managed/user-owned 边界修正，当时 core lib 177/177 通过。
- R1.2 已完成：computer_stop 已移入 TypeScript model tools 并移出 Host-only；现场 MCP golden 4/4、TS Computer Use 15/15。
- UI/i18n/settings 现场 117/117；Node syntax exit 0、profile tests 2/2。
- R1.3 正在进行且当前代码不能编译。已存在 ManagedPageRef、ManagedWorkerAction、ManagedWorkerUpload、ManagedTabAction、SharedTabOffer、TabAttachment；保留并完成它们，不要删掉重写。
- core cargo check 当前首先失败在 src-tauri/computer-use-core/src/broker/gates.rs 的旧式 10 参数 share_and_grant。
- 另有旧调用：broker/tests_lease_schema.rs 一处、src-tauri/src/computer_use/extension_pair.rs 两处。
- browser.rs 的 disconnect 分支仍有 needless_bool_assign，应写成 rec.info.closed = !rec.info.user_owned;。
- cargo fmt --all -- --check 当前有 browser.rs 两处、playwright_worker.rs 一处格式差异。
- tools/computer-use-browser/run-fixture.mjs 仍因 GROK_CU_BROWSER_TOKEN required 退出 1；它属于 R3.1，不能通过关闭认证修复。

本次任务只完成 R1.3、R2、R3。R3 全部门禁和三次真实 fixture 通过后停止、形成报告，绝对不要进入 R4，也不要做 macOS/Linux、UI 扩展、runtime pack、安装版 E4、commit、push 或 PR。

严格执行 docs/plans/2026-09-10-computer-use-grok-next-batch.md。每次只做一个原子项，固定循环：
Read → State → Red → Implement → Targeted test → Review → Gate → Record。

每个原子项开始时，在 docs/plans/2026-09-09-computer-use-execution-state.md 追加 in_progress 记录；结束时追加命令、exit code、测试数、真实后置条件、未验证项、安全复核和下一项。当前项不绿就留在当前项修复，不能标 unrelated/baseline 后继续。

第一步只能是 R1.3a：
1. 把上述四处旧 share_and_grant 调用改为显式 TabAttachment { session, run_id, tab_id, title, url, origin, extension_id, pairing_token, home_index, document_generation, connection_generation, focused }。
2. 保持原测试数据和授权语义；不得放宽 user-owned/managed 边界。
3. 修 needless_bool_assign，不加 #[allow]。
4. cargo fmt --all 后依次跑 fmt check、core check、App check、core 177+ tests、core clippy -D warnings。
5. 任一命令失败就修到绿，不进入 R1.3b。

R1.3b：重跑 MCP golden 4/4、TS Computer Use 15/15、Node syntax、profile 2/2。全部通过后才把 R1 标为 passed；这只算 E1/E2 基线，不是产品完成。

R2 必须按以下顺序：

R2.1 typed error envelope：
- Rust 是权威类型；Node 同步。
- 保留 HTTP status、稳定 error code、completion=not_started|unknown、message、可选 currentPageGeneration。
- 不能解析英文 message 决策；不能把所有 4xx 当 not_started；timeout/断线/worker exit 默认 unknown，除非能证明未派发。
- 先写 Rust/Node Red，覆盖 401/403/409/422/500、缺字段、非法 completion 和网络失败，再实现。

R2.2 identity：
- tabId 是 Host/模型稳定标识；pageId 是 worker 私有路由，模型不能传。
- pageGeneration 在主 frame 导航/reload/进程重建后变化。
- 每次 observe 生成 snapshotId；elementRef 只在 tabId+pageGeneration+snapshotId 内有效。
- 双 tab、popup、导航、close、新 observe、旧 ref 必须有零副作用负向测试。
- 模型 schema 禁止 selector、pageId、profile、evaluate、CDP、shell、Cookie、Token、storage。

R2.3 actionId：
- fingerprint 覆盖 tab、page generation、snapshot、action、target、parameters。
- same id + same fingerprint 回放原结果；same id + different fingerprint 拒绝；pending 不二次执行；unknown 不自动重放。
- worker 重启后的 Host 级幂等属于 R4，本批不能谎称已解决。

R3 必须按以下顺序：

R3.1 fixture 协议：
- run-fixture 每次生成随机 Bearer token，传给 worker并发送 Authorization；任何日志不打印 token。
- /open 后保存真实 pageId/pageGeneration；每个定向请求带 identity；每个写请求带唯一 actionId；导航后更新 generation。
- shutdown 后验证 worker 与 Chromium 子孙退出。
- 不能删除或绕过 token 校验。

R3.2 observe：
- 返回有界 PNG screenshot、width/height、ARIA/交互节点、snapshotId、opaque elementRef、truncated 标记。
- 同时限制像素与字节；base64 不进入 text/log。
- screenshot 缺失时明确 text-only，坐标动作必须拒绝。
- selector 映射只在 worker 内部，cross-origin iframe 不绕权限。

R3.3 typed actions：
- 按 click、fill/set_value、type_text、select、key、scroll、drag、wait、navigate 逐项实现和测试。
- 副作用前校验 owner/profile/page/generation/snapshot/ref/actionId 并检查 AbortSignal。
- 执行后验证真实页面状态，再检查取消和 generation，返回 typed outcome。
- 禁止任意 evaluate/CDP/shell/file URL；上传下载只走 run staging。

R3.4 fixture matrix：
- 双 tab、popup、iframe、导航代际、三种 actionId 组合、pending/unknown、stale ref、slow cancel、跨 run staging、下载重定向/保留名/超限、traversal、shutdown/crash 无残留必须分别命名和验证。
- 不只看 HTTP 200；检查页面计数/文本、文件字节、进程退出。
- 最终 browser fixture 连续跑 3 次，每次记录耗时和真实后置条件。出现一次 flake 后重跑通过，也必须保留首次失败并定位。

真实性红线：
- mock/jsdom 只算 E1；loopback/真实子进程通常只算 E2；自建真实页面/原生 fixture 才可能算 E3；已安装 App + 真实 Grok 模型才算 E4。
- Windows fixture 不能证明 macOS/Linux；CFT/Browser 不能证明 native Wayland；cross-compile 不能冒充实机。
- 当前 26-byte js-runtime 和 53-byte Playwright seed 仍是 placeholder，不得报 healthy。
- Existing Tabs extension transport、Windows 安装版、WebView 产品面、macOS/Linux、四 target packaging/E5 全部保持 not_run。

工程红线：
- 不触碰 H:\aicoding\grok-app、其他 worktree、stash、账号、Token、Cookie、共享 ~/.grok、系统代理、正式 Grok 安装数据或用户真实 Chrome/Edge。
- 测试仅用隔离 GROK_APP_HOME、profile、随机 loopback 端口、自建 fixture、专用 target-cu-review。
- 根目录只用 pnpm，不运行 npm install/yarn。
- 不向 src/App.tsx 或 src/app/AppWorkbench.tsx 添加 Computer Use state/大型逻辑。
- 不新增硬编码 UI 文案、window.confirm/prompt/alert、裸 select、透明菜单。
- 不加 lint allow、skip/ignore，不降低规则，不改成宽松断言。
- 未经用户另行明确授权，不 commit、push、PR、merge、tag、release。

本批最终输出必须包含：
1. R1.3、R2、R3 每项实现状态和 E0–E3 证据等级。
2. 全部命令、exit code、测试数、三次 fixture 的耗时与真实后置条件。
3. 所有首次失败/flake 与最终处理，不得只留最后一次绿。
4. 身份、授权、取消、幂等、staging、日志泄漏安全复核。
5. git status --short --branch、git diff --check、修改文件及职责。
6. 明确未完成的 R4–R9、E4/E5 和三平台事项。

只允许说“R1.3–R3 在具体证据等级通过”，不得说“Computer Use 已完成”。现在从 R1.3a 开始。
```
