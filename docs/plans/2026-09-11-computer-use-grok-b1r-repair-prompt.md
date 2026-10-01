# 给 Grok 的长任务提示词：Computer Use B1-R 交付修复

把下面代码块完整粘贴给 Grok。不要删掉范围、证据或停笔条件。

```text
你在 H:\aicoding\grok-app-computer-use 的 feat/computer-use-implementation 分支继续
Computer Use。你现在执行的是 B1 审查后的 B1-R 修复批次，不是 B2。

本次是一个长任务：按 B1-R.0 到 B1-R.7 自动连续推进。每个原子项 Green 后直接进入下一项，
不需要等待用户逐项回复；只有出现执行书定义的阻塞或需要产品/发布架构决策时才停止。全部
B1-R 门禁通过后立即停笔、形成报告，绝对不要进入 B2。

一、现场与保护边界

- 2026-09-11 审查基线 HEAD：30757366a739ec9aaf0ccc95bbb3efe19a067aa9。
- 分支：feat/computer-use-implementation。
- 审查时约 56 个 tracked modified、267 个 untracked；数量只是快照，以现场为准。
- 这些改动属于一个大型共享未提交工作树，必须全部保留。
- 禁止 reset、clean、checkout --、restore、stash、强切分支、推倒重来、覆盖或删除不属于
  当前原子项的文件。
- 禁止 git add、commit、push、PR、merge、tag、release。
- 只允许一个代码 writer；不要并发安排其他代理修改同一工作树或 execution-state。
- 不碰 H:\aicoding\grok-app 及其他 worktree。
- 不碰账号、Token、Cookie、代理、共享 ~/.grok、正式 Grok App、用户 Chrome/Edge profile。
- 测试只用随机 loopback/Bearer、隔离 GROK_APP_HOME、隔离 profile/staging/cache 和自建 fixture。
- 不直接运行无扩展名 js-runtime；Windows 只运行受校验的 node.exe。

二、开始前必须完整读取

1. AGENTS.md
2. docs/llm-wiki/computer-use.md
3. docs/plans/2026-09-10-computer-use-grok-remaining-roadmap.md
4. docs/plans/2026-09-10-computer-use-grok-runtime-pack-execution.md
5. docs/plans/2026-09-09-computer-use-execution-state.md 的末尾 B1 记录
6. docs/plans/2026-09-11-computer-use-grok-b1r-repair-execution.md（本次唯一执行书）
7. C:\Users\Administrator\AppData\Local\Temp\grok-goal-166c0b7fc477\implementer\b1-report.md
   和同目录全部 b1.* 原始日志；如果临时报告已不存在，以 execution-state 和仓库现场为准，
   明确记录缺失，不要伪造。

读完后先执行只读现场检查：

- git status --short --branch
- git rev-parse HEAD
- git diff --check
- git check-ignore -v 检查 Node、Playwright tgz/tree、Chromium zip/tree
- 搜索 prepare-computer-use-runtime 在 package.json、Tauri、build.rs、build-local、CI/release
  中的全部引用
- 读取 runtime.rs、playwright_materialize.rs、browser packaged contract、BrowserSupervisor、
  prepare/check 脚本、.gitignore、tauri.conf 和 NOTICE

只有完成全部 Read 和现场检查，才向
docs/plans/2026-09-09-computer-use-execution-state.md 追加：
“B1-R.0 冻结现场与捕获交付 Red 开始”。

三、Codex 审查结论：必须当作待修缺陷，不得争辩后绕过

1. 当前工作机上的 B1 真实链可信：packaged contract 最终三次为
   83642 / 41620 / 42096 ms，Node 20.18.0、Playwright 1.48.0、Chromium
   130.0.6723.31 rev 1140，真实页面和 PID 清理成立。
2. B1 尚不能作为可交付完成接受：正式构建/CI 没有强制 runtime prepare/check。
3. 模拟不含 ignored 产物的干净源码副本时，--check 因缺物化 Playwright tree 失败；
   --prepare 物化 Playwright 后仍因缺 Chromium 失败。当前 prepare 不获取 Chromium。
4. 当前 checkChromium 只重算 chrome.exe hash，然后比较 manifest/lock 中两个预存 tree
   digest；篡改非 exe Chromium 文件仍错误 Green。
5. 没有可复现的 Chromium 安全下载/解包实现；报告中“zip 已安全解包”的说法没有代码证据。
6. node.exe 约 69.8 MB 和 Playwright tgz 约 1.9 MB 未跟踪且未忽略，和原执行书“不把
   大二进制直接加入 Git”冲突；报告称全部大二进制 gitignored 不准确。
7. base tauri.conf 的 resources/computer-use/**/* 会覆盖所有平台，并可能把展开 Chromium
   tree 与 147 MB zip 一起收入包。
8. RuntimeStore::install_from_bundle 看到目标 pack manifest 已存在时会跳过重装；同版本
   active/mcp pack 损坏后 Repair 可能重新激活旧损坏 pack。
9. Host repair_from_install 最后只 diagnose JS_RUNTIME 和 PLAYWRIGHT_ARCHIVE，没有验证全部
   required_runtime_components。
10. Node 分发 NOTICE 仍写着“NOTICE 待补”。
11. 原始 B1 完整门禁首次有 browser/MCP/vitest/clippy 四个命令失败，之后分别重跑 Green；
    B1.2 也有一次错误 cargo 参数 TEST_EXIT=1。最终 Green 可以作为基线，但最终报告必须
    保留首次失败，不能写成一次性全绿。

四、本批唯一目标

修成：从不含任何 generated/ignored runtime 产物的干净源码副本，可以仅凭 tracked 源码、
精确官方 source lock 和允许的构建网络，安全、确定性地生成 Windows x64 runtime seed；
正式 Tauri Windows 构建在 resources 收集前强制完整检查；Windows seed 不进入 macOS/Linux
包；同版本 Repair 能恢复每个 required component；全部 B1 回归不降级。

B1-R 通过只证明 Windows packaged Browser runtime 的源码交付、构建准备、完整性和 Repair
契约 E2/E3。安装 App + 真实模型 E4、Existing Tabs/WebView、macOS/Linux 实现、S11/S12、
四 target E5 仍未完成。

五、必须按以下原子顺序执行

B1-R.0 冻结现场并捕获 Red

- 不改实现，先为以下行为建立自动化 Red/可重复 fixture：
  a) clean-source --check/--prepare 不能复建；
  b) 非 chrome.exe Chromium 文件被篡改而 --check 假 Green；
  c) 同版本 active pack 每个受管组件损坏后 Repair 不恢复；
  d) build/release 路径没有强制 check；
  e) Git ignore/资源 glob 与报告不一致。
- 干净源码模拟必须用 git ls-files --cached --others --exclude-standard 复制到新的专用临时
  目录，不复制 ignored 产物，不修改当前 seed。
- 每个 fixture 记录命令、exit、首个失败、PID/临时目录清理。
- Red 完整后追加 B1-R.0 完成记录，再自动进入 B1-R.1。

B1-R.1 收敛 artifact、lock 与 Git 契约

- 建立一个 Windows x64 权威 source lock：固定版本、target、官方 URL/允许 redirect host、
  archive SHA-256/bytes、archive root、tree SHA-256/files/bytes、executable hash、license。
- 生成的 Node archive/node.exe、Playwright tgz/tree/listing、worker seed 副本、Chromium
  zip/tree、seed manifest、cache/staging 必须全部 gitignored。
- 普通 Git 只跟踪 lock、prepare/check 源码和测试、production worker 权威源码、NOTICE、
  构建接线。未经维护者明确批准，不用 Git LFS，不提交大二进制。
- Rust 常量与 lock 写 golden test，不能多份值静默漂移。
- 新增 git check-ignore 和大文件 inventory 门禁。

B1-R.2 实现 deterministic prepare

- `--prepare --target <target>` 只接受显式支持 target；本批只生成 Windows x64。
- archive cache 放在不会被 Tauri resources 收入的 ignored cache 根。
- cache miss 只从 lock 的精确官方 URL 下载；限制 redirect host、timeout 和最大 bytes；先
  校验 bytes/hash，再原子进入 content-addressed cache。禁止 latest、npm install、postinstall。
- Node/Chromium zip 必须安全解包到随机 staging；Playwright 复用并补齐现有安全 tar
  materializer。
- zip/tar 拒绝 absolute、UNC、盘符、ADS、NUL、.、..、symlink、hardlink、junction/reparse、
  special file、Windows case-fold 重复路径、额外根、截断、超文件数/单文件/总大小/压缩比。
- worker 入口和 sibling 从 tools/computer-use-browser allowlist 生成；无 tests/fixtures/.run/
  日志/probe-only route。
- 所有产物先在一份完整 staging seed 生成和校验，再原子发布。不得先删健康 seed 再 rename。
- 失败删除本次 staging，上一份健康 seed 和 cache 不变。
- 空 cache 首次 prepare 与 cache hit 再次 prepare 的最终 digest 必须一致。

B1-R.3 实现真正只读的 complete check

- `--check --target <target>` 绝对不写文件；测试比较 check 前后递归 path/size/hash/mtime。
- 现场重算 Node/worker/sibling/Playwright tree/Chromium tree 的 hash、files、bytes；拒绝额外
  文件、缺文件、symlink/reparse/special file。
- 不允许只比较 manifest.treeSha256 == lock.treeSha256。
- App-owned Node 在空 PATH、无 NODE_PATH 时 import seed 内 playwright-core 并报告 1.48.0。
- 篡改任意 Chromium 非 exe 文件必须非零；恢复后 Green。
- manifest 必须由实际产物生成并与 lock/源码一致。

B1-R.4 接入 Tauri、本地构建、CI/release 和 target resource map

- 设计一个 Tauri build 前不可绕过的 prepare/check 入口；常用 pnpm build、build-local 和
  tauri-action release 都必须走到。
- CI/release 显式 prepare/check；cache 只是提速，命中后仍完整校验。
- 缺 seed、陈旧 seed、错 target 必须在 bundle 前失败。
- Windows 包只收最终 Windows seed；Chromium zip、download cache、staging、logs、fixtures
  不得进入 resources。
- macOS/Linux 当前不收 Windows seed；保持 default-off/unavailable，不伪装已支持。
- target 必须来自 workflow 显式参数或现场验证过的 Tauri target env，不能把 host 当 target。
- 先用测试确认 Tauri 配置数组 merge/replace 语义，再修改 resources；不要猜。
- pnpm build:ui 仍应独立，不触发 runtime 大下载。

B1-R.5 修复 same-version Repair/rollback

- 复用已有 pack 前完整 diagnose 所有 required_runtime_components、tree 和 sibling。
- exact source/content identity 必须包含 file hashes、tree digests、compatibility、worker modules
  和 embedded MCP；不能忽略 tree 后复用旧 mcp pack。
- 已有 pack 全健康且 identity 完全一致才允许幂等复用。
- 任一组件损坏时，从 installer bundle 构建新的 staging/repair generation，完整校验后原子
  激活；不能因为 manifest.json 存在就跳过复制。
- ensure_embedded_mcp 不得重新激活未经完整校验的旧 MCP pack。
- Host Repair 成功返回前 diagnose 全部 required components，并再走 product paths。
- Repair 失败保持 active/previous/current 不变，清本次 staging。
- rollback 前验证 previous 完整健康；缺失/损坏用稳定 typed issue fail closed。
- active manifest 的 schema、relpath/canonical containment 每次 resolve/diagnose 都校验。
- 测试 Node、worker entry、每个 sibling、Playwright archive/runtime、Chromium 的同版本损坏；
  active pointer 部分写、previous 缺失/损坏、连续 Repair、失败后旧 active 仍可启动。

B1-R.6 补许可和 package audit

- 补 Node 官方 LICENSE/NOTICE；保留 Playwright LICENSE/NOTICE/ThirdPartyNotices；保留
  Chromium CREDITS、来源和版本。
- 更新 docs/plans/NOTICE-computer-use.md，删除“NOTICE 待补”和过期状态。
- 最终 Windows seed inventory 记录 source/version/hash/bytes/license。
- 审计真实 bundle resource list：不得有 Chromium zip/cache/staging/test/fixture/log/.run/用户数据。
- 记录安装包相对主干的体积增量；不得隐瞒，不凭空设置阈值。

B1-R.7 最终门禁和停笔

在最终代码上保存原始日志，至少执行：

1) git diff --check
2) prepare/check read-only/deterministic/危险 archive/篡改测试
3) Browser Node 全套
4) MCP golden
5) 前端 Computer Use/settings/slash 定向测试
6) typecheck、lint、cargo fmt --check、core clippy -D warnings
7) core --lib、runtime、driver、cargo check grok-app
8) same-version Repair/rollback 完整矩阵
9) 当前工作树 browser-packaged-contract 连续三次
10) 不含 ignored 产物的隔离源码副本：空 cache prepare、check、第二次 prepare/check、
    browser-packaged-contract 连续三次
11) Windows Tauri bundle 或真实等价 resources 收集，并审计包内容/体积
12) target map 自动化证明 macOS/Linux 不收 Windows seed；没有对应 OS 实机只能写 not_run

任一 flake 都保留首次失败并定位修复；连续三次要求从第 1 次重新计数。不得只提高 timeout、
只重跑失败用例或把最终分别 Green 写成“一次性全绿”。

六、实现原则

- 优先修根因，不堆一次性 PowerShell/手工下载步骤。
- prepare/check 需要稳定 CLI、清晰 exit code 和可测试模块，不能只写一个巨大脚本。
- 下载/解包/发布/Repair 采用 staging + validate + atomic activation；每个失败路径都有后置断言。
- 产品 BrowserSupervisor 继续只接收 pack 内绝对 node/worker/browser；禁止 PATH/system fallback。
- production worker 继续禁止 selector/evaluate/CDP、/state、/marker、/crash 测试后门。
- 不向 App.tsx 或 AppWorkbench.tsx 增加状态/大型逻辑。
- 不用 mock/stub/fake 证明 production packaged contract。
- 不因当前机器已有 500 MB seed 就跳过 clean-source 复建。

七、阻塞条件

如果需要下列任一决定，停止修改并形成 blocked 报告，不要自行选择：

- 改 pin 或官方 URL/hash 无法核实；
- 引入 Git LFS、私有制品、长期凭据；
- Tauri 不能按 target 隔离资源，需要改变发布架构；
- 必须触碰正式安装、用户浏览器/账号/Token/Cookie/代理；
- 需要进入 B2、Existing Tabs/WebView、macOS/Linux 动作实现才能继续。

blocked 前至少做三次不同的安全尝试，报告 exact error、方案、风险和唯一待决问题。禁止用
系统 Node/Chrome fallback、skip、ignore 或假 Green 解阻塞。

八、execution-state 与最终报告

- 每个 B1-R.x 开始和结束各追加一条：实现状态、验证状态、Red、Green、命令、exit、测试数、
  耗时、证据路径、flake、未验证、下一项。
- 不改写旧记录，不把计划写成已完成。
- 全部 Green 后追加“B1-R 完成 / 停笔”，然后停止，不进入 B2。
- 最终报告必须包含：一句话 passed/partial/blocked；五类 Red->Green；tracked/generated/cache/
  package 分界；source/version/hash/bytes/license；prepare/check/build/target map；Repair 矩阵；
  clean-source 两次 digest；当前与干净副本 packaged contract 六次耗时及 PID/文件后置；包内容/
  体积；全部门禁和首次 flake；git status/diff-check；所有 E4/E5 未完成项。
- 未 commit、未 push、未 PR。

现在先完成全部 Read 和只读现场检查，然后只追加“B1-R.0 开始”，捕获五类 Red。不要先改
prepare、ignore、runtime 或 seed 来掩盖审查失败。B1-R.0 Red 完整后按执行书自动连续推进到
B1-R.7；全绿后停笔等待用户和 Codex 审查。
```
