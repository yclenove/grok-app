# Computer Use：Grok B1 发行 Browser Runtime 详细执行书

日期：2026-09-10  
工作区：`H:\\aicoding\\grok-app-computer-use`  
分支：`feat/computer-use-implementation`  
本批范围：只完成 B1；B1 全绿后停笔并交报告，不进入安装版模型 E4、其他平台或 PR  

## 1. 本批目标

修正当前“开发态 Browser 真实链通过，但发行资源仍不可启动”的断层。

B1 的最终后置条件是：

> 从仓库模拟的安装资源开始，在随机隔离 `GROK_APP_HOME`、空 PATH、无
> `GROK_CU_NODE_FILE`、无系统 Node/全局 npm 包的条件下，RuntimeStore 完成安装/修复，
> App 通过 `product_spawn_request()` 启动 App-owned Node、生产 worker、固定
> Playwright 和固定浏览器，连续三次完成 open → navigate → observe → opaque-ref act →
> observe verify → PNG → shutdown，且无 worker/browser PID 或临时文件残留。

B1 通过只代表 Windows 发行 Browser runtime 达到 E2/E3。没有安装版 App + 真实
Grok 模型，仍不得标 E4 或 Computer Use 完成。

## 2. 已知 Red 与不能误判的 Green

### 2.1 已知 P1 Red

- seed `playwright/worker.mjs` 只有一行占位导出，不会发布端口。
- seed `playwright-core-1.48.0.tgz` 没有被物化为 Node 可解析的模块目录。
- `product_spawn_request()` 通过 tgz 父目录猜 `worker.mjs`，没有独立解析并校验
  browser worker component。
- `required_runtime_components()` 不要求 `browser-worker` 或 Chromium。
- diagnose 只验证文件 hash/版本，不能证明 worker 可启动、Playwright 可 import、
  浏览器可启动。
- 开发 worker 使用 `playwright-core@^1.63.0`，发行 archive 为 `1.48.0`；现有 Browser
  测试跑的是开发依赖，尚未证明发行 pin 兼容。

### 2.2 当前 Green 的准确含义

- `browser-managed-contract` 使用 `GROK_CU_NODE_FILE` 与源码目录
  `tools/computer-use-browser/server.mjs`；这是开发态 E2/E3。
- official Node PE 和 Playwright tgz 的 hash/版本正确；只证明供应链输入文件。
- `isolated-runtime-repair` 只证明复制/resolve 与 Node `--version`；不证明生产 worker。

禁止复用这些 Green 冒充 packaged contract。

## 3. 固定执行纪律

每个原子项严格执行：

```text
Read → State → behavior Red → Implement → Targeted Green
     → Diff review → Regression gate → Record
```

- 共享工作树只允许一个代码 writer；不得启动多个子代理并发改代码或账本。
- 每次只处理本文件第 5 节中的一个编号；当前编号未绿不得进入下一项。
- 每项开始/结束各向 `2026-09-09-computer-use-execution-state.md` 追加一次。
- 先出现能描述目标行为的具体失败，再改生产代码；旧失败、编译失败或
  `MODULE_NOT_FOUND` 只能算 bootstrap Red，不能覆盖全部行为契约。
- 保留现有全部修改；禁止 reset、clean、checkout --、stash、强切分支。
- 不 commit、push、PR、merge、tag、release。
- 不碰账号、Token、Cookie、代理、共享 `~/.grok`、正式安装或用户浏览器 profile。
- 测试只用自建页面、隔离目录、随机 loopback/Bearer 和专用 target。
- 根目录使用 pnpm；不得通过 `npm install`、全局 npm 或 PATH Node 修好产品链。

## 4. 先读文件

开始 B1 前完整读取：

