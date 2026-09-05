# 浏览器重构需求与验收矩阵

状态：设计覆盖审查基线。开发与运行验收均未执行。总入口：[实施总索引](README.md)，任务与依赖的可读数据：[task-index.json](task-index.json)。

本文件将总体设计转换为可追踪需求。右列是未来开发必须提交的证据，不能用“已写测试”“功能已连线”或本次文档检查代替实际结果。状态统一为 `not_started` / `in_progress` / `blocked` / `passed` / `failed` / `not_run`，其中 `passed` 必须指向已验证证据。

## 需求覆盖

| ID | 必须满足的结果 | 开发任务 | 验收证据与反例 |
| --- | --- | --- | --- |
| BR-01 | 长期 Preview、任务分组、导航/标题/停止/下载真实可用 | M0-W01、M0-W03、M0-W04、M0-W06 | macOS/Windows原生fixture；同URL双tab/双下载、内部跳转、停止慢请求 |
| BR-02 | AppSession owner 与归档/删除/fork/rewind正确 | M0-W01、M0-W05、M1-G03 | commands/SSH/自动化删除同门、跨会话拒绝、删除中断、fork无旧grant、rewind不重放外部效果 |
| BR-03 | 投影单一、无晚回调覆盖、无原生遮挡回归 | M0-W02、M0-W04、M0-W06 | StrictMode/快速显隐/拖拽/双modal、旧boot/generation洪泛零重取、event缺口合并重取 |
| BR-04 | 按能力路由并尊重pin，后端切换不搬登录态 | M1-G05、M1-G06 | view/act/Chrome矩阵；pin不可满足返回unsupported；Cookie不跨边界 |
| BR-05 | 原生输入归因可证明、权限失败关闭writer | M0-R02、M0-R03、M0-R05、M0-R06、M1-G04 | 签名包TCC/Raw Input探针，权限拒绝/撤销、监听失效、窗口变化 |
| BR-06 | 第一方Browser MCP跨official/custom/shared/independent可用 | M1-G01、M1-G06 | 真实CLI new/load/warm/热更新矩阵；列表构建切会话拒旧；第三方超时不阻塞；SSH/侧信道不注入 |
| BR-07 | Host完整身份链、origin/frame与capability授权 | M1-G01、M1-G02、M1-G06 | 旧boot/binding/tab/nav、MCP伪造身份、跨frame/popup/redirect负例零放行 |
| BR-08 | 六种policy、后果性动作、秘密与OS权限一致 | M1-G02、M1-G03、M1-G06 | dont_ask无挂起、未知click不猜测放行、批准失效与秘密不进入输出 |
| BR-09 | 单writer、接管锁存、跨轮不夺回控制 | M1-G03、M1-G04、M1-R03、M1-G06 | Host active_turn_id/提前prompt_complete/真实结束、并发撤销/Playwright等待取消、旧ACK、新turn、共享Runtime观察停止 |
| BR-10 | project/ephemeral/named Profile与迁移不串数据 | M0-R05、M1-R04、M2-05、M2-06 | 同Profile锁、多App竞争、epoch升级/降级、named显式共享与删除确认 |
| BR-11 | 固定Runtime tuple、私有worker通道、无外部PATH依赖 | M0-R01、M1-R01、M1-R03 | manifest/hash/协议不匹配、无外部Node/Chrome机器、无调试TCP监听 |
| BR-12 | Host/worker/App崩溃均清理正确进程并守Profile锁 | M0-R04、M1-R02、M1-R06 | PID/start-time/run nonce、用户Chrome并行、30秒descendants=0或cleanup_failed |
| BR-13 | Artifact不泄漏路径/秘密、原子promotion与有界保留 | M1-R05、M2-01、M2-03、M2-06 | 写/rename/owner/journal各点crash、单主体capability、过期/owner重建 |
| BR-14 | 上传明确披露、副本按真实读取生命周期销毁、下载恢复不重复写 | M1-R05、M1-R06 | 延迟FileReader/提交、50/200MiB边界、Runtime-owned spool预算/退出清理、源文件替换、export中断needs_review、MOTW/quarantine |
| BR-15 | 恢复上下文不恢复自动驾驶/未知副作用 | M1-G04、M1-G05、M1-R06 | GET敏感URL、旧token/lease、inflight checkpoint、requestId重试零重放 |
| BR-16 | Console/Network/Trace完整查看与安全导出 | M2-01、M2-02、M2-03、M2-06 | cursor/过滤/详细/重开、跨origin去敏、未知raw trace schema拒绝 |
| BR-17 | 真实截图、Design Mode v2、响应式与人工检查可用 | M2-03、M2-04、M2-06 | 像素非空、选区/多选/注释、viewport恢复、user inspection permit不解除latch |
| BR-18 | Chrome pairing、显式claim、任务组、撤销和恢复完整 | M3-01、M3-02、M3-03、M3-04、M3-05、M3-06 | 正确native进程、窗口输入撤全claims、重连不复活、原标签保留；逐claim下载归因与能力对等，无证明则M3未完成 |
| BR-19 | Linux支持矩阵真实，sandbox/输入/凭据条件明确 | M4-01、M4-02、M4-03、M4-04、M4-05、M4-06 | Linux原生driver实控App、X11/Wayland逐组合、Secret Service、AppImage/deb/rpm真实包；未证明writer则unsupported |
| BR-20 | SSH只走本地Browser Bridge，隧道归属与清理可靠 | M5-01、M5-02、M5-03、M5-04、M5-05、M5-06 | Host/Guard双持有listener、ssh -W、专属mux/改alias目标、keepalive黑洞、单点kill/抢占竞态、新revision隔离Profile |
| BR-21 | 独立Runtime更新具有签名、防重放、兼容回滚 | M6-01、M6-02、M6-03、M6-04、M6-06 | attestation组件授权/拒绝旧App越权、签名恶意archive/sequence/撤销/断点/epoch/active tuple/一次LKG矩阵 |
| BR-22 | 性能/容量预算有参考机证据，CEF仅决策实验 | M0-R06、M6-05、M6-06 | 冷启动/新tab/接管/恢复、资源基线；CEF go/no-go报告且不默认替换WebView |
| BR-23 | 所有用户路径含i18n、settings、键盘和视觉验收 | M0-W04、M0-W06、M1-G03、M1-G05、M2-06、M3-06、M4-06、M5-06、M6-06 | 15目录key+非空+占位、en/zh/de/ru/ta视觉、search/deep link、focus、实体菜单 |
| BR-24 | 最终包签名/发布/灰度/kill switch/回滚与本地CI一起过门 | M1-D01、M1-D02、M1-D03、M1-D04、M1-D05、M1-D06、M6-06 | clean runner复下载、Draft不齐不发布、有效/过期bootstrap、portable手动更新、真实App updater安装交接/失败边界、完整CI |

