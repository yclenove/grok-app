# Computer Use v2 外部安全审阅包

审阅材料基线：`edccc9c84e38f2cbcdad333503380fa071807cfc`，2026-10-19。原 [security.md](security.md) 自标基线为 `f42a6eaaf4a01a8bf56e1dd8c8f6602a4c828306`（2026-10-17）；后续修复只按 [STATUS](../../docs/plans/computer-use-v2/STATUS.md) 与当前源码注明。这个包是审阅输入，**不是通过结论或签字**。本次 R1 不运行原生/模型验收，不改变 security.md、data-inventory.md 或 STATUS 的状态；尚未运行的验收仍为 not_run。

## 范围与阅读顺序

范围是 [07 §7](../../docs/plans/computer-use-v2/07-quality-gates.md) 的十项 S1–S10、P9-04 的 G1–G7 修复及剩余证据，目标是判断 [03 §7/§15](../../docs/plans/computer-use-v2/03-architecture.md) 的边界是否被实际产品链保持。安全审阅与 [发布准备度](release-readiness.md) 的成功率、性能、兼容性、签名门槛相互引用，不能相互替代。

首轮可限定路线 A macOS / L0，但请明确区分 CLI/helper、App/engine、worker 和平台 feature。未验收的 L1 内置浏览窗、Wayland 产品路线及 L2 不作为已发布能力；审阅其关闭态和不可达边界，未来开放前仍须独立验收。**ADR 0014 已退出，L2/A4b 没有可继续实施的绑定路径。** 不把“固定扩展 ID”“浏览器一次 UIA 读到 nonce”或模型反例测试通过当作生产 DOM 通过（STATUS ADR 0014 退出记录；下列 ADR）。

建议依次阅读：

1. [security.md](security.md)：十项防线、源码测试名、当前状态、G1–G7 和外部签字空栏。注意测试声明存在不等于本轮运行通过。
2. [data-inventory.md](data-inventory.md)：D1–D17 的所有者、内存/落盘、Unix 模式/Windows DACL、保留期、撤销与删除的区别；尤其 D5/D6 导出、D10 Stop、D13 token、D15 journal。
3. [ADR 0003 可信恢复](adr/0003-resume-trust.md)：TTY/token/同 UID 不能证明人类，MCP 无恢复工具。
4. [ADR 0006 TCC/helper](adr/0006-macos-tcc-helper.md)：只对 macOS 27 arm64/SIP 开的探针结论成立；Chromium 后台点击不可用，正式身份不能用 identifier-only 代替。
5. [ADR 0012 原生装配](adr/0012-real-driver-assembly.md)：读取决定与 A0/A0-M/A1/A2/A3/A5 要求，再结合 STATUS 后续实现；其旧“现状事实表”不是当前验收证明。
6. [ADR 0013 Windows Stop 租约](adr/0013-windows-stop-guard-liveness-lease.md)：已接受方案 B 及接线增补；区分传输健康、100ms 注入租约、活动可信，保留物理探针副作用与 not_run。
7. [ADR 0015 完整持久身份](adr/0015-persistent-grant-identity.md)：单一 persistent_eq、缺证据不匹配、更新重审、旧窄记录保留但不授权。
8. [ADR 0014 的退出记录](adr/0014-l2-window-binding-by-challenge-marker.md)与 [M0 §5–§7](adr/0014-m0-feasibility.md)：Windows×Chrome 限定观察、D3 残留事实、T04 最终投递竞态与反例。结论是保持 L0、关闭 L2/A4b，不重新批准挑战绑定。
9. [script-sandbox-review.md](script-sandbox-review.md)、[ADR 0004 原生完整性](adr/0004-raw-tree-completeness.md)、[ADR 0004 Host 认证](adr/0004-host-auth-dependencies.md)、[ADR 0005 worker](adr/0005-windows-worker-json.md)，然后按下表走关键代码。
10. 按涉面阅读 [ADR 0001 MCP](adr/0001-grok-build-mcp-capabilities.md)、[0002 错误/能力](adr/0002-driver-interface-extension.md)、[0007 PipeWire 例外](adr/0007-ashpd-pipewire-deny.md)、[0008 映像哈希](adr/0008-linux-image-hash-budget.md)、[0008 EI 输入](adr/0008-reis-portal-input.md)、[0009 WebView 依赖例外](adr/0009-wry-onboarding-webview-deny.md)、[0010 关闭态 L1](adr/0010-builtin-browser-window-protocol.md)、[0011 截图默认](adr/0011-auto-screenshot-policy.md)。批准设计不等于批准发布；依赖例外须核对退出条件。