1. `AGENTS.md`
2. `docs/llm-wiki/computer-use.md`
3. `docs/plans/2026-09-10-computer-use-grok-remaining-roadmap.md`
4. 本执行书
5. `docs/plans/2026-09-09-computer-use-execution-state.md` 最后 260 行
6. `docs/plans/2026-09-09-computer-use-grok-step-plan.md` 的 S2、S7、S10、S12
7. `src-tauri/computer-use-core/src/runtime.rs`
8. `src-tauri/src/computer_use/runtime.rs`
9. `src-tauri/src/computer_use/browser_supervisor/process.rs`
10. `src-tauri/src/computer_use/browser_supervisor/handshake.rs`
11. `src-tauri/src/computer_use/browser_managed_contract.rs`
12. `src-tauri/resources/computer-use/seed/manifest.json`
13. `src-tauri/resources/computer-use/pack-targets.json`
14. `tools/computer-use-browser/server.mjs` 及其所有 production imports
15. `src-tauri/tauri.conf.json` 的 resource 配置

读完先记录实际 `git status`、HEAD、资源文件尺寸/hash、生产 worker import 图、
manifest 组件和产品启动路径。不要以本文的文件数量替代现场状态。

## 5. 原子执行顺序

### B1.0：冻结基线与建立 packaged contract Red

#### 目标

新增一个真正走产品资源的探针，先证明当前 seed 不能启动。它必须与现有开发态
`browser-managed-contract` 分开命名。

#### 建议入口

- 新 gate：`cu_probe browser-packaged-contract`。
- 测试 helper 可以拆到独立模块，不能把大量逻辑继续塞进 `mod.rs`。
- gate 必须调用生产 `repair_from_install()`、`product_spawn_request()` 和
  `BrowserSupervisor`；禁止手工传源码 `server.mjs`。

#### Red 前置条件

- 随机隔离 `GROK_APP_HOME`。
- 把 PATH 设为只含一个会失败的 decoy，或清空；保存并在 finally 恢复当前进程环境。
- 清除 `GROK_CU_NODE_FILE`、`NODE_PATH`、npm prefix/cache 和会影响模块解析的变量。
- 设置 `OPENAI_API_KEY`、`GROK_API_KEY`、`HTTPS_PROXY` 等 sentinel，后续证明不进入
  worker/browser。
- 只提供当前 seed resource 根。

#### 首个行为 Red

当前应在 worker 未发布端口、Playwright 无法解析或浏览器组件缺失处失败。记录：

- exact command/exit code；
- failure code/message（必须脱敏）；
- worker 是否启动、PID 是否退出；
- 隔离 runtime/profile/staging 是否清理；
- 没有用户浏览器窗口/profile 被触碰。

这一项只建立探针和 Red，不修实现。

### B1.1：定义 runtime pack v2 契约

#### 目标

消除“文件存在就健康”和“由某个组件的 parent 猜另一个入口”的隐式关系。

#### 必需逻辑组件

- `js-runtime`：平台原生可执行文件；Windows relpath 使用 `bin/node.exe`，逻辑 id
  仍可保持 `js-runtime`。
- `playwright-archive`：固定官方 tgz，只是安装输入，不是可运行模块。
- `playwright-runtime`：物化后的 `node_modules/playwright-core`，有明确版本与内容清单。
- `browser-worker`：生产入口文件。
- `browser-worker-modules`：入口依赖的 production-only 模块集合及每文件 hash。
- `chromium`：与固定 Playwright revision 匹配的 App-owned browser executable/tree。
- 现有 MCP server/protocol components。

允许调整逻辑命名，但最终必须区分 archive、materialized runtime、worker entry 和 browser，
不能继续用一个 `PLAYWRIGHT` id 同时代表 tgz、worker parent 和可运行模块。

Playwright 只能有一个精确 pin。默认保持已审计的 `1.48.0`，将开发 worker 的
`package.json`/lock 与发行测试对齐并移除 `^`。如果源码确实依赖更高版本 API，必须先用
行为 Red 证明，再作为独立决定同时更新 runtime lock、archive hash、Chromium revision、
NOTICE 和全部测试；禁止开发 1.63、发行 1.48 的双轨状态。

#### manifest 要求

- schema/pack format version 与 Computer Use wire protocol 分开。
- 每个文件记录 id、version、arch、relpath、size、SHA-256。
- 目录组件使用排序稳定的 path/size/hash 清单或等价 tree digest；禁止只 hash
  `package.json` 后默认其余文件可信。
- relpath 逐段拒绝 `..`、绝对路径、空段、alternate data stream 和跨根路径。
- Host arch 精确匹配；`any` 只给纯 JS/数据，不能给 native browser/runtime。
- `required_runtime_components()` 覆盖 worker 与 browser；缺失、占位、错 hash、错版本、
  错架构、不可执行或 import 失败都不健康。
