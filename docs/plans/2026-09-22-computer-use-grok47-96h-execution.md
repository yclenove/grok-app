# Computer Use：Grok 4.7 最长 96 小时连续执行计划

日期：2026-09-22  
状态：待执行  
固定工作区：`H:\aicoding\grok-app-computer-use`  
固定分支：`feat/computer-use-implementation`  
基线 HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`  
起始结论：**partial — not releasable**

## 1. 目标和完成定义

目标是把当前 Computer Use 从“Windows 上已有大量真实 fixture 和部分产品闭环”推进到
可审查的最终候选，而不是跑满 96 小时或堆出更多单元测试。时间是最大预算，不是成功
条件。若某个平台、真实模型、浏览器安装或系统权限缺失，应标明 `not_run` 或
`blocked_external`，继续完成所有不依赖它的 ready work。

只有同时满足下列条件，才允许把整体状态改成 release candidate：

1. 当前代码、runtime、extension、runner 和构建配置形成同一冻结 fingerprint。
2. Computer Use 默认关闭，模型永远不能自行授权/恢复/扩权。
3. Desktop、Managed Browser、App WebView、ExistingTab 四面都完成指定的产品路径；
   unsupported capability 必须明确拒绝，任何面都不回退 Desktop。
4. Windows、macOS arm64、macOS x64、Linux X11、GNOME native Wayland 按各自要求
   有实机证据；缺一项就不能叫“三 OS 首版完成”。
5. source 与 installed、Chrome 与 Edge、scripted agent 与真实模型分别记证据等级，
   不能互相替代。
6. 安装、修复、升级、回滚、卸载和 runtime 损坏恢复有安装态证据。
7. 同一冻结候选上至少 12 小时主动长稳；12 类任务每 OS 至少 5 次，总成功率
   `>= 90%`，且所有安全计数为 0。
8. 全仓门禁、隐私、bundle、license/NOTICE、i18n、代码预算全部通过；没有 placeholder、
   probe backdoor、测试私钥、临时 profile 或本机绝对路径进入提交候选。

若任何 required 项未达到，最终标题必须保持：

`partial — not releasable`

## 2. 证据等级与状态词

- `implemented`：代码存在，不等于通过。
- `E1`：unit/mock/jsdom/纯函数。
- `E2`：真实本机进程、pipe、HTTP/IPC、真实模块。
- `E3`：branch-built App/Host + 自建原生或浏览器 fixture，有独立后置条件。
- `E4`：安装 App + 真人授权 + 真实 Grok 模型。
- `E5`：多 target 安装生命周期 + 重复任务/故障矩阵。
- `not_run`：尚未运行。
- `blocked_external`：缺机器、系统权限、真人浏览器手势、真实账号或其他外部条件。
- `failed`：当前代码/候选真实失败；不能因后续另一用例通过而删除首败。

Exit 0、0 tests、skip/ignore、capability=false、空 target、grep/string presence、旧日志、
旧 fingerprint、cross compile、XWayland、浏览器 fixture 或“代码看起来存在”均不能提升
证据等级。

## 3. 全程安全与 Git 规则

- 单 writer；不得让其他 Agent/Cursor/Grok 同时写本工作树。
- 保留当前 240 行 dirty status；禁止 reset/restore/clean/stash/rebase/强制 checkout。
- 禁止 pull/merge 上游，禁止触碰其他 worktree 和仓库级 stash。
- 未经用户明确授权，禁止 commit/push/PR/merge/tag/release/deploy。
- 不操作真实账号、Cookie、Token、浏览器日常 profile、正式 Grok、共享 `~/.grok`、
  系统代理/VPN、用户聊天和文件。
- 不修改用户级 Chrome/Chromium policy 或 external-extension registry。安装态实验若
  需要系统级动作，转为 VM/临时用户方案或 `blocked_external`，不得猜测式试写。
- 不打印/提交任何 `.pem`、CRX signing key、pairing Bearer、MCP token、页面隐私内容。
- 清理仅限本轮创建且 owner.json、PID create-time、canonical path 全部匹配的资源；
  不按进程名、端口范围或 `.cu-probe-*` 通配符做破坏性清理。
- 测试必须使用隔离 `GROK_APP_HOME` 与 owned profile；借用 tab 永不关闭。
- 重型 Cargo、浏览器、App、安装器、runtime prepare 串行执行。
- 禁止 arbitrary shell/evaluate、Cookie/storage export、all_urls/debugger 权限膨胀、
  放宽断言、增加无条件 sleep、删测试、skip、blanket allow、关闭安全门禁。
- 不向 `src/App.tsx` 或 `src/app/AppWorkbench.tsx` 增加新的 feature state/大块逻辑；
  App shell + AppWorkbench 合计行数只能下降。

## 4. 证据根与断点续跑

开工后新建唯一、gitignored 的证据根：

`tools/computer-use-probe/.run/grok47/<UTC-run-id>/`

至少包含：

```text
owner.json
baseline/head.txt
baseline/status.txt
baseline/processes.json
baseline/ports.json
baseline/fingerprint.json
manifest.jsonl
state.json
logs/
failures/
checkpoints/
metrics/
reports/
artifacts/
```

规则：

- `owner.json` 记录 repo canonical path、branch、HEAD、host/OS/arch、writer PID、开始
  时间和精确 cleanup allowlist，不含秘密。
- `manifest.jsonl` append-only，一场景一行；first failure 永远保留，恢复只追加。
- `state.json`、checkpoint 和报告由 manifest 聚合，不能手写 passed 覆盖日志。
- 每次 candidate-affecting edit 后更新 source fingerprint；freeze 后任何源码、测试、
  runner、配置、runtime 或 extension 改动都使 smoke/soak 失效，必须 append invalidation。
- Goal/进程中断后从最后 checkpoint 继续，不重写历史。

## 5. 依赖图

```text
G0 现状冻结与安全清理复核
  -> G1 runtime seed 恢复为全绿
  -> G2 浏览器真实 startup/安装态语义
  -> G3 完整浏览器退出 cleanup 协议
  -> G4 renderer/reload/update/双 Host 组合故障
  -> G5 工具栏 activeTab + 截图正向链
  -> G6 App UI/ACP/session 生命周期产品闭环
  -> G7 Windows source/installed + Chrome/Edge + 真实模型
  -> G8 macOS arm64/x64
  -> G9 Linux X11 + GNOME native Wayland
  -> G10 隐私/诊断/性能/代码质量/文档收敛
  -> G11 全量门禁、freeze、12h 主动长稳
  -> G12 发布矩阵与最终审计