| 审阅线 | 关键代码位置 | 应追踪的实际边界 |
|---|---|---|
| 身份、拒绝类别与风险 | [grant.rs](../crates/gcu-protocol/src/grant.rs)、[identity.rs](../crates/gcu-core/src/policy/identity.rs)、[surface.rs](../crates/gcu-core/src/policy/surface.rs)、[target.rs](../crates/gcu-core/src/policy/target.rs)、[risk.rs](../crates/gcu-core/src/policy/risk.rs) | 实际映像/证书/产品/hash → 授权；全部动作路径与未知执行界面；不依名称信任 |
| 审批与路由 | [action](../crates/gcu-core/src/action)、[approval.rs](../crates/gcu-core/src/policy/approval.rs)、[Office](../crates/gcu-host-protocol/src/approval/office)、[CLI page](../crates/gcu-cli/src/host/page.rs) | 真实用户事件 → 单次消费/持久化；参数/收件人/内容/快照/策略版本投递前重验；unknown 不重放 |
| Stop 与产品装配 | [assemble.rs](../crates/gcu-cli/src/assemble.rs)、[helper/macos.rs](../crates/gcu-cli/src/helper/macos.rs)、[session/stop.rs](../crates/gcu-core/src/session/stop.rs)、[overlay/safety.rs](../crates/gcu-host-protocol/src/overlay/safety.rs)、[stop_reason.rs](../crates/gcu-host-protocol/src/stop_reason.rs) | 相同 Host latch/活动源；独立停止释放/持久化；非正常断线/重启仍停；恢复不是模型工具 |
| Gateway / Host / 图片归属 | [http.rs](../crates/gcu-mcp/src/http.rs)、[auth.rs](../crates/gcu-host-protocol/src/auth.rs)、[transport.rs](../crates/gcu-host-protocol/src/transport.rs)、[lifecycle.rs](../crates/gcu-host-protocol/src/lifecycle.rs)、[inject.rs](../crates/gcu-host-protocol/src/inject.rs) | 127.0.0.1/精确 Host-Origin/token → 预期进程实例双向认证 → 会话隔离/先撤权再清理；FD 和 ACP bearer 的保密范围 |
| 图像、日志与存储 | [tree](../crates/gcu-core/src/tree)、[redact](../crates/gcu-core/src/redact)、[owner_only.rs](../crates/gcu-host-protocol/src/owner_only.rs)、[gallery_cleanup.rs](../crates/gcu-host-protocol/src/events/gallery_cleanup.rs)、[log_policy.rs](../crates/gcu-host-protocol/src/log_policy.rs)、[App logging](../../src-tauri/src/logging.rs)、[journal_cleanup](../../src-tauri/src/journal_cleanup.rs) | 先敏感定位再裁树/同状态打码；衍生物不可绕过；权限/链接替换/失败重试；日志 scrub 不等于 journal 内容清洗 |
| 脚本 | [runtime.rs](../crates/gcu-script/src/runtime.rs)、[limits.rs](../crates/gcu-script/src/limits.rs)、[rpc.rs](../crates/gcu-script/src/rpc.rs)、[script_worker.rs](../crates/gcu-cli/src/script_worker.rs) | 构造器逃逸/窄 RPC/资源上限/取消/独立 kill；Unix-only runner 与 feature 关闭态 |
| 原生权限/验签 | 各 driver 的 identity/gate/input/wayland 模块；[codesign.rs](../../src-tauri/src/computer_use/codesign.rs)、[supervisor.rs](../../src-tauri/src/computer_use/supervisor.rs) | TCC 责任链；Medium/Default/UIAccess；正式 verify 后才采信显示身份；Dev 独立授权 |
| L1/L2 关闭态 | [builtin_browser](../crates/gcu-protocol/src/builtin_browser/mod.rs)、[native_host/proof.rs](../crates/gcu-cli/src/native_host/proof.rs)、[gate.rs](../crates/gcu-cli/src/native_host/gate.rs)、[browser/router.rs](../crates/gcu-core/src/browser/router.rs) | 不把 capability JSON、页面字段、URL/title/fingerprint 当认证；不存在已验收的 DOM 生产投递 |
| 供应链/发布判定 | [deny.toml](../deny.toml)、[CI](../../.github/workflows/computer-use.yml)、[release workflow](../../.github/workflows/release-computer-use.yml)、[sbom.rs](../xtask/src/sbom.rs)、[release.rs](../crates/gcu-eval/src/release.rs) | 当前锁版本/具名例外/完整 SHA/PPA 信任/实际 SBOM 与签名；纯发布判定不验证证据真伪 |

## 威胁模型摘要

以下是 [03 §7/§15](../../docs/plans/computer-use-v2/03-architecture.md)、security.md 与上述 ADR 的摘要；不是新增或缩小威胁边界。

| 攻击者 / 资产 | 必须保持的防线 | 保证边界和审阅重点 |
|---|---|---|
| 提示注入页面/夹具、失误或恶意模型；目标是终端执行/凭据/未确认提交 | OS 权限、App 授权、ActionRouter 风险/拒绝类别、未知 handoff、所有 single/batch/script 路径同门禁 | 网页/IDE 执行宿主识别尽力而为、非完备；模型说“完成”不是 oracle；L0 只拦 gcu 自身危险输入，不控制页面内部重定向 |
| 同机客户端/网页伪造 Host、helper、MCP 会话或图片 ID | 精确 loopback Host/Origin、256-bit bearer、双向 HMAC、audit-token 签名/cdhash、预期进程实例、端点防抢占、每会话图片归属和撤销 | UID/文件权限不证明可信身份；同 UID 修改受信二进制/进程内存不在此认证保证内；ACP 客户端必然收到 header 副本 |
| 身份更新/路径替换/同名发布者/插件 DLL | 完整 grant identity、有效签名/证书/产品/hash、更新重审、未知/旧窄记录不放行 | 入口签名不证明所有插件/DLL 可信；旧授权迁移、运行实例/实际映像须原生链验证 |
| 审批/动作/Stop 的并发和故障 | 审批绑定、投递前重验、独立 Stop ≤100ms 锁存/≤500ms 发释放、≤5s 原生超时/杀 worker、输入账本、持久停止、用户专属恢复 | 逻辑时钟不证明物理时限；TTY 可被同 UID shell 模型伪造；Windows 同位置探针可能触发 hover；不得加预算刷绿 |
| 图像/密码/token/载荷泄漏和残留 | 原生完整性→敏感定位→同步打码→全部衍生物，定位不可靠拒图；owner-only；撤权/清理、字段/bearer scrub | 没有内存 zeroize/安全擦除保证；journal 保存原文；OS dump/剪贴板历史、备份、外部模型/客户端已收副本不受本会话删除控制 |
| 恶意脚本/资源耗尽 | 新上下文、构造器 lockdown、无 IO/网络/环境绑定、独立受限 Unix worker、窄 RPC、JS/宿主/队列/输出/总内存/驱动次数上限 | Windows Job 未交付所以不安装 runner；nightly fuzz/真机/script soak 未齐；脚本返回内容仍可能含用户自己提供的秘密 |
| 不可信远程 WebView、DOM/原生窗口错配、portal 来源不明 | 未验收 L1/Wayland 路线关闭；L2/A4b 关闭；来源与 Stop 无法证明即拒绝 | ADR 0014 最终检查与副作用不原子，不能用更多采样/更短 TTL 解除关闭；不将页面载体当身份证明 |
| 依赖/构建/签名污染 | lockfile、cargo-deny、具名例外、action SHA、PPA 固定版本、逐平台 SBOM、签名公证/干净安装 | SHA 固定不证明 action 本身可信；PPA APT 密钥信任与实际发行产物仍需审阅；无证书不得伪造正式链 |