- `product_spawn_request()` 分别 resolve node、worker entry 和 browser，不再由 tgz 路径
  猜 worker。

#### Red/Green 测试

至少覆盖：

1. 一行 worker 占位文件被 diagnose 拒绝。
2. worker component 不在 required list 时测试失败。
3. tgz 存在但没有 materialized Playwright 时不健康。
4. worker import 缺一个 sibling module 时不健康。
5. Chromium 缺失/错架构/错 hash/不可执行时不健康。
6. 文件换名为另一平台 blob 仍拒绝。
7. product spawn 不读 PATH、`GROK_CU_NODE_FILE` 或源码 tools 目录。

### B1.2：安全物化 `playwright-core@1.48.0`

#### 固定输入

- 只使用现有官方 tgz；版本 `1.48.0`。
- SHA-256 必须保持
  `60cbf41da4e72847064ad7c720088dd133cf8601f7af9d5b1b73751d6e649562`。
- 不执行 `npm install`、postinstall、生命周期脚本或在线依赖解析。

#### 物化策略

优先在**受信任的构建准备阶段**验证并展开固定 archive，生成待打包的只读 seed tree；
安装/Repair 只把 manifest 中逐文件校验过的物化 tree 原子复制到 App 私有 runtime。
不要默认在每台用户机器上运行 npm 或解析压缩包。只有仓库既有打包链无法可靠携带物化
tree 时，才允许采用运行时安全解包，并必须在 ADR 中说明原因与额外攻击面。

无论在构建期还是运行时物化，都必须满足：

- 在独立 staging 下展开，成功并生成内容清单后才成为 bundle seed/active pack；不得直接
  写 active pack。
- 只接受预期 `package/` 根；剥离这一层后写入
  `playwright/node_modules/playwright-core/`。
- 拒绝绝对路径、`..`、盘符、UNC、ADS、NUL、symlink、hardlink、device、FIFO 和
  任何逃出 staging 的 canonical path。
- 设文件数、单文件、总展开字节、路径长度和压缩比上限。
- 重复路径、大小/hash 不一致、提前 EOF、额外根目录全部失败。
- 失败清理整个 staging；active/previous 指针不变化。
- 生成或验证稳定内容清单；确认 `package.json` name/version 为
  `playwright-core`/`1.48.0`。
- Windows 不继承 archive 中不可信 ACL；Unix 后续 pack 只给明确文件设置可执行位。

#### 测试

- 正常 tgz 展开并由 App-owned Node 成功执行一次只读 import/version probe。
- traversal、symlink、重复路径、超量、截断、错 hash、错 package version 全拒绝。
- 中断后没有半激活 pack；rollback 仍可回到 previous。
- import probe 使用空 PATH、无 `NODE_PATH`，且不能从仓库根 `node_modules` 命中。

### B1.3：打包真实 production worker，消除双源漂移

#### 权威源与产物

`tools/computer-use-browser/` 保持 production worker 源码权威。新增一个确定性 pack
准备/校验脚本，使用显式 allowlist 复制生产入口与所有直接/间接 sibling imports。

要求：

- `--prepare` 只写明确的生成目录/staging，再原子发布为 bundle seed。
- `--check` 不写文件，只比较源、资源产物和 manifest hash；CI/本地门禁使用它。
- allowlist 中不得包含 `*.test.mjs`、fixtures、`.run`、profile、日志或 probe-only route。
- 入口和模块要么逐文件进入 manifest，要么生成稳定 tree manifest。
- 禁止保留手写的一行 `worker.mjs` 占位；禁止靠人工复制后不做同步检查。
- production worker 启动必须输出一行严格 port/handshake JSON，并保持后续 stdout 干净。
- worker 的 `import("playwright-core")` 只能解析 active pack 内物化模块。
- BrowserSupervisor 子进程使用 env allowlist；sentinel keys/proxy 不进入 worker/browser。
- production entry 不含 `/state`、`/marker`、`/crash`、selector/evaluate/CDP 测试后门。