## 分层验证

| 层级 | 证明范围 | 不能替代的证据 |
| --- | --- | --- |
| TypeScript/Rust单元 | reducer、policy、状态转换、schema与失败分支 | 原生窗口、系统权限、真实CLI |
| Host adapter集成 | owner、dispatch顺序、持久化/故障注入、传输 | 签名包中的驱动与OS进程清理 |
| 真实浏览器fixture | locator、导航、popup/frame、真实像素、下载 | 最终安装包签名/升级/隔离 |
| Tauri UI/原生联合 | menu cover、焦点、IME、WebView几何、接管 | 仅DOM screenshot不能证明native surface非空 |
| 真实CLI | ACP注入、turn/connection、模式/来源/GROK_HOME | fake MCP server通过不能代替 |
| 最终包平台矩阵 | installer/portable、签名、升级/回滚、权限、清理 | dev build与交叉编译不能代替 |
| beta运行证据 | 动作成功率、crash-free、unknown outcome与容量 | 时间经过或少量开发机日志不能代替 |

每个平台/包型对声明fixture连续三轮通过才可标完成。性能报告至少30样本才报P50/P95/max，P99至少100样本；均写精确参考机、输入范围、测量起止点和排除项。

## 平台与发布范围

| 里程碑 | 必须验证的发行组合 | 通过前的产品状态 |
| --- | --- | --- |
| M0 Preview | macOS arm64/x64、Windows x64原生WebView；现有Linux兼容回归 | Preview 2.0内部flag，Managed关闭 |
| M1 | macOS arm64 DMG、macOS x64 DMG、Windows x64 NSIS和portable；最低支持OS+当前稳定OS | 未齐套不宣称首批完成，失败平台不授writer |
| M2 | M1平台与归一化Trace/真实截图/Design/named Profile | 检查与named Profile分别受Host flag控制，不伪造空成功 |
| M3 | 声明支持的Chrome版本/OS/扩展+native companion元组 | 未配对或无法归因仅人工/受限观察 |
| M4 | Linux发行版/桌面会话/包型精确矩阵 | 未验证组合显示unsupported；Wayland可维持受限beta但完整M4未通过，不能用--no-sandbox通过 |
| M5 | Host/Guard双持有loopback listener + OpenSSH ssh -W；POSIX仅身份已验证的Bridge专属mux，Windows独立进程 | remote ACP继续不注入local Browser MCP；新tunnel/revision不继承登录态 |
| M6 | App N-1/N、Runtime N-1/N、Profile epoch、online/offline、撤销/回滚 | 独立updater单独flag；CEF仍为决策结果 |