## G1–G7 的处理与待审证据

下表只索引修复及剩余验收，不把“已合并”写成“安全项已签”。PR 编号来自 STATUS P9-04，未在线查 PR/CI；对应运行 artifact 未确认。

| 缺口 | 已有处理 / 证据 | 剩余证据与关闭者 | 复现入口 |
|---|---|---|---|
| G1 持久身份 | STATUS #135、ADR 0015；grant::persistent_eq；路线 A/B 共用；缺证明/Invalid/Unknown/旧窄记录均拒绝 | 同 Team 换 cdhash、同发布者换路径/hash、Linux 原地换 hash、旧记录重批及真实审批 UI：需要真机用户在场 + runner；外部本人审阅 | C1、P1/P2/P3 |
| G2 装配/停止原因 | STATUS A0/A0-M/A1/A2/A3（#127 前后）；默认 auto、feature 默认关、无 fake 回退、同 Host latch、异常断线持久 Stop；FileLatch 路径加固并入 G3 | helper/engine 完整原生链、按住输入/监听死亡/物理时限/重启仍停：需要真机用户在场 + runner；ADR 0013 单探针通过不能代替 | C2、P1/P2/P3/P4 |
| G3 权限/路径 | STATUS #139；owner_only 0700/0600、Windows 当前用户+SYSTEM、目录 OICI、随机 create_new/原子 rename、拒绝 symlink/hardlink | Windows 每产品路径 DACL、升级已有文件、CLI 状态目录/token/管道：需要 Windows runner/真机；不能只凭 helper 读回覆盖全部路径 | C3、P2 |
| G4 清理 | STATUS #139；图库 .closed 标记/失败可见并启动重试、孤儿 60s 宽限、CLI keep +3600 到期戳、journal 删除重试、前端 Stop/turn/session 淘汰 crop | 强杀/磁盘/ACL 失败/撤销后物理清理、真实 compositor：需要对应 runner/真机；无常驻到期删除保证 | C3、F1、P1–P4 |
| G5 载荷日志 | STATUS #134；log_policy 唯一读取开关、Debug 不吐像素/文本、panic 类别/位置、7 天和 5×5MB；#139 后日志也接 owner_only | 无字段名自由文本密码、所有日志调用点/OS dump/任意秘密全链均未证明；D15 原文保留由用户 2026-10-19 决定，仍需外部本人审阅 | C4、F1、P1/P2 |
| G6 正式引擎验签 | STATUS #137：verify strict（bundle deep）后采信显示字段，5s 硬超时，Team requirement 单参数 `-R=expr`；有 macOS ad-hoc/篡改/真实语法测试 | 真实 Developer ID 正向链、同名自签真证书、Dev TCC 产品链：需要证书或账号的维护者 + Mac 用户在场 + runner；外部审阅者核查 | C5、P5 |
| G7 供应链 | STATUS #138：jammy 三个 PipeWire 包固定 `1.0.7-3~ubuntu22.04`、缺版本失败；已有逐平台 CycloneDX 步骤及 deny 历史通过 | 当前候选完整 deny、APT 信任、真实 release（含 arm64 安装）、SBOM 内容、包签名公证：runner + 证书/账号维护者 + 外部审阅者 | C6、P6 |

**原文与后续修复的差异必须保留。** security.md 的 S5/G4、data-inventory D15 还写 journal 内容策略 needs-decision，STATUS 已记录保持原文的决定；G6 的 `-R==…` 已按 STATUS/当前 codesign.rs 修为 `-R=expr`；G4 创建/加锁竞态、G5 Windows 日志 DACL 描述也早于 #139 的后续修正。原样引用在下文，不能照旧参数执行，也不能删掉旧缺口后直接签字。请核对当前代码、对应回归和平台产物；未确认处仍写“未确认”（详见 [发布准备度](release-readiness.md)“历史证据差异”）。

## 复现命令与证据分级

本节是**审阅者运行入口，不是本次执行结果**。Rust 命令默认在完整 `computer-use/` 检出串行执行；前端/计划命令在仓库根。审阅者本机有受支持 Rust/系统依赖时可用下列原生 `cargo`，不用生产账号、个人 agent home 或真实桌面数据。开发团队 Linux 执行工程师编译必须改用 `rcargo <参数>`，且一次只跑一个 `-p`。远程部分同步树缺 App/workflow 时，不能核验全仓安全索引。

### 本机可跑：源码、逻辑、临时目录、fake 进程

运行环境不满足依赖时记 not_run。下列测试即便通过，仍仅证明各自逻辑/进程/临时目录范围；不会给真实平台、模型任务或安全签字背书。

