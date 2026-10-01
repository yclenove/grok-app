# 交给 Grok 的 Computer Use B1 长任务提示词

日期：2026-09-10  
用途：将下面正文直接粘贴到 Grok App 的目标任务模式。若在 Grok Build 交互终端使用，
才在正文最前添加 `/goal`。

```text
在 H:\aicoding\grok-app-computer-use 的 feat/computer-use-implementation 分支继续
Computer Use 开发。你本次只执行 B1“Windows 发行 Browser runtime 真闭环”，B1
全部门禁通过后立即停笔并形成报告；不要进入 B2、真实模型 E4、Existing Tabs、WebView、
macOS、Linux、S11/S12、commit、push 或 PR。

你接手的是一个大型共享未提交工作树，不是新项目。2026-09-10 审查时 HEAD 为
30757366a739ec9aaf0ccc95bbb3efe19a067aa9，约 56 个 tracked modified、246 个
untracked；数量只是快照，以现场 git status 为准。所有现有修改都要保留。禁止初始化、
推倒重来、reset、clean、checkout --、stash、强切分支、覆盖或删除不属于当前原子项的
文件。

一、开始前必须完整读取

1. AGENTS.md
2. docs/llm-wiki/computer-use.md
3. docs/plans/2026-09-10-computer-use-grok-remaining-roadmap.md
4. docs/plans/2026-09-10-computer-use-grok-runtime-pack-execution.md
5. docs/plans/2026-09-09-computer-use-execution-state.md 的最后 260 行
6. docs/plans/2026-09-09-computer-use-grok-step-plan.md 的 S2、S7、S10、S12
7. src-tauri/computer-use-core/src/runtime.rs
8. src-tauri/src/computer_use/runtime.rs
9. src-tauri/src/computer_use/browser_supervisor/process.rs
10. src-tauri/src/computer_use/browser_supervisor/handshake.rs
11. src-tauri/src/computer_use/browser_managed_contract.rs
12. src-tauri/resources/computer-use/seed/manifest.json
13. src-tauri/resources/computer-use/pack-targets.json
14. tools/computer-use-browser/server.mjs 及其全部 production imports
15. src-tauri/tauri.conf.json 的 resources 配置

旧的 R1.3、R2.2–R3、R4–R9 提示词全部只作历史，禁止按其旧指针返工。状态冲突时，
以 execution-state 末尾、remaining-roadmap 和 runtime-pack-execution 为准。

二、当前可信 checkpoint

- Rust core 236/236、Browser Node 76/76、MCP 5/5、前端定向 120/120 已在
  2026-09-10 现场复核通过。
- typecheck、ESLint、App cargo check 通过；App check 只有 3 个既有 dead_code warning。
- 开发态 BrowserSupervisor → 系统 Node → tools/computer-use-browser/server.mjs →
  真实页面 open/observe/act/verify/PNG/shutdown 通过。
- official Node 20.18.0 Windows PE 和 playwright-core 1.48.0 tgz 的现有 hash 正确。
- 这些 Green 不证明发行 Browser runtime。

当前 P1 Red：

1. src-tauri/resources/computer-use/seed/playwright/worker.mjs 仍只有一行
   export const grokComputerUsePlaywrightWorker = true;，不会发布端口。
2. playwright-core-1.48.0.tgz 只被复制，没有展开为 Node 能 import 的
   node_modules/playwright-core。
3. product_spawn_request() 从 tgz parent 猜 worker.mjs，不独立 resolve/verify worker。
4. required_runtime_components() 没有 browser-worker/Chromium，diagnose 会产生假健康。
5. 现有 browser-managed-contract 依赖 GROK_CU_NODE_FILE 和源码 server.mjs，只是开发态。
6. tools/computer-use-browser/package.json 当前是 playwright-core@^1.63.0，发行 archive 是
   1.48.0；76 项 Node Green 跑的不是发行 pin。

不要再只验证 tgz hash、Node --version 或源码 fixture；本批必须让 product path 使用
发行资源真实启动。

三、单 writer 和执行循环

这个共享工作树只能有一个代码 writer。不要启动多个子代理并发修改代码或执行账本；
只读搜索和互不争抢构建目录的测试可以并行。execution-state 只由你追加。

每次只做一个原子项，严格按：

Read → State → behavior Red → Implement → Targeted Green → Diff review
     → Regression gate → Record

每项开始先向 execution-state 追加 in_progress、预计文件、具体行为 Red、明确不做；
结束再追加实现状态、验证状态、命令、exit code、测试数/耗时、真实后置条件、证据等级、
首次失败/flake、未验证项和下一项。当前项不绿不得进入下一项。

编译失败或 MODULE_NOT_FOUND 只算 bootstrap Red。每个安全/行为契约还要有对应的具体
assertion Red。已有覆盖的契约可记 already-covered，禁止故意破坏代码制造 Red，也禁止
复用旧 Red 冒充新测试。

四、严格执行以下顺序

B1.0 建立真实 packaged contract Red

- 新增独立 gate，建议名 browser-packaged-contract。
- 随机隔离 GROK_APP_HOME，清空/decoy PATH，清除 GROK_CU_NODE_FILE、NODE_PATH、npm
  相关解析变量。
- 设置 API key/proxy sentinel，后续证明不进入 worker/browser。
- gate 必须调用 repair_from_install、product_spawn_request、BrowserSupervisor；禁止手工传
  tools/server.mjs、系统 node.exe 或根 node_modules。
- 当前应因 worker 不发布端口、Playwright 不可 import 或 browser component 缺失而 Red。
- 记录 exact failure、PID 退出和 temp cleanup。本项只建立 Red，不修实现。

B1.1 runtime pack v2 契约

- 区分 js-runtime、playwright-archive、materialized playwright-runtime、browser-worker
  entry/modules、chromium 和 MCP components。
- pack schema version 与 wire protocol 分开。
- 文件组件记录 version/arch/relpath/size/SHA-256；目录使用稳定 path/size/hash 内容清单
  或等价 tree digest。
- browser-worker、Playwright runtime、Chromium 全部进入 required/diagnose。
- Windows node 使用平台原生 bin/node.exe；逻辑 id 可继续是 js-runtime。
- product_spawn_request 分别 resolve node、worker 和 browser，不再由 tgz parent 猜路径。
- 缺失、占位、错 hash、错版本/架构、不可执行、import 失败都返回稳定 typed issue。
- 先写 placeholder worker、缺 sibling、tgz 未展开、browser 缺失和 PATH/source fallback 的
  Red 测试，再实现。
- Playwright 只保留一个 exact pin。默认对齐到已审计的 1.48.0 并移除开发依赖的 ^；如果
  真实行为 Red 证明必须升级，先停下记录决策，再同时更新 archive/hash、Chromium revision、
  lock、NOTICE 和测试，禁止开发 1.63/发行 1.48 双轨。

B1.2 安全物化 playwright-core@1.48.0

- 只使用现有官方 tgz，SHA-256 必须是
  60cbf41da4e72847064ad7c720088dd133cf8601f7af9d5b1b73751d6e649562。
- 禁止 npm install、postinstall、生命周期脚本或在线解析 latest。
- 优先在受信任的 build prepare 阶段展开到 staging，生成逐文件内容清单和待打包的只读
  seed tree；安装/Repair 只校验并原子复制物化 tree。只有既有打包链确实无法携带该 tree
  时才采用运行时解包，并写 ADR 说明原因与攻击面。
- 只接受 package/ 根；拒绝 absolute、..、盘符、UNC、ADS、NUL、symlink、hardlink、
  device、FIFO、重复路径和逃出 staging 的 canonical path。
- 设文件数、单文件、总字节、路径长度和压缩比上限；截断、额外根和版本不符均失败。
- 生成/验证内容清单；package name/version 必须是 playwright-core/1.48.0。
- 正常路径用 App-owned Node 在空 PATH、无 NODE_PATH 时成功 import 并报告版本；不得从
  仓库根 node_modules 命中。
- 失败清 staging，不改 active/previous；覆盖 traversal/截断/超量/错 hash/rollback。

B1.3 打包真实 production worker

- tools/computer-use-browser 保持权威源码。
- 增加确定性 prepare/check 脚本；显式 allowlist 复制 server.mjs 及全部 production sibling
  imports 到 seed，并生成文件 hash/内容清单。
- --check 必须只读并在源码/资源/manifest 漂移时失败。
- 产物不得包含 *.test.mjs、fixtures、.run、profile、日志和 probe-only route。
- 禁止保留一行 worker 占位或人工复制后无同步门禁。
- worker 的 playwright-core 只能解析 active pack 内 materialized package。
- stdout 只先发严格 port/handshake JSON；环境使用最小 allowlist，secret/proxy sentinel
  不得进入子进程。
- production 不得重新出现 /state、/marker、/crash、任意 selector/evaluate/CDP 后门。
- 删除/篡改一个 sibling 时，prepare --check 和 runtime diagnose 都必须 Red。
- 不把 Node/Playwright/Chromium 大二进制直接加入 Git，除非维护者明确批准 Git/LFS。
  提交小型 exact runtime lock/source manifest；生成 seed 必须 gitignored、可复现、受 hash
  约束。正式 Tauri 构建前强制 --check，缺失/陈旧即失败。

B1.4 固定 App-owned Chromium

- 从当前 tgz 的 package/browsers.json 读取权威 metadata；当前预期 Windows Chromium
  revision=1140、browserVersion=130.0.6723.31，但不得从提示词猜 URL/hash/layout。
- 使用固定 Playwright 官方 URL，记录最终 URL、archive hash、展开 tree manifest、
  license/NOTICE、复现命令和包体积。
- 采用与 Playwright 相同的安全 staging、路径/容量检查、原子激活和 rollback。
- Host 显式向 worker 传 resolve 后的 pack browser executable。
- product 不扫描系统 Chrome/Edge；系统浏览器 fallback 只能留在明确的 developer probe。
- profile 必须属于 App 私有 runtime/data 根，绝不复用用户 Chrome/Edge profile。
- PATH/system Chrome decoy 存在时仍要启动 pack Chromium；缺失/篡改/错架构必须 fail closed。
- 若许可或体积需要产品决策，停止并报告 blocked；禁止静默回退系统 Chrome后标 B1 完成。

B1.5 packaged contract Green

继续使用 B1.0 同一 gate，不另写绕过生产路径的替代测试。证明：

1. repair_from_install 从 seed 创建 active pack。
2. product_spawn_request 的 node、worker、Playwright、Chromium 都在隔离 runtime 根内。
3. handshake 返回 protocol/build/source hash、Node/Playwright/browser version、generation/caps。
4. open 隔离 profile，navigate 到随机 loopback 自建页面。
5. observe 得到 snapshot、opaque refs、PNG/geometry。
6. opaque ref action 后由独立 oracle 验证真实页面状态。
7. 再 observe 得到新 snapshot并验证状态。
8. shutdown 后 worker、Chromium及子孙 PID 全退出。
9. temp/staging 按策略清理，用户目录/浏览器不受影响。
10. sentinel secret、proxy、源码绝对路径不出现在响应、trace、stdout/stderr。

连续三次 Green。任一 flake 必须保留首次记录、定位修复，并从第 1 次重新计数。

B1.6 损坏/repair/rollback

覆盖 Node、worker entry/sibling、Playwright archive/runtime、Chromium 的缺失、占位、错 hash、
错版本/架构；staging 中断、active pointer 部分写、previous 缺失；worker timeout/崩溃/
handshake mismatch；repair 成功、同版本幂等、失败不破坏 active、rollback 成功。

每种情况都必须有稳定 typed issue/code 和可行动文案，不泄漏 profile、Token、Cookie、URL
query 或 secret。不能把“重装”当成所有错误的唯一分类。

B1.7 完整门禁

严格执行 runtime-pack-execution.md 第 5 节和第 6 节的全部门禁。至少包括：

- runtime prepare --check；
- Browser Node 全套；
- runtime Rust tests；
- browser-packaged-contract 连续三次；
- MCP、前端定向、typecheck、lint；
- Rust fmt、core clippy -D warnings、core lib、driver、App cargo check；
- git diff --check 与 git status；
- production pack 内容、import 图、required components、系统 Node/Chrome/source fallback、
  selector/evaluate/test route 和 secret 静态检查。

不要让多个 cargo 命令并发写同一个 target-cu-review，锁等待不算代码故障但会污染耗时。
Windows App lib test 若遇 0xc0000139，不得把环境入口点错误写成断言通过；原生产品 gate
使用 cu_probe 独立 bin。

五、不可违反的边界

- 不碰 H:\aicoding\grok-app 或其他 worktree。
- 不碰用户账号、Token、Cookie、代理、共享 ~/.grok、正式安装、真实 Chrome/Edge profile、
  ChatGPT/微信窗口。
- 测试仅用随机 loopback/Bearer、隔离 GROK_APP_HOME/profile/staging、自建 fixture、
  target-cu-review 和 Rust --offline。
- 不硬编码 10808/10809，不改系统代理。
- 不运行根 npm install/yarn，不依赖 PATH Node、全局 npm、系统 Chrome 或在线 latest。
- 不加 lint allow、skip/ignore、宽松断言或 production 测试后门。
- 不向 src/App.tsx/src/app/AppWorkbench.tsx 增加 Computer Use 状态或大型逻辑。
- 不做 UI 美化、平台动作、Existing Tabs/WebView、真实模型 E4、S11/S12。
- 不 commit、push、PR、merge、tag、release。

六、完成语义与停笔

只有 packaged contract 真正使用发行资源、连续三次 Green，损坏/repair/rollback 和所有回归
门禁通过，且没有系统依赖、secret 泄漏或残留进程，才允许写：

“B1 Windows packaged Browser runtime passed（E2/E3）。”

不得写“Computer Use 完成”“Windows 完成”“三平台完成”“可发布”。安装 App + 真实模型
仍是 E4 not_run；Existing Tabs/WebView、macOS/Linux、S11/S12、四 target E5 仍未完成。

B1 完成后立即停止修改，输出：一句话结果、首次 Red、实现文件职责、runtime layout、
Node/Playwright/Chromium 来源版本/hash/license/大小、三次真实链耗时与后置条件、损坏矩阵、
安全审查、全部门禁、flake/warning、git status/diff-check，以及 B2 的精确接力点。等待用户
和 Codex 审查，不要自行进入下一批。

现在先完成全部 Read，检查现场 git status/HEAD 和 execution-state 末尾。然后只追加
“B1.0 packaged contract Red 开始”记录，只建立真正走 repair_from_install +
product_spawn_request 的隔离探针。看到占位 worker/未物化 Playwright/缺 browser 的真实失败并
完成清理证据后停在 B1.0 的 Red 记录，再进入 B1.1；不要先改 seed 或 runtime 实现掩盖首个 Red。
```