测试必须人为改动/删除一个资源模块，证明 `--check` 与 runtime diagnose 都会失败；
恢复后再 Green。不要通过忽略未知 import 让检查通过。

#### Git 与构建产物卫生

- 不把 Node/Playwright/Chromium 大二进制直接加入 Git，除非维护者明确批准 Git/LFS 策略。
- 源码提交精确的 runtime lock/source manifest：版本、最终 URL、archive hash、tree digest、
  license、期望大小和 target。
- 生成的 seed tree 放在明确 gitignored 目录；`--prepare` 可复现生成，`--check` 可验证。
- 正式 Tauri 构建必须在 resources 收集前执行 `--check`；缺少/陈旧/错 hash 直接失败，不能
  生成一个悄悄缺 Computer Use runtime 的安装包。
- CI cache 只是提速，不是信任来源；每次仍校验 lock 中的 hash。

### B1.4：加入固定 App-owned Chromium

#### 版本

从当前 `playwright-core@1.48.0` 的 `browsers.json` 读取并锁定 Windows Chromium：

- revision：`1140`；
- browser version：`130.0.6723.31`。

不得从文字计划猜下载 URL、hash 或文件布局。使用 Playwright 官方 metadata/固定 URL，
记录最终 URL、archive SHA-256、展开 tree manifest、license/NOTICE 和复现命令。

#### 规则

- browser archive 与展开后的 executable/tree 都进入 runtime 契约。
- 采用与 B1.2 相同的安全 staging、路径检查、容量上限、原子激活和 rollback。
- product Host 显式把已 resolve 的 browser executable 传给 worker。
- production `chromePath()` 不扫描系统 Chrome/Edge；系统浏览器 fallback 只能存在于明确的
  developer probe helper，不能被产品入口调用。
- 使用 App-owned profile；绝不复用用户 Chrome/Edge profile。
- 记录发行包体积变化。若固定 Chromium 的许可或包体积形成产品决策阻断，停止并报告，
  不能静默退回系统 Chrome 后把 B1 标完成。

#### 测试

- PATH/system Chrome decoy 存在时仍启动 pack Chromium。
- 浏览器 handshake 返回实际 executable path 的脱敏 pack identity、revision/version。
- 删除/篡改 browser executable 或任一内容清单文件时 diagnose 与 spawn 均失败。
- shutdown、worker crash、App-side abort 后 Chromium 全进程树退出。

### B1.5：让 packaged contract 达到真实 Green

在 B1.0 探针上继续，不另写一条绕过产品路径的测试。

必须证明：

1. `repair_from_install()` 从 seed 创建新的 active pack。
2. `product_spawn_request()` resolve 的 node、worker、Playwright、Chromium 全在隔离
   App runtime 根内。
3. 子进程 handshake 明确返回 wire protocol、worker build/source hash、Node version、
   Playwright version、browser revision/version、page generation 和 response caps。
4. open 隔离 profile；navigate 到随机 loopback 自建页面。
5. observe 返回新 snapshot、opaque refs、合法 PNG/geometry。
6. 使用 opaque ref 点击或填写；独立 oracle 读取真实页面后置条件。
7. 再 observe 得到新 snapshot，并验证页面状态。
8. shutdown 后 worker、Chromium 及子孙 PID 全退出。
9. profile/staging/temp 在策略允许范围内准确保留或删除；测试临时根必须删除。
10. sentinel secret、Token、proxy、源码路径和用户目录不出现在 stdout/stderr/trace/响应。

连续三次运行。任一 flake 后保留首次失败，定位修复，并从第 1 次重新计数。

### B1.6：损坏、修复、升级与回退矩阵

在隔离 runtime 根内覆盖：

- 缺 Node、错 Node hash/版本/架构；
- 缺 worker entry/sibling、worker 占位、worker hash 错；
- 缺 Playwright materialization、archive 错、import 失败；
- 缺 Chromium、browser tree hash 错、不可执行；
- staging 中断、active pointer 部分写入、previous 缺失；
- worker 启动超时、handshake 版本不匹配、worker 崩溃；
- repair 成功、同版本幂等 repair、repair 失败保持旧 active、rollback 成功。

每种情况必须返回稳定 typed issue/code 和可行动文案；不输出绝对 profile、Token、Cookie、
URL query 或 secret。不能把“重装即可”作为所有错误的唯一分类。