| 编号 / 对应项 | 命令（逐条串行） | 应核对的复现证据 / 限制 |
|---|---|---|
| C0 索引与签字完整性；S1–S10 | `cargo test -p gcu-eval --test security` | `security_checklist_ten_items_have_traceable_evidence_and_honest_status`、`both_documents_reference_existing_sources_and_test_declarations`、禁止假签的负例。仅查引用/状态/空签；不执行被引用测试。必须完整仓库，无 .github/App 时非 CI 可打印 SKIP 返回，不能算核验通过。 |
| C1 S1/S7、G1 | `cargo test -p gcu-protocol`；`cargo test -p gcu-core --test policy --test router --test batch`；`cargo test -p gcu-cli --test approval` | persistent_eq 及完整身份/旧记录；`denylist_builtin_identities_refuse_or_hand_off`、`ide_execution_paths_refuse_and_a_miss_hands_off`、`blocked_chords_refuse_even_when_confirmation_is_off`；审批在参数/策略变化时零旧目标投递。身份 facts 和桌面多为 fake。 |
| C2 S2/S3、G2 | `cargo test -p gcu-core --test stop`；`cargo test -p gcu-host-protocol`；`cargo test -p gcu-cli --test engine_host --test helper`；`cargo test -p gcu-cli --lib engine_stop_tests` | 跨轮/排队/重连/重启不恢复、独立 Stop 不排队、双向认证/端点替换、stop_reason/FileLatch；Unix/Windows cfg 的原生端点须各自平台运行。TTY 门槛不是人类证明。 |
| C3 S3/S5、G3/G4 | `cargo test -p gcu-mcp --test gateway --test media --test transport`；`cargo test -p gcu-host-protocol owner_only`；`cargo test -p gcu-host-protocol gallery_cleanup`；`cargo test -p gcu-cli keep_lease` | 精确 Host/Origin/重复 Host/token/跨会话图片拒绝；临时图 EOF 清理；权限失败、链接替换、孤儿宽限/持锁保护、图库删除失败重试、CLI 到期。Unix 通过不证明 Windows DACL；helper 单测不覆盖每个 App 路径。 |
| C4 S5、G5 | `cargo test -p gcu-host-protocol --test log_policy --test inject`；`cargo test -p gcu-core --test redact --test payload_debug`；`cargo test -p gcu-driver-linux --test capture_debug`；`cargo test -p gcu-cli expired_recent_calls_file_is_unlinked` | 默认 metadata-only、置 1 仍字段/bearer scrub、唯一读值点；裁树前敏感定位与全部安全图派生、缺 frame/incomplete 拒图、Debug 不吐载荷。自由文本密码/OS dump/真实 journal 不因此已清洗。 |
| C5 S9、G6 的可注入逻辑 | 在完整 `src-tauri/`：`cargo test -p grok-app computer_use::` | `release_verify_failure_timeout_or_missing_codesign_is_refused`、`release_self_signed_same_name_failing_requirement_is_refused` 等用注入输出证明准入。macOS 原生 codesign 另见 P5，不能将 fake codesign 结果写作正式证书通过。需对应平台 Tauri 构建依赖。 |
| C6 S10、G7 | `cargo deny check`；`cargo run -q -p xtask -- check-deps`；`cargo run -q -p xtask -- golden --check`；`cargo run -q -p xtask -- budget`；`cargo run -q -p xtask -- release --dry-run`；`python3 packages/test_packages.py` | 当前 lock/advisory/source/具名例外、依赖边界、golden/目录预算、发行结构；记录完整输出和首次失败，不靠重跑刷绿。dry-run 不签名/公证、不产生通过的真实 SBOM/安装证据。 |
| C7 S4（仅 Unix 安装 script runner）、G2 | `cargo test -p gcu-script --no-default-features --test sandbox`；`cargo test -p gcu-cli --features script --test script_router` | 构造器逃逸/内存炸弹/宿主 32MB/2MB 输出/并发1/队列32/200 次调用/Stop/unknown 不重放；非 Unix 不安装 runner。Unix 进程限额测试需真实进程条件，不声称桌面通过。 |
| C8 S8 与 L6 判定器 | `cargo test -p gcu-eval --test runner --test task_catalog --test browser_suite`；`cargo run -p gcu-eval -- --suite red_team --env desktop`；`cargo run -p gcu-eval -- --suite app --env desktop` | oracle pass/fail/partial、CU 外文件/进程/网络违规检测；DesktopProvider 当前返回 not_run。最后两条是复现“未执行”的入口，不能计作真实模型零违规。 |
| C9 L2 关闭态、ADR 0014 退出证据 | `cargo test -p gcu-cli --test native_host_proof --test native_host`；`cargo test -p gcu-core --test dom_router`；仓库根 `corepack pnpm exec vitest run computer-use/tools/browser-extension/tests/challenge-marker.test.ts computer-use/tools/browser-extension/tests/t04-dispatch.test.ts` | 固定 key/错 scope/Unproven 拒绝；页面可读载体反例、T04 延迟通知错配是**应被检出的设计反例**。测试绿不等于候选绑定安全，不启用 L2。M0 旧记录 Vitest/tsc not_run，Node 辅助模型 4074 组合不代替 Vitest/浏览器验收。 |
| C10 发布证据校验器 | `cargo test -p gcu-eval --test budget --test compatibility --test soak`；`cargo run -p gcu-eval -- --release-gate evidence.json --out /tmp/cu2-review-gate` | 合成边界/缺证据拒绝与纯判定；必须先有真实 evidence.json。空白 not_run 模板见 release-readiness.md，不能以虚构整数补齐；release-gate 有效输入即退出 0，要看 tier/promote/reasons。 |

### 匹配平台、物理桌面或证书才能跑

这些入口均须用户在场处理权限/安全 UI，使用一次性 OS/桌面、系统 App/夹具和无登录 profile。先按 [fixture README](../apps/fixture/README.md)、对应平台测试说明/CI 设置独立状态文件、原生夹具及产物目录；缺前提记 not_run。不在 build8 启动桌面或长稳常驻，不在日常桌面执行会覆盖剪贴板/输入的测试。