最低OS/发行版具体值由各平台计划的实验/打包任务锁定，并作为版本化支持矩阵提交；未取得对应设备证据前该行not_run，不根据本机“能启动”推断全部版本通过。

## 证据记录合同

未来记录放在 `docs/qa/browser-rearchitecture/<milestone>/`，每次运行独立runId。公开文档只引用去敏结果；大截图/trace/package通过受控artifact记录hash，不能把用户profile、账号、token、原始网络body写入Git。

```json
{
  "schemaVersion": 1,
  "requirementIds": ["BR-09"],
  "taskId": "M1-G04",
  "status": "not_run",
  "sourceCommit": null,
  "runtimeTupleDigest": null,
  "platform": null,
  "fixtureVersion": null,
  "command": null,
  "exitCode": null,
  "passedCases": 0,
  "failedCases": 0,
  "measurements": [],
  "artifactDigests": []
}
```

上例是待运行记录结构，不是通过证据。任务完成时实际字段必须填入运行事实，且read-only检查能复现对应结论。失败记录同样保存；不可删掉失败样本后只报告成功率。

## 统一发布检查

所有代码包遵守仓库 `docs/llm-wiki/maintain.md`。在提出PR前执行并通过：

```bash
pnpm typecheck
pnpm test
pnpm lint
python3 scripts/check-code-quality-gates.py --mode final
python3 scripts/publish-website-downloads.py --self-test
pnpm build:ui
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml
```

子包worker/MCP/extension的独立测试和真实package E2E也必须跑；根Vitest仅覆盖src，不能把根测试通过当子包测试已执行。Linux/Windows/macOS目标相关检查使用各平台runner。

现基线`FILES_OVER_1K_BUDGET=79/77`是已知失败。本计划将经手EmbeddedBrowser和session_lifecycle拆分列入M0-W02/W05，目标回到预算；若基线另有变化重新审计，不提高阈值。主App两文件总行数只下降。

UI设计/新面板按维护者审阅规则合并，功能开发中的本地commit同轮push。正式发版需CHANGELOG对应版本、release脚本生成说明和contributors更新；本次文档提交不发版。

## 完成判定

设计文档包完成：十份规格、十份计划、共用协议、六十任务无环依赖、全部BR需求映射、平台/发布/证据规则具备，文档验证通过并提交推送。

产品重构完成：上述开发任务已实现，所声明平台的运行证据逐项通过，相关已发布flag可用，回滚与恢复演练达标。两种完成状态独立，当前只交付前者。