### B1.7：本批完整门禁与停笔

#### Runtime/Browser 定向

```powershell
cd H:\aicoding\grok-app-computer-use
node scripts\prepare-computer-use-runtime.mjs --check

$browserTests = Get-ChildItem -LiteralPath tools\computer-use-browser -Filter *.test.mjs |
  ForEach-Object FullName
node --test $browserTests

cd H:\aicoding\grok-app-computer-use\src-tauri
cargo test -p grok-computer-use-core runtime:: --lib --offline --target-dir target-cu-review
cargo run -p grok-computer-use-probe --bin cu_probe --offline --target-dir target-cu-review -- browser-packaged-contract
```

`browser-packaged-contract` 连续三次，每次都必须记录 wall time 与 PID/文件后置条件。
命令名可因实现调整，但不能退回 `GROK_CU_NODE_FILE` 或源码 worker。

#### 回归

```powershell
cd H:\aicoding\grok-app-computer-use
node --test tools\computer-use-mcp\*.test.mjs
pnpm exec vitest run src/components/computer-use src/lib/computer-use `
  src/lib/settingsCatalog.test.ts src/lib/sideWorkbench.test.ts src/lib/slashCatalog.test.ts
pnpm typecheck
pnpm lint

cd H:\aicoding\grok-app-computer-use\src-tauri
cargo fmt --all -- --check
cargo clippy -p grok-computer-use-core --all-targets --offline `
  --target-dir target-cu-review -- -D warnings
cargo test -p grok-computer-use-core --lib --offline --target-dir target-cu-review
cargo test -p grok-computer-use-core --test driver --offline --target-dir target-cu-review
cargo check -p grok-app --offline --target-dir target-cu-review

cd H:\aicoding\grok-app-computer-use
git diff --check
git status --short --branch
```

不要让两个 cargo 命令并发写同一个 target dir；否则锁等待不算代码故障但会污染耗时。

#### 静态审查

- seed 中不再存在 53-byte/一行 browser worker 占位。
- product spawn 不读取 `GROK_CU_NODE_FILE`、PATH Node、源码 tools 路径或系统 Chrome。
- product pack 不含测试、fixture、`.run`、profile、dump、token 或开发机绝对路径。
- worker 全部 import 均存在并在 manifest/tree manifest 中。
- `browser-worker`、Playwright runtime、Chromium 都是 required/diagnosed components。
- 不新增 production selector/evaluate/CDP/test route。
- 不新增 `allow`、skip/ignore 或 AppWorkbench Computer Use 状态。

## 6. B1 停止条件

以下全部成立才允许写“B1 passed（Windows E2/E3）”：

- packaged contract 使用真实发行资源且连续三次 Green；
- 不依赖系统 Node、系统 Chrome、根 node_modules、源码 worker 或手工环境变量；
- repair/diagnose/rollback 与损坏矩阵 Green；
- 全部回归门禁 Green；
- 没有 worker/browser 残留；
- 资源来源、版本、hash、license 和包体积已记录；
- 执行账本包含首次 Red、最终 Green 和所有未验证项。

B1 结束后立即停笔，不进入 B2，不 commit/push/PR。

## 7. 最终报告模板

1. **结果一句话**：B1 passed/failed/blocked；证据等级。
2. **首次 Red**：命令、exit、具体失败和清理后置条件。
3. **实现清单**：每个文件职责；manifest/runtime layout。
4. **供应链**：Node、Playwright、Chromium 的来源、版本、hash、license、大小。
5. **安全审查**：archive extraction、路径、环境、secret、profile、loopback、后门。
6. **三次 packaged contract**：每次耗时、Node/Playwright/browser identity、页面/PNG/
   文件/PID 后置条件。
7. **损坏/repair/rollback 矩阵**。
8. **回归门禁**：命令、exit、test count、warning/flake。
9. **工作树**：`git status --short --branch`、`git diff --check`、新增大文件及大小。
10. **明确未完成**：B2–B7、安装 App + 真实模型 E4、Existing Tabs/WebView E4、
    macOS/Linux、S11/S12、四 target E5。

禁止写“Computer Use 完成”“三平台完成”或“可发布”。