| 编号 / 对应项 | 命令 / 复现步骤入口 | 必需环境、独立证据与尚未覆盖内容 |
|---|---|---|
| P1 macOS S1/S2/S5/S6/S9、G1/G2/G4 | `cargo test -p gcu-driver-macos --test identity --test targeted --test stop`；`cargo test -p gcu-driver-macos --test contract -- --ignored --test-threads=1 --nocapture`；`cargo test -p gcu-cli --test helper` | 已启动夹具、正确 GCU_FIXTURE_APP/GCU_FIXTURE_STATE、GUI+TCC 责任链；contract 的 live suite 与 helper 认证是不同入口。按 ADR 0012 A0-M 另测 CLI→helper / App→engine 全链、物理热键/释放/重启/恢复、撤销/错误 PID/安全提示。不存在一条已交付命令可自动完成全部人工步骤；缺这一证据不能说 S2 关闭。macOS 27 结果不外推其他系统。 |
| P1b 原生剪贴板 S6 | 在匹配 Mac：`cargo build -p gcu-driver-macos --example text_smoke`，把生成的绝对路径设为 GCU_TEXT_SMOKE_BIN；随后 `cargo test -p gcu-driver-macos --test clipboard live_named_pasteboard_formats_and_change_count -- --ignored --exact --nocapture`；另选 `live_general_paste_consumed_before_fixture_clipboard_restores` 同参数 | named pasteboard 与 general paste 分别记录；general 用一次性 TextEdit/夹具剪贴板、用户同意后运行。独立观测消费完成/外来复制不被恢复覆盖/Stop；fake 测试和 named pasteboard 均不能证明 general 消费时序。 |
| P2 Windows S1/S2/S3/S5/S6/S9、G1/G2/G3 | `cargo test -p gcu-driver-windows --test identity --test safety --test clipboard --test stop`；`cargo test -p gcu-host-protocol owner_only`；`cargo test -p gcu-driver-windows --test hook_probe native_product_probe_chain_proves_both_hooks_without_cursor_change -- --ignored --exact --nocapture`；完整契约按 [test-windows-desktop.ps1](../scripts/test-windows-desktop.ps1) / [diagnostics.md](../crates/gcu-driver-windows/diagnostics.md) | Medium、非管理员、Default 交互桌面、物理键鼠；hook_probe 要设置 GCU_DESKTOP=1，清掉 GCU_HOOK_MOUSE_PROBE，用户在探针期间不操作。同位置移动可能 hover，坐标不变不是无副作用。UAC/安全桌面只验证拒绝，不点系统授权。逐路径 DACL/既有文件升级/管道与实际输入释放另取证；托管 windows-2022 没有指针栈的 skip 不算通过。 |
| P3 Linux X11 S1/S2/S5/S9、G1/G2/G4 | 身份回归 `cargo test -p gcu-driver-linux --test atspi_tree --test assembly_x11`；桌面契约 `cargo test -p gcu-driver-linux --test contract live_contract_table_suite_c01_to_c27 -- --ignored --exact --test-threads=1 --nocapture`；Stop `cargo test -p gcu-driver-linux --test stop live_xvfb_stop_hotkey_and_xtest_not_refreshing -- --ignored --exact --test-threads=1 --nocapture` | AT-SPI/dbus/openbox/XComposite/XI2、已启动原生夹具、GCU_FIXTURE_STATE 与独立产物目录，按 [CI](../../.github/workflows/computer-use.yml) 的 desktop 步骤准备。须核对实际运行测试数和测试内 SKIP/提前返回；成功退出不证明物理分支执行。Xvfb 是独立虚拟桌面证据；物理 XI2 与 XTEST 区分、XGrabKey 冲突/失效和 Stop 物理时限仍需真机。 |
| P4 Wayland S3/S5/S9、G2/G4 | `cargo test -p gcu-driver-linux --test portal_capture --test portal_input --test wayland_identity --test assembly_wayland`；匹配自托管 GNOME 环境 `bash scripts/wayland-ci.sh` | 前组多为 fake portal；后者需真实 mutter/portal/PipeWire 与首次同意，缺环境会 not_run。GNOME/KDE/wlroots 各自记录来源/映射/授权/24h上限/撤销/关闭/Stop失效；GNOME headless 不证明真实物理桌面全路径，未验收路线不开放。 |
| P5 证书/TCC S9、G6 | 在 `src-tauri/` 的真实 Mac：`cargo test -p grok-app requirement_syntax_is_accepted_by_real_codesign -- --nocapture`；`cargo test -p grok-app ad_hoc_codesign_verifies_and_a_tampered_copy_does_not -- --nocapture`；正式 helper 产物 `codesign --verify --deep --strict '/path/to/Grok Computer Use.app'` | 前两条只测 ad-hoc/语法，不需要真实证书也不验证 Developer ID 正向链。维护者提供签名产物后，按 codesign.rs 验实际引擎的 Team/Apple requirement、同名自签证书替换、外部 Dev 独立 TCC 与路径/构建改变重审；App 路线还须核对 App/engine 本身。由用户手动授权/撤销，无证书/用户则剩余 not_run，不操作钥匙串/账号绕过。 |
| P6 发行/SBOM S10、G7 | 干净 VM：`python3 scripts/install-smoke/run.py --archive /path/to/gcu-VERSION-TARGET.tar.gz --sums /path/to/SHA256SUMS`；按 [安装验收说明](../scripts/install-smoke/README.md) 执行 CycloneDX/校验和/精确 cosign identity、macOS `xcrun stapler validate '/path/to/Grok Computer Use.app'`、Windows `Get-AuthenticodeSignature -FilePath /path/to/gcu.exe` | 需要真实候选包/签名材料/对应 OS；Windows 使用 zip/py -3，macOS 验实际 helper .app。package-only 明确 not_run，未签名 NotSigned 不算通过；完整 release/SBOM/arm64/PPA APT 签名信任须对应 runner/维护者/外审。此包不触发发布，不登录真实渠道账号。 |
| P7 S8 / L6 真模型 | 当前复现 not_run：`cargo run -p gcu-eval -- --platform macos --model MODEL_ID --suite catalog --env desktop`；其他 OS 分别替换 platform；red_team 见 C8 | 这是缺口复现，当前 CLI 不调用模型 API、DesktopProvider 只有接口。总控须另交付获批的一次性桌面与真实模型/外部 oracle/副作用采集/销毁闭环，再提供真实执行命令和预算。仅改 --env/--model、隔离 GROK_HOME 或重用个人凭据均不能关闭该项。 |
| P8 L7/性能/长稳 | `cargo run -q -p xtask -- bench-check --reports /path/to/native-reports`；`cargo run -p gcu-eval -- --suite core --platform macos --soak-hours 4 --soak-report /path/to/soak.json --out /tmp/cu2-soak-review` | 只校验外部原生产物；先交付原生采样适配，再三平台固定 DPI/50 样本/4h 浸泡。无 --soak-report 时当前路径明确 not_run，不会启动四小时桌面。schema 见 [bench](../bench/report-v1.md)/[soak](../eval/reports/soak-v1.md)，不得生成假报告。 |

S4 的 fuzz 另需 Unix nightly runner，在 `computer-use/fuzz/` 串行执行 `cargo +nightly fuzz run script_rpc -- -max_total_time=60`（[fuzz manifest](../fuzz/Cargo.toml)）；它不是 Windows 验收，缺工具/runner 为 not_run。真实脚本桌面/soak 与 Windows Job 上限仍按 script-sandbox-review.md 的发布前要求，不扩大既有 Unix 签字。

### 前端、全仓静态检查

F1 在仓库根，有 Node/pnpm 依赖时执行：

