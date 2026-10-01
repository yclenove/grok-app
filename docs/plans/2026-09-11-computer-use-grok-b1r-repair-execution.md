# Computer Use B1-R：Windows packaged runtime 交付修复执行书

> - 日期：2026-09-11
> - 分支：`feat/computer-use-implementation`
> - 基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`
> - 性质：B1 审查后的修复批次；不是 B2，也不是 Computer Use 完成声明
> - 权威状态账本：`docs/plans/2026-09-09-computer-use-execution-state.md`

执行边界说明：单独使用 B1-R 提示词时，B1-R 完成后停笔。若用户明确使用
`2026-09-11-computer-use-grok-overnight-prompt.md` 启动整夜任务，则 B1-R 全绿并写完独立
检查点后，可按整夜执行书继续下一个 ready 批次；这只覆盖“停在批次边界”的要求，不覆盖
任何测试、证据、安全或外部条件门禁。

## 1. 本批目标

B1 已经证明当前 Windows 工作机上的 App-owned Node + Playwright + Chromium 能沿
`repair_from_install -> product_spawn_request -> BrowserSupervisor` 完成真实页面闭环。
B1-R 不重做这条功能，而是补齐 B1 尚未满足的交付契约：

1. 从不含任何 gitignored 产物的干净源码副本，可按锁文件复建 Windows runtime seed；
2. 正式 Tauri 构建在收集 resources 前强制 prepare/check，不能产出缺 runtime 的安装包；
3. Chromium archive 使用受限、安全、可复现的 staging 解包，完整 tree 被现场重新校验；
4. Windows runtime 只进入 Windows 包，不把 Windows 二进制塞进 macOS/Linux 包；
5. Repair 能修复同版本 active pack 的任意受管组件损坏，失败不破坏 active/previous；
6. Git、缓存、包内容、许可和报告与实际一致；
7. 在隔离干净副本重新通过 B1 packaged contract 和全部门禁后停笔。

B1-R 通过只允许写：

> Windows packaged Browser runtime 的源码交付、构建准备、完整性校验和 Repair
> 契约通过（E2/E3）。

仍不得写“Computer Use 完成”“Windows Computer Use 完成”或“可发布”。安装 App +
真实模型 E4、Existing Tabs/WebView、macOS/Linux 原生实现、S11/S12 和四 target E5
仍不在本批范围。

## 2. 审查基线与必须复现的 Red

### 2.1 已可信的 B1 结果

- packaged contract 最终连续三次通过：`83642 / 41620 / 42096 ms`；
- runtime 为 Node `20.18.0`、Playwright `1.48.0`、Chromium
  `130.0.6723.31` revision `1140`；
- 真实 loopback 页面完成 open、observe、opaque-ref click、独立 oracle、再 observe；
- worker/Chromium 后置清理成立；
- Browser Node 76、core 249、runtime 22、driver 12、MCP 5 和前端定向 120
  在最终重跑中通过。

这些结果是回归基线，修复中不得降低或用 mock/stub 替代。

### 2.2 B1-R 必须先固定的失败

在改实现前，把以下行为写成自动化 Red 或可重复门禁。不得只把审查文字抄进报告。

1. **干净源码不可复建**
   - 用 `git ls-files --cached --others --exclude-standard` 复制可交付文件到隔离临时目录；
   - 不复制任何 ignored seed/cache/target；
   - 当前 `--check` 因缺物化 Playwright tree 失败；
   - 当前 `--prepare` 物化 Playwright 后仍因缺 Chromium 失败。
2. **Chromium checker 假 Green**
   - 在隔离 fixture 中保持 `chrome.exe` 不变；
   - 篡改 Chromium tree 内另一个普通文件；
   - 当前 `--check` 仍为 0，证明只校验 exe、没有现场重算全 tree。
3. **同版本 Repair 不修复**
   - 安装 seed 并生成带 embedded MCP 的 active pack；
   - 分别篡改 Node、worker、worker sibling、Playwright runtime 和 Chromium；
   - 再调用同一 bundle 的 Repair；
   - 当前实现因为目标 pack manifest 已存在而复用旧 pack，至少一种损坏仍存在。
4. **构建链可绕过**
   - 证明 `package.json`、Tauri build hook、`scripts/build-local.sh`、CI/release workflow
     均未强制执行 runtime check；
   - 证明缺 ignored seed 时仍可能开始正式 bundle，或至少没有 fail-closed gate。
5. **Git/包内容策略不一致**
   - `seed/bin/node.exe` 和 `playwright-core-1.48.0.tgz` 当前未跟踪且未忽略；
   - Chromium zip/tree 被忽略，但没有可复现获取器；
   - base `tauri.conf.json` 的 `resources/computer-use/**/*` 会覆盖所有平台，并可能同时
     收入展开 tree 与 147 MB archive。

每个 Red 必须记录命令、exit、首个失败和 fixture 后置清理。Red fixture 只允许写随机临时
目录；禁止破坏当前 seed 来制造失败。

## 3. 不可违反的边界

- 只在 `H:\\aicoding\\grok-app-computer-use` 和专用临时目录工作。
- 保留全部既有修改。禁止 `reset`、`clean`、`checkout --`、`restore`、`stash`、强切分支、
  覆盖或删除不属于当前原子项的文件。
- 不执行 `git add`、commit、push、PR、merge、tag、release。
- 不进入 B2，不做 UI 美化、真实模型 E4、Existing Tabs/WebView、macOS/Linux 动作实现。
- 不碰账号、Token、Cookie、代理、共享 `~/.grok`、正式 Grok App、用户 Chrome/Edge
  profile 或其他 worktree。
- 产品路径继续禁止 PATH Node、系统 Chrome/Edge fallback、npm lifecycle 和在线 `latest`。
- 构建时网络只允许锁文件中的精确官方 URL/镜像；不得搜索并选“最新版”。
- 不把 Node、Playwright、Chromium 大二进制加入普通 Git。没有维护者明确批准时不得引入
  Git LFS 或私有制品依赖。
- 不加 lint allow、test skip/ignore、宽松断言、假能力或 production 测试后门。
- 任何工具输出不得打印环境凭据、代理认证信息、Bearer 或用户目录敏感内容。
- 同一共享工作树只允许一个代码 writer。

## 4. 固定交付设计

### 4.1 tracked 与 generated 分界

源码仓只跟踪：

- prepare/check 源码和测试；
- 每 target 的精确 source lock：版本、target、固定 URL/允许 redirect host、archive
  SHA-256、archive bytes、展开根、tree SHA-256、tree files/bytes、关键 executable hash；
- production worker 权威源码；
- NOTICE/license 清单；
- Tauri/CI 接线。

以下全部是 generated，必须被 `git check-ignore` 命中：

- Node 下载 archive 和提取后的 `node.exe`；
- Playwright tgz、物化 `node_modules/playwright-core` 和生成 listing；
- seed 内 worker 副本及 sibling listing；
- Chromium archive、展开 tree 和最终 Windows seed manifest；
- 下载临时文件、staging、构建缓存。

archive cache 必须位于不会被 Tauri resources glob 收入的 ignored cache 根。Chromium zip
不得留在最终 seed 中。Playwright tgz 是否继续作为 runtime component，B1-R 默认保持现有
v2 契约；若要移除，必须先写 ADR 和迁移测试，不能顺手改。

### 4.2 单一锁源

不得继续让 Node/Playwright hash 分散在脚本、Rust 常量和手写 manifest 中而无一致性门禁。
建立或收敛为一个 Windows x64 权威 lock；生成 manifest 必须从该 lock 和实际内容产生。
Rust 必要常量可保留，但必须有 golden test 证明与 lock 完全一致。

当前已知 pin 只能作为核对基线：

| 组件 | 固定值 |
| --- | --- |
| Node | `20.18.0` win-x64，`node.exe` SHA-256 `35b7c95a…4cc94` |
| Playwright | `1.48.0`，tgz SHA-256 `60cbf41d…49562` |
| Chromium | revision `1140`，version `130.0.6723.31` |
| Chromium archive | SHA-256 `da752c83…4aedb`，147201336 bytes |
| Chromium tree | SHA-256 `63c6075f…45e3`，84 files，361230888 bytes |

实现必须从已审计 lock 读取完整值，不得从本表省略值或凭记忆补全。

### 4.3 prepare/check 语义

`--prepare --target <target>`：

1. 只解析受支持 target；B1-R 只生成 Windows x64；
2. 从 content-addressed cache 读 archive，缺失时按固定 URL 下载到临时文件；
3. 限制 redirect host、响应大小和超时，下载完先校验 bytes/hash，再原子进入 cache；
4. Node/Chromium archive 均走受限安全解包；Playwright 继续走安全 tar materializer；
5. worker allowlist 复制、所有 tree/listing/manifest 都在新的 staging seed 中生成；
6. 对 staging 执行与 `--check` 相同的完整检查；
7. 全绿后原子发布 generated seed；失败删除 staging，保留上一份健康 seed；
8. 不运行 npm install/postinstall，不执行 archive 内脚本。

`--check --target <target>`：

- 严格只读；测试要比较调用前后文件快照；
- 重新计算每个文件 hash、tree digest、file count 和 total bytes；
- 校验 source worker 与生成 worker、sibling listing、manifest、lock 的一致性；
- 校验 Node/Chromium executable 格式和精确 hash；
- 用 App-owned Node 在空 PATH、无 `NODE_PATH` 环境 import pack 内 Playwright 并输出
  `1.48.0`；
- 任一缺失、额外文件、篡改、版本/架构错、symlink/reparse/special file 均非零退出；
- 不把 manifest 中预存 digest 与 lock 中预存 digest 相互比较后当作现场校验。

### 4.4 Chromium/Node 安全解包

Chromium 与 Node zip 至少拒绝：

- absolute、UNC、盘符、ADS、NUL、`.`、`..`；
- symlink、hardlink、junction/reparse point、device 和其他 special entry；
- Windows 大小写折叠后的重复路径；
- 非预期 archive root、多根、缺根；
- 超文件数、单文件大小、总展开大小、压缩比和过长路径；
- 截断、CRC/读取失败、archive hash/bytes 不匹配；
- 错 executable hash、错版本布局和缺 license/notice 输入。

解包必须在随机 staging 中完成。发布时不能先删除上一份健康目录再 rename；采用可恢复的
swap/backup 或新的 content-addressed 目录，保证中断后旧 seed 仍可用。

### 4.5 Tauri 与 CI 接线

必须同时满足：

1. Windows `tauri build` 的 `beforeBuildCommand` 或等价不可绕过入口先 prepare/check，后
   build UI；直接从常用脚本和 release action 构建都能触发。
2. CI/release workflow 显式执行 prepare/check，cache 仅提速，命中后仍重新校验。
3. 缺 generated seed 时必须在 bundle 之前失败，不能生成悄悄缺功能的安装包。
4. 目标识别来自已验证的 Tauri target 环境或 workflow 明确参数；不得猜 host 等于 target。
5. Windows resources 只收最终 Windows seed；不收 cache、Chromium zip、staging、日志或
   fixture。
6. macOS/Linux 当前不打包 Windows seed；功能保持 default-off/unavailable，不伪装成已
   支持。为 target 映射写自动化测试。
7. `pnpm build:ui` 仍可独立运行，不应无故下载 500 MB runtime。

如果 Tauri 平台配置的数组 merge/replace 语义不确定，必须先用最小 fixture 或生成配置
输出验证；禁止凭经验修改后直接宣称资源隔离完成。

### 4.6 Repair 正确语义

Repair 的目标是恢复全部 `required_runtime_components()`，不是仅检查 Node 和 tgz。

- 复用已有 pack 前，必须完整 diagnose 所有 required components 和 sibling/tree；
- 已有 pack 全健康且 source manifest/embedded MCP 完全一致时可幂等复用；
- 只要任一组件损坏，就从安装 bundle 构造新的 staging/repair generation，完整校验后再
  原子激活；不得因 `manifest.json` 存在而跳过复制；
- content identity 必须包含 file hashes、tree digests、compatibility 和 embedded MCP，
  不能忽略 tree 后复用旧 `mcp-*` pack；
- `ensure_embedded_mcp` 不得重新激活一个未经完整校验的旧 MCP pack；
- Repair 成功返回前再次 diagnose 全部 required components；
- Repair 失败保持原 active/previous 和 current pointer 不变，并清理本次 staging；
- rollback 只切换到完整健康的 previous；previous 缺失/损坏返回稳定 typed issue；
- active manifest 的 schema、relpath 和 canonical containment 每次 resolve/diagnose 都要
  fail closed，不能仅信任安装时校验。

## 5. 原子执行顺序

每一项开始和结束都追加 execution-state。当前项未 Green 不得进入下一项；Green 后自动
继续，不需要等待用户逐项确认。只有遇到需要改变本执行书的产品决策才停下。

### B1-R.0：冻结现场与捕获五类 Red

- 记录 HEAD、完整 status、ignored/generated inventory、现有 B1 三次日志；
- 用隔离 fixture 固定第 2.2 节全部失败；
- 新测试先 Red；不得修改 seed/实现掩盖 Red；
- 清理所有自建临时目录和进程。

### B1-R.1：收敛 artifact/lock/Git 契约

- 建立单一 Windows x64 source lock 和 schema/golden tests；
- 将所有 generated/cache 路径完整 ignore；
- Node/tgz/Chromium 不进入 `git ls-files --cached --others --exclude-standard` 的可交付集合；
- 保留 production worker 源码为唯一权威源；
- 写 `git check-ignore` 和“大文件未进入 Git”门禁。

验收：源码副本不含任何 runtime 二进制，但拥有复建所需 lock、代码和许可元数据。

### B1-R.2：实现可复现、安全、原子 prepare

- 实现固定下载/cache；
- 实现 Node/Chromium 安全 zip staging；
- 复用并补齐 Playwright tar materializer；
- worker/manifest/tree listing 全部从权威源生成；
- 故障注入覆盖下载截断、错 hash、危险路径、重复路径、超量、解包中断和 publish 中断；
- 证明失败不改变上一份健康 generated seed。

验收：空 generated/cache 的干净源码副本执行 prepare 后生成与 lock 完全一致的 seed；再次
prepare 幂等且 digest 不变。

### B1-R.3：实现真正只读的完整 check

- 完整重算 Playwright/Chromium tree；
- 校验所有 worker sibling 和无额外 production 文件；
- 校验 Node、Playwright import、Chromium executable；
- 对任一非 exe Chromium 文件篡改必须 Red；
- 对 check 前后递归快照必须完全一致。

验收：第 2.2 的 Chromium 假 Green 转为稳定 Red，恢复后 Green。

### B1-R.4：接入 Tauri、本地构建与 CI，并按 target 隔离资源

- 调整 package/Tauri/build-local/CI/release 入口；
- Windows bundle 前 prepare/check；macOS/Linux 不打包 Windows seed；
- 为 target 参数和 Tauri config 合并写测试；
- 在临时资源树证明缺 seed、陈旧 seed、错 target 都在 bundle 前失败；
- 审计最终 resource 列表，Chromium zip/cache/staging 不得出现。

验收：常用正式构建路径不可绕过，UI-only 构建不触发 runtime 下载。

### B1-R.5：修复同版本 Repair/rollback

- 先加入每组件同版本腐坏 Red；
- 修复 existing pack 复用、MCP pack identity 和全 required diagnose；
- 覆盖 active pointer 部分写、previous 缺失/损坏、失败清 staging、连续两次 Repair；
- Repair 后再次走 product paths 和真实 worker handshake；
- 保证旧健康 active 在所有失败注入后仍可启动。

验收：Node、worker、sibling、Playwright archive/runtime、Chromium 任一损坏均可由同版本
Repair 恢复；不可恢复情况诚实失败且不破坏旧 active。

### B1-R.6：许可、包内容与报告一致性

- 补 Node 官方 LICENSE/NOTICE；保留 Playwright LICENSE/NOTICE/ThirdPartyNotices；
- 保留 Chromium CREDITS/版本/license 来源；
- 更新 `NOTICE-computer-use.md`，删除“待补”和过期状态；
- 列出最终 Windows seed 每组件 source/version/hash/bytes/license；
- 审计包中不存在下载 archive 重复、用户数据、测试 fixture、日志和源码绝对路径；
- 记录安装包相对主干的体积增量，不凭空设阈值，也不隐瞒增长。

### B1-R.7：完整回归、干净副本和停笔

必须在最终代码上一次性执行并保存原始日志：

1. `git diff --check`；
2. runtime `--check` 的只读证明；
3. Browser Node 全套；
4. MCP golden；
5. 前端 Computer Use/settings/slash 定向测试；
6. typecheck、lint、`cargo fmt --check`、core clippy `-D warnings`；
7. core `--lib`、runtime、driver、App `cargo check`；
8. 同版本 Repair/rollback 完整矩阵；
9. 当前工作树 packaged contract 连续三次；
10. 从不含 ignored 产物的隔离源码副本：空 cache prepare、check、第二次 prepare/check、
    packaged contract 连续三次；
11. Windows Tauri bundle 或等价真实 resources 收集，随后审计包内容；
12. target-map 测试证明 macOS/Linux 不收 Windows seed；未在对应 OS 构建则明确 `not_run`。

任一 flake：保留首次失败，定位并修复，从要求连续计数的第 1 次重新计数。不得仅提高 timeout
或只重跑失败用例后称“一次性完整门禁通过”。

全部 Green 后追加“B1-R 完成”并形成独立检查点。单批运行到此停笔；整夜 master 运行可按
其依赖图继续 O2，但不得把 B1-R 结果写成 B2 或整项 Computer Use 完成。

## 6. 必须新增或补齐的自动化测试

- clean-source prepare：无 generated/cache 时可复建；
- prepare deterministic：两次 seed tree/manifest digest 一致；
- check read-only：递归 path/size/hash/mtime 快照不变；
- Chromium non-exe tamper；额外文件、删文件、exe tamper；
- zip absolute/UNC/drive/ADS/NUL/traversal/symlink/reparse/duplicate-case/extra-root/oversize；
- prepare publish interruption 保留 previous generated seed；
- build hook missing/stale/wrong-target fail-before-bundle；
- Windows/macOS/Linux target resource map；
- same-version corruption repair：每个 required component；
- existing healthy pack idempotent reuse；existing corrupt pack 不得复用；
- embedded MCP identity 包含 tree/content identity；
- repair failure preserves active/previous/current；
- previous missing/corrupt rollback fail closed；
- active manifest relpath/schema tamper fail closed；
- final package deny-list：archive cache、Chromium zip、tests、fixtures、logs、`.run`。

## 7. 阻塞与停止条件

出现以下情况立即停止当前实现、保留现场并报告，不得自行扩大范围：

- 锁定官方 URL/哈希无法核实，或必须改用未批准的镜像/私有制品；
- 需要 Git LFS、仓库外长期凭据或发布基础设施决策；
- Tauri 无法按 target 隔离 resources，需要改变整个发布架构；
- 需要升级 Node/Playwright/Chromium pin；
- 修复要求触碰正式安装、用户 profile、账号/Token/Cookie/代理；
- 发现 B1 真实页面回归且无法在本批边界内修复。

阻塞报告必须包含：已经做过的三次不同尝试、原始错误、可选方案、风险和需要维护者决定的
单一问题。不能用系统 Node/Chrome fallback 或跳过校验解除阻塞。

## 8. 最终报告模板

1. 一句话结果：`passed` / `partial` / `blocked`；
2. 五类首次 Red 与最终 Green；
3. tracked/generated/cache/final-package 分界；
4. 每组件 source/version/hash/bytes/license；
5. prepare/check/build 接线和 target map；
6. same-version Repair/rollback 故障矩阵；
7. 干净源码副本复建命令、耗时、两次 digest；
8. packaged contract 六次结果（当前工作树三次 + 干净副本三次）与 PID/文件后置；
9. Windows 包内容和体积增量；
10. 全部门禁命令、exit、测试数、耗时、flake；
11. `git status`、`git diff --check`；
12. 明确列出 E4、Existing Tabs/WebView、macOS/Linux、S11/S12、E5 仍未完成；
13. 停笔，不提出已进入 B2 的实现结果。