```

G8/G9 的可移植实现与测试准备可在等待外部机器时进行，但不能写 passed。G2/G3 未闭合
前，不再扩大 ExistingTab 动作集；G1 未恢复前，不把 Core 红灯归入后续代码修改。

## 6. G0：0–2 小时，建立可信基线

全文阅读：

1. `AGENTS.md`
2. `docs/llm-wiki/computer-use.md`
3. `docs/llm-wiki/i18n.md`
4. `docs/llm-wiki/dialogs.md`
5. `docs/llm-wiki/maintain.md`
6. `docs/plans/2026-09-22-computer-use-grok47-handoff.md`
7. 本文件
8. `2026-09-21-computer-use-browser-exit-recovery-adr.md`
9. `2026-09-21-computer-use-host-process-retirement-checkpoint.md`
10. `2026-09-20-computer-use-restart-and-mcp-execution.md`

操作：

- 核对 pwd/branch/HEAD/status/index/worktree/stash，但不改它们。
- 记录全部 tracked/untracked 分类，尤其区分产品源码、docs、runtime seed、证据、
  `.cu-probe-*`、PEM/CRX 和 build output。
- 检查残留 PID/端口/owned profile，只记录，不结束非本轮进程。
- 复核本交接所述注册表状态为：Google Chrome policy 只保留原有
  `QuicAllowed=0`、`DnsOverHttpsMode=off`；无测试 ID/force-list；不得再写。
- 计算 source fingerprint，覆盖 Rust/TS/JS/CSS/JSON/TOML/workflow/build scripts、
  extension、runtime locks 和计划指定 runner，排除 target/node_modules/.run/profile/secret。
- 重跑最小基线并记录精确结果：扩展 Node、Computer Use 定向 Vitest、typecheck、
  lint、Driver、App cargo check、runtime check、Core lib。

通过条件：证据根可恢复、首败已记录、没有动用户资源。G0 不要求红灯变绿。

## 7. G1：2–6 小时，修复 runtime seed 完整性红灯

当前已知 Red：

- `pnpm check:computer-use`：Chromium expected
  `63c607...`，actual `a38e94...`。
- Core：452/454；`installer_seed_repairs_from_shipped_resources` 和
  `prepare_check_fails_when_production_sibling_is_removed` 被该完整性错误抢先阻断。

步骤：

1. 只读比较 tree inventory、mtime、size、hash 与 lock，确定是探针写入、Chrome
   首次运行污染、硬链接污染、生成器 nondeterminism 还是源锁错误。不要先删目录。
2. 验证 runtime seed 是否被用作可写 profile 或运行目录；若 Chrome 会写安装 tree，
   修产品/探针让运行时只执行 immutable binary，用户数据全部到 owned profile。
3. 使用现有受 hash 约束的 `pnpm prepare:computer-use` 安全重物化；只能替换生成的
   ignored seed，保留旧树 inventory/hash 证据。不能手改 lock hash迎合污染树。
4. 连续两次 prepare 的 manifest/tree digest 必须相同；`--check` 必须只读。
5. 增加/修复 regression：运行 packaged browser 后 seed digest 不变；并发 prepare
   fail closed；损坏 tree repair 创建新健康 pack，不修改健康 previous。
6. 重跑 runtime 定向、Core 454、Driver 12、packaged contract 至少 3 轮和 bundle audit。

退出条件：`pnpm check:computer-use`、Core、runtime/bundle gate 全绿，seed 运行前后 hash
不变；若仍红，标 failed，保留首败并继续不依赖 runtime 的 G2 研究，不伪造 Green。

## 8. G2：6–14 小时，建立真实浏览器 startup/安装态语义

目标是得到可信、可重复的 Chrome/Chromium extension lifecycle 证据。当前
`--load-extension` 不足以证明 `runtime.onStartup`，过去的 force-list、手写 Preferences、
外部扩展注册表均失败，禁止重复猜测。

先做规范和源码研究：

- 核对 Chromium/Chrome 官方 `runtime.onStartup`、扩展 install/update/reload、service
  worker 生命周期、profile startup 的精确定义。
- 研究项目所用 Chromium 版本与系统 Chrome/Edge 的差异，以及可受支持的持久安装
  test API/企业策略要求；记录为什么过去 `[BLOCKED]`。
- 若自动持久安装必须用签名 CRX/update manifest，建立完全隔离的临时用户或 VM
  方案，不能写当前用户 HKCU policy/registry。
- 若本机无法自动化原生文件夹选择/安装确认，准备真人一步操作的严格 runbook，
  将自动部分推进到等待点并标 `blocked_external`，不要绕过用户手势。

最小真实矩阵：

| 场景 | 必须观察 |
| --- | --- |
| 浏览器运行中首次安装扩展 | 初始 BrowserSession、是否收到 onInstalled、不得伪造 startup |
| 首次完整退出并重启 | 是否收到一次 onStartup；必须建立可区分旧 browser owner 的新身份 |
| 第二次完整重启 | 新身份、previous identity、事件次数、持久化原子性 |
| 仅终止/重启 service worker | BrowserSession 身份不变 |
| `runtime.reload` | 不得冒充完整 browser startup |
| 扩展 update | 旧 world/新 worker/identity 语义明确，不能凭 storage 丢失清 owner |
| storage.local 读写失败/损坏 | fail closed，不能发布 startup 成功 |
| 重复/迟到事件 | 状态机确定、幂等或显式代际，不误退当前 owner |

修正 `BrowserSession` 的状态模型。特别处理“浏览器运行中首次安装、started=false、
第一次真实重启”边界。测试必须表达真实生命周期，而不是直接 fire listener 就算 E3。

通过条件：至少一个隔离持久安装 profile 的安装→完整退出→重启证据；SW restart/reload
负例；事件/存储/身份矩阵全绿。否则保持 `blocked_external`，不进入声称修复的 G3。

## 9. G3：14–24 小时，闭合完整浏览器退出恢复

先更新 ADR，明确：

- Browser identity 是启动边界，不是动作完成证据。
- cleanup journal 需要跨完整浏览器退出时，只保存有界、cleanup-only、不可恢复授权
  的元数据；不保存 pairing Bearer、页面内容、输入文本、完整 command、Cookie/token。
- 持久层优先扩展 origin 私有 IndexedDB 或经论证的等价方案；定义 schema/version、
  原子 commit、8 条容量、字节上限、迁移、损坏、删除失败、incognito/normal 隔离。
- claim 前必须 durable write 成功；写失败零 claim/零动作。
- 新浏览器只可在真实 startup identity + 当前新配对 + 原 Host exact pending/proof +
  原本地 physical/cleanup state 全部匹配时收束旧 owner。
- 收束结果只能是 cleanup/unknown，不能发布 applied/verified、恢复授权或重放动作。
- absence、404、超时、session storage 消失、documentId null、重新 Share 不能释放 owner。

真实故障矩阵至少覆盖：

- claim 回包滞留；claim 前 journal 写回包滞留；native 注入前；真实 Wait；click 已应用
  但回包滞留。
- 正常完整退出和强制终止；old effect 分别 0/1，新动作必须恰好 1。
- 先关浏览器/后关 Host、先关 Host/后关浏览器、两者同时、Host 保持存活。
- 丢失 register/retire/settle 回复；错误 previous identity；旧配对；错误 proof；新 owner
  已占同 tab；journal 写/删失败；容量满；schema 旧版/损坏；incognito。
- 旧 pending 最终归还；旧动作永不重放；新配对/Share 在旧 owner 未证明结束前不得绕过。

修复 `existing-tab-browser-exit` gate，并把 PASS 检查数从实际数组生成。连续运行至少
5 次，任何一次首败都保留。重跑 Node、Core、Driver、观察/MCP、Host/SW 重启、BFCache
和 completion 回归。

## 10. G4：24–32 小时，组合生命周期故障

在 G3 的协议上继续，不另造第二套 cleanup：

- renderer crash/restore；extension reload/update；旧 isolated world；旧版本无 guardian
  记录；Chrome 已接受但未执行的注入；页面进 BFCache 后浏览器退出。
- 两个真实 Host 同时存活；Host retirement 回复丢失；真实 storage 删除失败；PID reuse
  身份负例；App + SW + browser 三者不同顺序。
- App update/restart、feature off、logout/provider/data-root 切换、session delete、fork、
  compact、换模型、ACP exit/reconnect、background/parked/shared ACP tenant。
- 每个入口先同步 fence dispatch/credential，再异步 cleanup；旧回调不得作用 replacement。
- Stop 响应不等待浏览器网络超时，但 `stop_requested` 到 `stopped` 必须等待真实 physical idle。

每一行记录旧/新 credential oracle、Broker grant/lease/pending、MCP catalog、页面独立计数、
tab/profile/PID/port、cleanup ledger、late callback。错误目标、post-fence dispatch、旧动作
重放必须为 0。

## 11. G5：32–38 小时，工具栏 activeTab 与截图正向链

- 使用真实 browser toolbar 用户手势在 owned local fixture 上 Share/Unshare；直接打开
  popup.html 不算 activeTab 授权。
- 明确截图只允许当前显式分享且仍 active/visible 的 tab；前后复验 tab/document/window
  activity，不偷切 tab，不捕获别的 tab。
- 正向验证 PNG signature、dimensions、viewport、snapshot/generation；页面像素 oracle
  或已知 marker 验证内容确来自正确 tab。
- 覆盖 permission deny、切 tab、切窗口、scroll、reload、unshare、close、capture 超时、
  大图、focus away-and-back、并发 preview/model observe。
- preview 隐藏立即停轮询；旧帧不能闪回；preview 不覆盖模型 refs。
- Chrome 和 Edge 分开；Chrome for Testing 不能替代 installed Chrome/Edge。

若缺 native UI automation，停在明确的真人一步 runbook，不使用内部 API 伪造手势。

## 12. G6：38–48 小时，App UI/ACP/session 产品闭环

用 branch-built Tauri App、隔离 home、真实 Tauri commands 和 session MCP，不直接调用
Core helper 冒充产品：

1. 默认关闭；两条 slash 只打开面板，不发送假聊天命令，也不自动授权。
2. 设置开关、配对码、App 确认、extension 确认、Share、picker、authorize、MCP catalog
   注入的完整用户链。
3. ExistingTab `observe → click/set_value/type_text/scroll/wait → observe verify → stop`；
   独立页面 oracle 验证副作用，unsupported key/drag/coordinates/clipboard 明确拒绝。
4. Managed Browser 和 WebView 的同等基本交互；无 surface fallback。
5. busy/error/empty/expired/retry/keyboard/focus/dialog/overlay/hidden-preview/stale-frame 路径；
   所有文案走 15 locale，无 OS 默认 select/menu、无 window alert/confirm/prompt。
6. session switch、background、delete、fork、compact、换模型、Stop、App exit、ACP crash/
   reconnect、feature off/on；旧 credential/catalog/callback 不复活。
7. UI task card 只显示真实 target/backend/state，不把 tool ok 当 verified。

至少两个全新隔离 App home 各跑两轮；退出后检查 PID/port/profile/temp/lease/token/binding
无 orphan。

## 13. G7：48–58 小时，Windows source/installed/真实模型

在 Windows 上分别记录：

- source App + packaged runtime；
- installed App clean install/first start；
- installed Chrome extension；
- installed Edge extension；
- same-version repair；上一版本 upgrade；失败 rollback；uninstall；
- 真实 Grok 模型 + 真人授权 E4。

安装包必须使用隔离 identifier、安装目录、App data root，不覆盖用户正式 Grok。任何涉及
真实模型、浏览器安装确认或系统权限的步骤只在用户明确介入时执行；等待期间完成自动
部分，不读取/导出登录 Cookie/Token。

四 surface 各至少 20 个产品轮次，覆盖成功、拒绝、取消、超时、worker/Host/browser kill、
window/tab close、App exit 和资源清理。真实模型必须通过会话 MCP 自己选择和调用工具，
不能用 scripted agent 代替。

## 14. G8：58–68 小时，macOS arm64/x64

代码准备：

- AX 元素身份、语义 click/set_value/type_text/key/scroll/drag/wait；删除固定坐标伪语义。
- CGWindow capture、Retina scale、多屏/负坐标/窗口移动与 resize。
- Accessibility/Screen Recording TCC 缺失/撤销/重授；签名 identity 与更新后权限。
- arm64/x64 target-specific Node/Playwright/Chromium/driver lock、hash、license/NOTICE、
  immutable seed、repair/rollback。

实机：两个架构分别构建/安装/启动；至少一个架构完成四 surface + 真实模型 E4，另一
架构至少安装/权限/runtime/基本动作。未上机一律 `not_run`，Windows/交叉编译不能替代。

## 15. G9：68–78 小时，Linux X11 与 GNOME native Wayland

X11：窗口身份、截图、DPI/multi-monitor、click/set_value/type_text/key/scroll/drag/wait、
焦点/用户接管、取消/Stop、clipboard restore、AppImage 安装和 runtime 生命周期。

Wayland：必须采用 native portal ScreenCast + RemoteDesktop 和 libei/EIS 或经验证的等价
方案，明确 session/consent、PipeWire frame、input identity、显示器变化、撤销、App
重启、portal crash 和资源回收。`native_wayland` 在真实动作后置条件通过前保持 false。

XWayland、浏览器、截图-only、portal token 存在或 cross compile 都不能算 native
Wayland。缺 GNOME 环境时完成 protocol/helper/tests/runbook，标 `blocked_external`。

## 16. G10：78–84 小时，质量、隐私、性能和文档收敛

- 更新 Computer Use wiki、execution-state 顶部、extension INSTALL；保留历史，但把
  ExistingTab 五动作的当前事实与未发布边界写准。
- 修 `checks=37` 硬编码；所有 gate 数量来自实际 expected list，报告读取 manifest。
- 审计失败实验：三个 probe 脚本要么删除，要么限制到 owner-verified temp root并写测试；
  `.cu-probe-*`/PEM/CRX/profile/证书全部被 ignore 且 bundle audit 拒绝。
- 拆分超 1000 行的 Computer Use 大文件；减少 AppWorkbench；不做 forwarding-only 拆分。
- trace/support bundle 有记录数/字节/时间上限，二次 secret scan；默认不持久化 screenshot、
  页面正文、输入文本、剪贴板、URL query、Cookie、token、绝对私有路径。
- symlink/junction/path traversal/zip bomb/硬链接污染/cleanup path 验证。
- 采集 observe/capture/act/verify/stop 的 p50/p95，preview latest-frame-only 和背压；
  不为追求均值放宽安全超时。
- 15 locale key parity、设置 catalog/deep link、dialogs/overlay、键盘可访问性、缩放/DPI。
- 审计 ZCode/DSH/Pi/Claude Code/Codex 参考只用于架构对照；不得复制不兼容许可证或
  引入 placeholder runtime。

## 17. G11：84 小时后，完整门禁、freeze 与 12 小时主动长稳

在最后一次 candidate-affecting edit 后串行运行：

```text
git diff --check
pnpm deps:check
pnpm audit:prod
pnpm typecheck
pnpm lint
pnpm test
pnpm build:ui
python scripts/check-code-quality-gates.py --mode final
python scripts/publish-website-downloads.py --self-test
pnpm check:computer-use
node scripts/audit-computer-use-bundle.mjs
node --test tools/computer-use-extension/*.test.mjs
node --test tools/computer-use-browser/*.test.mjs
node --test tools/computer-use-mcp/*.test.mjs
cargo fmt --all -- --check
cargo clippy -p grok-computer-use-core --all-targets --all-features --locked --offline -- -D warnings
cargo test -p grok-computer-use-core --all-features --locked --offline
cargo test -p grok-computer-use-core --all-features --test driver --locked --offline -- --test-threads=1
cargo check -p grok-app --all-targets --locked --offline
cargo clippy -p grok-app --all-targets --locked --offline -- -D warnings
cargo test -p grok-app --lib --locked --offline --no-run
```

Windows App test harness 仍须使用 CI 同款 `mt.exe` post-link manifest；`0xc0000139` 是环境
启动错误，不是测试通过。四 target runtime check、bundle/package audit、App-shell smoke
和安装态 smoke 也必须使用同一源码 fingerprint。

freeze 后执行至少 12 小时主动场景；build、sleep、idle、等待用户、旧 fingerprint 和
单纯 soak timer 不计时：

| 场景 | 最低主动时间 | 最低轮次 |
| --- | ---: | ---: |
| Windows Desktop App-shell | 2h | 60 |
| Managed Browser product | 2h | 80 |
| ExistingTab product | 2h | 80 |
| App WebView product | 1h | 60 |
| Stop/restart/fault matrix | 2h | 80 |
| privacy/runtime/package mutation | 1h | 60 |
| macOS/Linux 实机合计 | 2h | 每环境至少 30 |

每类保存 rounds/ok/fail/first failure/consecutive failure/p50/p95/resource trend。普通任务
成功率 `>=90%`；以下必须全部为 0：

- unauthorized read/write；wrong target；post-stop/post-fence dispatch；
- stale credential/old generation accepted；unknown 自动重放；
- old cleanup/exit 伤害 replacement；base MCP 丢失或 stale CU catalog；
- Cookie/token/secret/private URL/input/path/screenshot 泄漏；
- borrowed tab closed；owned orphan PID/port/profile/lease/token/listener；
- XWayland 冒充 Wayland；mock/source 冒充 installed/real-model。

任何代码修复使 freeze 与累计 soak 失效；保留旧失败，重新 fingerprint 和完整累计。

## 18. G12：发布矩阵和最终报告

发布矩阵必须逐行给出状态、证据等级、fingerprint、日志和阻塞：

- Windows x64 / macOS arm64 / macOS x64 / Linux x64；
- Linux X11 / GNOME native Wayland；
- source App / installed App；Chrome / Edge；extension source / signed installed；
- Desktop / Managed / WebView / ExistingTab；
- scripted agent / 真实 Grok 模型；
- clean install / first start / same-version repair / upgrade / rollback / uninstall；
- 网络/权限/worker/Host/browser/App crash；
- 12 类任务 × 每 OS 5 次。

最终报告必须包含：

- repo/branch/HEAD、完整 dirty inventory、baseline/freeze/current fingerprint；
- G0–G12 每个原子项的 implemented/verification/evidence；
- exact test counts、命令、exit、first failure/recovery、flake；
- 四 surface、四平台 target、Chrome/Edge、source/installed、scripted/real-model；
- pairing/startup/browser-exit threat model 与负例；
- runtime/bundle/install inventory、来源/hash/license/NOTICE/SBOM；
- soak active wall/rounds/success/failure/safety counters/resource trend；
- warnings、技术债、文档漂移、external blocker 和可直接执行的恢复步骤；
- 清理了什么、保留了什么、为什么；
- 原样写：`未 commit / 未 push / 未提 PR`。

## 19. 持续执行纪律

- 每个 atomic batch 后、且至少每 60–90 分钟写 checkpoint，然后自动进入下一 ready
  batch，不问“是否继续”。
- 同一失败同一假设最多三次；每次必须有新证据。三次后保留首败，转到不依赖它的
 工作，不能无限重跑。
- 每个 checkpoint 必须列 fingerprint、修改文件、manifest seq、精确测试数、首败、
  hypothesis/recovery、资源 owner/cleanup、平台证据、下一动作与 Git 边界。
- 若长任务提前实现代码，继续做 mutation、negative、race、leak、安装态和主动长稳；
  不得因“计划跑完”提前宣布完成。
- 只有用户明确停止，或所有剩余项都真正依赖缺失的外部机器/真人权限且没有其他 ready
  work，才停止。此时给出精确阻塞与恢复命令，不把 blocked 写成 passed。