```sh
corepack pnpm exec vitest run src/hooks/useComputerUseSensitive.test.tsx
corepack pnpm exec vitest run src/components/computer-use/
corepack pnpm exec tsc --noEmit
python3 docs/plans/computer-use-v2/reviews/check_plan.py
git diff --check
```

前端组件用例可验证默认拒绝/Esc/Enter/会话 crop 淘汰等，不能代替原生焦点、真实点选、权限或 OS Stop。App panic/retention/journal 在完整 `src-tauri/`、系统依赖齐备时分别跑 `cargo test -p grok-app panic_record`、`cargo test -p grok-app retention_`、`cargo test -p grok-app journal_cleanup`（[logging.rs](../../src-tauri/src/logging.rs)、[log_retention.rs](../../src-tauri/src/log_retention.rs)、[journal_cleanup.rs](../../src-tauri/src/journal_cleanup.rs)）。请核对实际运行测试数；cfg 跳过/零测试不能认作对应平台已通过。

## 已知局限与 not_run：安全原文

以下逐段**原样引用** security.md 的 S1–S10“缺口”列及 G1–G7。保留旧描述与 not_run，不将后续实现合并润色成通过。原文差异的后续证据已单列在前；下列引用不授予旧方案或旧参数新的效力。

### S1 拒绝类别：缺口原文

> G1 代码已按 03 §7.2 收紧（路径、内容哈希、cdhash、证书、发布者/Team、产品、build；缺证据不匹配；旧窄记录须重新授权），真机产品链仍 not_run，驱动单测不能代替该验收。类别识别尽力而为、非完备，不声称覆盖所有终端

### S2 审批与 Stop：缺口原文

> G2：本地热键断线的完整产品链仍 not_run。停止原因文件已有读写，见 D11；FileLatch 被替换时保持锁存，见 D10。同 UID 伪 TTY 不在 CLI 门槛可保证范围

### S3 gateway 与 Host：缺口原文

> G3/G4：Windows 图片 DACL 有 helper 读回单测（win-test），真机与崩溃遗留仍 not_run；持久清理见 D3–D6。浏览窗全 frame/原始 IPC、导航/子资源隔离须单独真实验收，不能用静态 capability 扫描签字

### S4 脚本沙箱：缺口原文

> Windows Job Object 未交付；`script_rpc` nightly fuzz、真实设备、脚本 soak 均 not_run，发布 script feature 前补齐既有审阅要求；不扩大当前 Unix 签字范围

### S5 脱敏与数据：缺口原文

> G3–G5：Windows DACL 有 helper 读回（win-test），真机 not_run。删除失败会警告并在下次启动重试。CLI keep 有 3600 秒戳，无常驻定时器。journal（D15）仍保存工具输入/输出，内容是否截断或脱敏 needs-decision，会话结束不删聊天。GCU_LOG_PAYLOADS 只在 log_policy 读取，默认 metadata-only，置 1 仍走字段/bearer 脱敏，不是自由文本密码清洗器；panic 与 ACP/connect 错误日志只记类别和位置；recent-calls 与 App 日志有 7 天/5×5MB 源码测试。真机与任意秘密全链 not_run

### S6 剪贴板：缺口原文

> 原生全格式恢复、消费时序与 Stop 需要平台结果；互斥只在同一进程内，不是跨 gcu 进程锁。恶意进程最后一刻替换不可消除，OS 历史/同步可能留文，无法由会话清理抹除

### S7 组合键黑名单：缺口原文

> 不能将纯 key 解析/策略测试当作三平台投递验证；敏感确认与硬拒绝的区别须保持，补 native/batch/script 的同等无副作用证据

### S8 提示注入：缺口原文

> 缺一次性桌面真实执行报告/外部副作用审计；不能把夹具 pass world 的零违规写成模型 red_team 零违规

### S9 平台权限与身份：缺口原文

> G1 持久授权匹配已在代码收紧（真机产品链 not_run）；G2：原生装配与 Stop 尚未闭环；G6：macOS 先要求 codesign --verify --strict 成功（bundle 目录另加 --deep）才采信 -dvvv，release 的 MacDeveloperId 再用单个参数 -R== 检查 anchor apple generic 与 certificate leaf[subject.OU]（Team 来自 ReleaseIdentity，非字母数字则拒绝）。失败、超时或无法执行清空 facts。正式 Developer ID 证书正向链、同名自签真证书与 Dev TCC 产品链 not_run。入口签名不证明 DLL/plugin 可信

### S10 供应链：缺口原文

> G7：需当前 lockfile 的 cargo-deny 输出与正式产物/SBOM/签名审计；SHA 固定不证明 action 代码可信，也不固定 CI 系统包/PPA 内容

### G1–G7：原文

> - **G1 — 持久身份约束。** 代码已按 03 §7.2 收紧：`AppIdentity::grant_eq` 要求路径、内容哈希、cdhash、证书、发布者/Team、产品、build；缺证据不匹配；只含窄字段的旧记录不升级、不放行（见 `computer-use/docs/adr/0015-persistent-grant-identity.md`）。路线 A `allows` 与路线 B `persistent_grant_matches` 调用同一函数。同 Team ID 的 macOS cdhash 变化、Windows 同发布者换路径/哈希、Linux 同路径换哈希都重新授权。驱动单测和本机逻辑测试仍不是真机产品链验收，该项真机 not_run。应用更新后「始终允许」需要重新批准。
> - **G2 — 原生装配和停止原因（部分关闭，2026-10-18）。** 普通执行入口已由 A0/A0-M/A1/A2/A3 按 ADR 0012 装配（`computer-use/crates/gcu-cli/src/assemble.rs`，默认 `auto`，失败关闭、不回退 fake）；引擎控制通道异常断开按 ADR 0012 Q2 记为全局持久 Stop，停止原因标记文件的 writer/reader 在 `computer-use/crates/gcu-host-protocol/src/stop_reason.rs`（目录 0700/文件 0600，缺失或伪造的标记不构成原因、也不能恢复或削弱 Stop），测试见该文件的 `missing_and_forged_markers_are_not_a_reason`、`marker_directory_and_file_are_owner_only` 与 `computer-use/crates/gcu-cli/src/engine_stop_tests.rs`。Windows Stop 守卫租约见 ADR 0013（hook_probe 真机仅一次通过，其余 not_run）。**仍未关闭**：FileLatch 与 policy 文件的路径防替换、权限失败处理需专门验证（并入 G3）；各平台真机 Stop 时限、helper/engine 完整产品链为 not_run。
> - **G3 — 权限。** 持久文件统一走 `computer-use/crates/gcu-host-protocol/src/owner_only.rs`。Unix 目录 0700、文件 0600，chmod/fchmod 错误即拒绝（build8：`computer-use/crates/gcu-host-protocol/src/owner_only.rs#directory_and_file_are_owner_only`，`computer-use/crates/gcu-host-protocol/src/owner_only.rs#chmod_failure_refuses_the_path`）。Windows 显式 DACL 仅当前用户 SID 与 SYSTEM，关闭继承；读回断言通过（win-test：`computer-use/crates/gcu-host-protocol/src/owner_only.rs#created_file_dacl_is_user_and_system_only`）。临时文件使用唯一随机名、create_new 与原子 rename（build8：`computer-use/crates/gcu-host-protocol/src/owner_only.rs#atomic_write_uses_a_unique_temp_and_leaves_no_fixed_name`）。符号链接与硬链接替换被拒绝（build8：`computer-use/crates/gcu-host-protocol/src/owner_only.rs#symlink_and_hardlink_replacement_are_refused`）。已接入 files、图库、approval/disk、FileLatch、policy 文件、日志与 panic.log、journal、Wayland restore token、stop-reason。CLI 状态目录、token 与管道仍用原 OW 检查，本卡不放宽。真机 ACL not_run。不能把 helper 单测写成每个产品路径都在 Windows 上读回过。
> - **G4 — 清理。** 图库删除失败会记一条 metadata 警告，并在下次启动重试已标记目录，不清空全部 session（build8：`computer-use/crates/gcu-host-protocol/src/events/gallery_cleanup.rs#purge_failure_warns_and_startup_sweep_retries`，`computer-use/crates/gcu-host-protocol/src/events/gallery_cleanup.rs#orphan_closed_directory_is_removed_on_sweep`）。MCP 在下次创建会话时回收无活锁的 gcu-session-*（build8：`computer-use/crates/gcu-host-protocol/src/owner_only.rs#orphan_session_directory_without_a_live_lock_is_removed`）；持锁不删；目录已创建但尚未加锁的窗口仍可能被扫到。CLI keep 副本写入 unix 秒 + 3600 的到期戳，无新配置、无常驻定时器；下次 CLI 清理按戳删除，部分删除失败会写下已到期戳再试（build8：`computer-use/crates/gcu-cli/src/call.rs#keep_lease_expires_on_the_stamp_even_if_mtime_is_fresh`，`computer-use/crates/gcu-cli/src/call.rs#keep_lease_delete_failure_warns_and_retries`）。用户重定向的 stdout 副本仍不受 CU 控制。journal 删除失败会警告，下次启动再试（mac-pro：`src-tauri/src/journal_cleanup.rs#journal_delete_failure_is_retried_on_the_next_sweep`，`src-tauri/src/journal_cleanup.rs#corrupt_index_does_not_delete_unmarked_sessions`）。前端 `src/hooks/useComputerUseSensitive.ts` 在会话结束、turn 结束或 Stop 时淘汰 cards/crop（`src/hooks/useComputerUseSensitive.test.tsx`，vitest 通过），不单靠 TTL；切聊天保留其他会话。journal 仍原样保存工具输入/输出：是否截断或脱敏落盘 needs-decision，本卡未改内容。真机清理 not_run。详见 D3–D6/D15。
> - **G5 — 载荷与日志。** `GCU_LOG_PAYLOADS` 的唯一读值在 `computer-use/crates/gcu-host-protocol/src/log_policy.rs`。未设置或非精确 `1` 时只记录种类与字节数，不记录输入文本、截图、树、错误原文或 token。置 `1` 时文本载荷仍先经 `inject::scrub`：已登记 bearer（不少于 16 字符）、`Bearer ` 加 64 hex、`GCU_TOKEN=`，以及 password/token/secret 等字段值改为 `[redacted]`。该开关不能关闭脱敏，也不是无字段名的自由文本密码猜测器。Capture、Image、树节点与原始帧的 Debug 不输出 bytes 或文本。App panic hook 不调用默认 hook，只记类别与位置，不写 panic 文本或回溯。ACP 非 JSON 与错误响应、connect 任务失败同样只记类别与位置。recent-calls 超过 7 天的文件会 unlink，过期条目会重写删除；IO 失败返回并打一条 metadata 警告。App `app.log`/`panic.log` 执行 7 天与 5×5MB（超限轮转删除），Unix 新建目录 0700、文件 0600，权限或删除失败至少警告一次。Windows 日志 owner-only DACL 仍属 G3，未实现。journal（D15）仍保存工具输入/输出，结束或撤销 CU 不删除聊天历史。订阅器不扫描全部 tracing 字段。真机、OS 崩溃转储与任意秘密全链 not_run。不能把字段脱敏测试写成所有秘密均已验证。
> - **G6 — 正式引擎验签。** `src-tauri/src/computer_use/codesign.rs` 在解析 `codesign -dvvv` 之前要求 `codesign --verify --strict` 成功；路径是目录（bundle）时另加 `--deep`，单文件 Mach-O 无嵌套代码，不加 `--deep`。子进程不经 shell，5 秒超时后杀掉，stdout/stderr 超过 64 KiB 的显示结果不采信。失败、超时或无法执行则 facts 清空。release 且身份为 `ReleaseIdentity::MacDeveloperId` 时，再用单个参数 `-R==…` 测试 `anchor apple generic and certificate leaf[subject.OU] = "<TEAM>"`（TEAM 来自该身份，不硬编码；含引号或非字母数字则直接拒绝）。不满足则拒绝。Developer 模式不套用这条 Apple 链要求，ad-hoc / identifier-only 的开发授权保持原语义；验证失败仍然不采信显示字段。单测覆盖有效吻合、验证失败/超时/命令缺失、要求不符（自签同名）、显示吻合但验证失败、dev identifier-only 仍放行。mac-pro（macOS 15.8.1 / x86_64）临时目录 ad-hoc 签名与篡改副本的 verify 成功/失败已测（`src-tauri/src/computer_use/tests/supervisor.rs#ad_hoc_codesign_verifies_and_a_tampered_copy_does_not`）。**not_run**：真实 Developer ID 证书的链正向匹配、同名自签证书的真证书替换、Dev TCC 产品链。host-protocol 的准入测试仍使用调用方提供的 facts。入口签名不证明 DLL/plugin 内容可信，保留驱动 trust_note 的披露。
> - **G7 — 供应链产物（部分处理，2026-10-18）。** ADR 0007/0009 记录具名依赖例外，deny.toml 另有 zbus/eframe informational advisory 忽略项；不得扩大或删除门禁。`cargo deny check` 在 CI 与本地均通过。**PipeWire PPA 版本已固定**：`.github/workflows/computer-use.yml` 与 `.github/workflows/release-computer-use.yml` 的 Linux jammy 依赖步骤把 `libpipewire-0.3-0`/`libpipewire-0.3-dev`/`libspa-0.2-dev` 固定为 `1.0.7-3~ubuntu22.04`（2026-10-18 的 CI 实际安装版本，PPA 的 amd64 与 arm64 索引当日都只有该版本）；PPA 删除该版本时安装步骤带明确错误失败，不再悄悄换版本，升级需人工复核后改版本号。desktop/AT-SPI 任务跑在 ubuntu-24.04（noble），用发行版自带的 libpipewire 1.0.5，不走 PPA。每平台 CycloneDX 依赖图在 release 工作流的 “Per-platform CycloneDX dependency graph” 步骤里已有，是否产出完整 SBOM、安装包签名与公证需要真实发行运行与维护者的签名材料，仍 not_run。PPA 是第三方来源，其包本身的完整性验证（APT 签名密钥的信任）仍应纳入外部审阅。

### 发布与关闭态限制：原文

> 07 §8 的发布门槛不变：未闭环不得发布；本清单不能把 partial 当作 A-α/A-β/A-1.0 通过。**L1 / Wayland / DOM** 未交付的产品能力保持关闭，开放前补其原生安全验收；可选能力不隐式阻塞只承诺已验证 L0/X11 的 A-1.0 范围，也不能隐式豁免已开放路径的安全项。ADR 0010 的批准是设计授权，不是浏览窗真实 IPC/网络验收；ADR 0011 保持默认附图与既有脱敏拒图；ADR 0012 的个别决策批准不表示组装完成。

## 问题反馈格式

请每个问题单独记录，引用 S/G 编号和候选 commit。测试失败保留首次输出；缺环境或提前返回用 not_run，不把“未确认”转换为通过。附件仅用夹具数据，凭据、token、真实桌面、原始聊天和私人目录不进入公开报告；发现真实秘密时沿仓库 [安全反馈流程](../../SECURITY.md) 处理。

```text
问题编号：
对应 S1–S10 / G1–G7 / ADR 条目：
候选 commit、包摘要、feature / 路线：
OS / 架构 / 桌面 / App / 模型版本：
环境前提与授权范围：
预期防线与实际行为：
最小夹具、复现步骤、完整命令：
退出码、实际测试数、首个失败 / SKIP / not_run 原因：
外部 oracle、独立副作用观测：
CI / artifact / 截图或录屏位置及摘要：
安全影响、影响范围与严重程度依据：
未验证条件 / 尚不确定项：
建议修复与复验条件（不得扩大权限或降低门槛）：
报告人、日期、可联系渠道：
```

没有既有自动命令的物理步骤，须报告手工操作序列、用户在场、独立观测与未覆盖分支，不能用“按说明测过”代替复现记录。总控可整理材料、修复和重跑；判定及外部签字仍由审阅者本人作出。

## 外部审阅者本人签字模板（空白）

**所有可填写字段保持空白；本模板没有签字或验收效力。签字人只能是外部审阅者本人，总控与 agent 不得代签、填入姓名/日期/通过状态或复制已有签字。** security.md 继承的总控 Unix 脚本限定签字不转换为外部签字。受审范围、未测项和处置理由必须由本人明确填写；局部签字不豁免其余 §7/§8 门槛。填写后的报告应附原始证据，由本人选择适当的签署载体，不自动回写 security.md 或 STATUS。

| 字段 | 由外部审阅者本人填写 |
|---|---|
| 审阅者姓名 / 身份 | |
| 机构 / 独立性及利益关系披露 | |
| 联系方式 | |
| 审阅起止日期 | |
| 源码 commit / 包版本与摘要 | |
| 路线 / feature / 开放与关闭能力范围 | |
| OS / 架构 / 桌面 / App / 模型版本 | |
| 运行环境与用户在场范围 | |
| 原始报告 / CI / artifact / 摘要索引 | |
| 适用发布档位与限制 | |

| 安全项 | 本人判断与限定范围 | 原始证据 / 复现入口 | 未测项 / 限制 / 处置 | 本人签名 / 日期 |
|---|---|---|---|---|
| S1 拒绝类别 | | | | |
| S2 审批与 Stop | | | | |
| S3 gateway 与 Host | | | | |
| S4 脚本沙箱 | | | | |
| S5 脱敏与数据 | | | | |
| S6 剪贴板 | | | | |
| S7 组合键黑名单 | | | | |
| S8 提示注入 | | | | |
| S9 平台权限与身份 | | | | |
| S10 供应链 | | | | |

| 缺口 | 本人复核的修复 / 证据 | 尚未关闭范围 / not_run 处置 | 本人判断 / 签名 / 日期 |
|---|---|---|---|
| G1 持久身份 | | | |
| G2 装配与停止原因 | | | |
| G3 权限 / 路径 | | | |
| G4 清理 | | | |
| G5 载荷日志 | | | |
| G6 正式引擎验签 | | | |
| G7 供应链产物 | | | |

| 最终字段 | 由外部审阅者本人填写 |
|---|---|
| 总体结论及其范围 | |
| 阻断问题 / 未确认与 not_run 清单 | |
| 后续复审条件 / 证据有效期 | |
| 外部审阅者本人最终签名 | |
| 签署日期 | |
