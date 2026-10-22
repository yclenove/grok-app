# 未签名预览发布通道提案

日期：2026-10-22；静态核对基线：`ef5cb876cbb26046c39742d2ba5b803f1a0c9554`。
状态：**提议，待维护者裁定；本次仅交付文档，不新增发布资格或开启原生路线。**

## 一页分析：现状、平台后果与签名途径

**可以制作不带平台发布者签名的包；当前不能据此认定可以公开发布可用的 Computer Use。** 必须分开看打包政策、§8 质量档位、运行时身份准入和实际候选证据。没有可供任意项目共用、同时获得 macOS/Windows 平台信任的“公共签名私钥”；开源签名服务需要项目审核，Sigstore 证明构建来源也不等于平台代码签名。

| 现有依据 | 要求 / 允许 / 禁止 |
|---|---|
| [路线 A §6](../04-route-a-standalone-mcp.md#6-签名公证与分发)、[packages README](../../../../computer-use/packages/README.md) | 明确允许缺 Apple 证书时保留 ad-hoc helper、缺 Windows 证书时产出未签名 exe，并披露限制。这是产物政策，不是安全验收。 |
| [§8](../07-quality-gates.md#8-发布门槛)、[release.rs](../../../../computer-use/crates/gcu-eval/src/release.rs)、[准备度表](../../../../computer-use/docs/release-readiness.md) | A-α/A-β 没有单列平台签名公证要求，但所有适用安全规则继续有效；正式档位累计 α/β、全部质量/安全项及 `signed_and_notarized=true`。缺签名的完整质量输入目前最高判为 beta，channel 仍 experimental。 |
| [ADR 0006 决定 3](../../../../computer-use/docs/adr/0006-macos-tcc-helper.md)、[ADR 0012 Q5](../../../../computer-use/docs/adr/0012-real-driver-assembly.md) | 发布用 Developer ID + Team ID；已有未签名产物政策不能自动豁免正式引擎身份/TCC 要求。§7 S9 的身份/权限核验与 S10 的供应链签名不能被删掉或自动签字。 |
| [发行工作流](../../../../.github/workflows/release-computer-use.yml)、[macOS 脚本](../../../../computer-use/packages/macos-sign.sh)、[Windows 脚本](../../../../computer-use/packages/windows-sign.ps1) | 缺证书可跳过平台签名；配置签名后的错误必须中止，不能退回未签名。manual dispatch 仅产出 artifact；tag 发布须过三平台 perf-gate，预发布后缀也不豁免。tag 发布保留 SHA256SUMS 的 cosign 签名。工作流没有调用 gcu-eval 的完整 §8 判定，能走到 publish 不能证明全部门禁通过。 |
| [§8/§9](../07-quality-gates.md#8-发布门槛)、准备度表 | 禁止用 partial、skip、not_run、fake 或实验标签替代原生证据；App 实验可见也需 A-α。新增名称不能让不合格候选变为可发布。 |

**macOS：** 浏览器等下载链可能加上 `com.apple.quarantine`，解压后 Gatekeeper 会评估应用；无 Developer ID/公证的应用可能被拦截。具体提示/可否由用户手动放行受系统版本和管理策略影响，不能承诺一定可打开。不要用自动删除隔离属性、关闭 Gatekeeper/SIP、修改 TCC 数据库来完成安装。Apple Silicon 上不能把“未签名预览”理解为去掉所有代码签名：仓库 helper 保留 ad-hoc seal，它不证明发布者身份、不等于公证，也不会消除 Gatekeeper 问题。

普通 ad-hoc 重建会改变 cdhash，可能使 TCC 视为新身份，辅助功能和屏幕录制需由用户分别重新授权，可能还需退出并重开。**仓库有必须披露的例外：** [bundle.rs](../../../../computer-use/xtask/src/bundle.rs) 使用 identifier-only designated requirement；ADR 0006 在 macOS 27 arm64 上测到它重建后仍保留 TCC 授权。因此不能声称本项目“每次重建一定掉权限”，也不能宣传权限会安全稳定继承：同标识符 ad-hoc 替身可能继承 TCC，这正是发布要求 Team ID 的原因，其他系统未验证。每次更新都应检查权限和身份，身份失配必须重新授权；OS TCC 与 App 的持久授权是两套检查。

**Windows：** 下载文件可能带来源标记；SmartScreen 可能显示未知发布者/信誉不足，组织策略或其他系统保护可能直接阻止。Authenticode 也不保证没有信誉提示，EV 也不应承诺免提示。未签名不会赋予管理员/UIAccess 权限；不能以关闭 SmartScreen、提权或放宽桌面/Stop 守卫解决安装或输入失败。

确知存在的公开途径如下；本次离线核对，链接供维护者后续确认，**不声称核验了 2026-10-22 的价格、地区、资格或项目获批情况**。

| 途径 | 能提供什么 / 限制 |
|---|---|
| [Apple Developer ID](https://developer.apple.com/developer-id/) + [notary service](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution) | 通过 Apple Developer Program 的合资格个人或组织身份取得 Developer ID，签名后提交公证。普通免费 Apple ID/本地开发签名不等于 Developer ID 分发；没有已知通用免费公共证书可替代。费用/减免资格本次未核实。 |
| 受信任 CA 的 Windows Authenticode 代码签名证书 | 商业 CA 路径存在；当前脚本接收证书材料并用 signtool。个人项目能否申请、身份审核、密钥保管/硬件要求、费用由提供方确定，本次不作保证；自签证书没有默认公众信任。 |
| [Microsoft Artifact Signing（原 Trusted Signing）](https://learn.microsoft.com/en-us/azure/artifact-signing/overview) | Microsoft 托管代码签名服务；是否接受该维护者的个人身份/地区及当前条件未核实，不能作为已可用方案。现有 PFX 脚本不能因此假定已接入该服务。 |
| [SignPath Foundation](https://signpath.org/) | 为符合其条件且获审核的开源项目提供免费代码签名服务，是 Windows 路径的候选；不是任意项目直接领取的共享证书，也不是 Apple Developer ID/公证替代。项目资格、平台覆盖和接入方式需维护者核实。 |
| [Sigstore](https://docs.sigstore.dev/) / cosign | 现有 tag 流程使用 GitHub OIDC/Fulcio/Rekor 对校验和清单签名，可保留供应链来源证据；不能替代 Developer ID、公证、Authenticode、SmartScreen 信誉或 TCC 身份。离线 manual artifact 当前没有该 tag 签名。 |

## 最小目标方案：unsigned_preview

“未签名”只指缺少完整的平台发布签名/公证保证，**不表示没有供应链签名，也不要求删除 macOS ad-hoc seal**。应逐平台披露实际状态（macOS ad-hoc/Developer ID、有无公证；Windows 有无 Authenticode），不能从 `signed_and_notarized=false` 推断所有二进制均无签名。该 bool 还可能代表未知或只完成部分签名，不能单独触发预览准入。

保守定义：`unsigned_preview` 满足现有正式档位的**全部安全和质量门槛**，仅不取得正式分发签名/公证资格；仍是主动选择的预览，App 默认关闭。它不会缩减 α/β 的条件，不新增 Host 模式或权限。候选缺任一必需证据继续不合格；先解决下节的身份适用性问题，不能通过把 S9/S10 填 true 来制造一条捷径。

| 必须满足的行 | unsigned_preview 的要求 |
|---|---|
| A-α 全部 | macOS L1–L4 pass；L6 native ≥7000 bp；S1–S6 有效审阅；首次截图全链脱敏、不可靠拒图、IDE/未知执行与 Stop 锁存回归全部通过。 |
| A-β 全部 | 三平台 L1–L5 pass；三平台 native、text 各 ≥7000 bp；真实 red_team 零违规。 |
| A-1.0 的质量部分 | 三平台 L1–L7 pass；overall macOS ≥8500 bp、Windows/Linux X11 ≥7500 bp；原生性能预算、四小时长稳及 ≥30 已验证 App 兼容性，不能拿目录行数或 fixture 充数。 |
| §7 S1–S10 | 全部有效签字；包括 S9 的实际候选身份、TCC、撤销/错 PID/独立 Dev 授权等适用测试；S10 的 deny、固定 Actions、SBOM 和供应链来源签名。关闭的功能必须审阅不可达边界，不能自动免签。 |
| 分发与安装 | 相同候选/feature 的完整 CI/artifact；干净机安装→接入→授权→夹具动作→Stop/用户恢复→撤销→卸载；最终字节的 SHA256SUMS、SBOM、可验证来源签名，保留现行 perf-gate。 |
| 仅允许欠缺的正式项 | 平台发布者签名/公证；明确记录实际欠缺项，`signed_and_notarized=false`。**不得欠缺运行时身份防冒充、授权、Stop 或供应链签名。** 若现有 S9 身份要求只能靠 Developer ID 满足，该 macOS 候选仍不能进入此通道。 |

这是严格目标，不是 macOS-only 捷径；现有判定器固定累计三平台条件，不能在同一草案中把未测平台改为不适用。若只分发单平台原生预览，应另行定义并批准平台范围的证据模型，不能用省略平台来令门禁变绿。

| 现有名称 / 目标名称 | 关系与输出 |
|---|---|
| experimental | 可能低于 A-α；不是发布/可见性许可，缺报告也落在此处。 |
| alpha / beta | 现有质量层级不变，App channel 目前仍为 experimental；不是签名状态。 |
| NATIVE_PREVIEW / `NATIVE_PREVIEW_BP` | 当前代码没有名为 NATIVE_PREVIEW 的枚举档位；Rust/TypeScript 的 `NATIVE_PREVIEW_BP=7000` 是 α/β 的 native/text 成功率门槛，不能拿它代表“未签名可发”。 |
| unsigned_preview（拟新增 tier/channel） | 全部上述门禁 + 显式预览选择 + 已审阅的逐产物签名/身份事实；`promote=false`、`default_formal=false`，保留“正式签名公证缺失”的原因。 |
| formal | 仍累计 α/β、全部正式门槛和完整签名公证；仅此档位允许 `promote=true`、`default_formal=true`。 |

不得宣称“1.0 正式”“已签名公证”“系统验证的发布者”“免 Gatekeeper/SmartScreen”“权限可安全继承”“三平台已支持”或“Stop 风险已消除”。已有 [Windows 2026-10-22 诊断](../../../../computer-use/crates/gcu-driver-windows/diagnostics.md) 的 20 pass / 9 skip / 1 not_run / 0 fail、400 次拖拽无 Stop 锁存，只是所列测试的历史证据；回调偶发缺失原因仍未知，不能替代其他 §8 行或新候选验收。本次不重测、不改守卫常量。

面向用户的候选文案（仅文案设计，未写入 UI；落地 UI 时必须走 15 语言 i18n）：

> **未签名预览 · 非正式版本**。本预览未完成平台签名或公证，系统可能阻止打开。请核对下载来源、版本和校验信息，并查看已验证的平台与限制。
>
> **macOS：** 本包保留 ad-hoc 签名，未获 Developer ID 签名或 Apple 公证。首次安装及每次更新后，请检查实际授权对象；重新构建或身份变化可能要求分别重新授予辅助功能和屏幕录制权限，并退出后重开。路线 A 授权 Grok Computer Use helper，路线 B 授权 Grok App；不要把权限授给终端来绕过检查。系统显示已授权也不能代替应用内的身份重验。
>
> **Windows：** 本包没有 Authenticode 发布者签名，SmartScreen 或组织策略可能阻止运行。请先核实来源与校验信息；此预览不要求关闭系统保护或以管理员身份运行。

文案必须按实际产物选择：若 macOS 已有 Developer ID 但未公证，改为如实说明“已签名，未公证”，不得套用 ad-hoc 文案。安装帮助放 README/包说明；Release body 继续来自 CHANGELOG/既有生成脚本，不手改 GitHub notes，也不把去隔离命令塞进版本变更说明。

## 为何本次不改 release.rs：需要裁定的边界

1. **运行时身份与 TCC 是安全要求。** ADR 0006 决定 3 要求发布用 Developer ID + Team ID；ADR 0012 Q5 明文说“已有未签名产物政策不能自动豁免正式引擎身份/TCC 要求”。[Host policy](../../../../computer-use/crates/gcu-host-protocol/src/supervisor/policy.rs) 的 `MacDeveloperId` 路径在 Release 模式拒绝 identifier-only；[App supervisor](../../../../src-tauri/src/computer_use/supervisor.rs) 也存在无 team 时使用精确 `ContentSha256` 的路径。后者只能证明 App 所启动引擎的字节匹配，不能据此宣称解决了 OS 对 identifier-only helper 的 TCC 身份继承。不得把 HostBuild 改成 Dev、移除 Apple requirement、放宽 helper 认证或换成同 UID 检查来落地预览。若要公开开放这条 ad-hoc 原生路径，需要先裁定并审阅身份方案，可能涉及 ADR/打包/运行时，已超出文档和 eval 的范围。
2. **安全签字不等于分发签名位。** `security_signed` 是 S1–S10 人工审阅位；`signed_and_notarized` 是粗粒度分发事实。简单删除 `formal_extra` 中的 signed 条件，或把 `!signed_and_notarized` 自动映射为可发布，既不能确定逐产物状态，也不能证明 S9/S10 已成立。S10 的 cosign 来源签名可以保留，不应因“未签名”而删除；是否充分须由实际安全审阅确认。
3. **App 不读取 Rust 判定来自动改变档位。** [TypeScript 判定器](../../../../src/lib/computer-use/releaseGate.ts) 独立镜像门槛，[设置 UI](../../../../src/components/settings/ComputerUseSection.tsx) 只展示 formal/experimental，`shippedReleaseEvidence()` 返回 null。只加 Rust 枚举会造成两端语义分歧，且不会提供正确标签/权限提示；完整用户通道还需前端测试与 i18n，超出本次允许的代码范围。不能将报告枚举草案当作已完成的发布通道。

后续获批后最小代码顺序：先写失败测试；将 `formal_extra` 拆成**不改变阈值的质量判定**与**分发签名判定**；保留 alpha/beta 累计和完整十项安全签字；新增 `ReleaseTier::UnsignedPreview` / `ReleaseChannel::UnsignedPreview` 及 `as_str`/serde 拼写；仅在显式预览、全部质量/安全通过、签名欠缺事实明确且身份方案已获认可时返回该档位，formal 优先且条件不变。未知状态保持关闭，缺签名的当前 beta 输入不能不经审阅自动获得发布资格。同步 eval README、§8/准备度表，App 展示另行获批后同步 TypeScript、组件与全部语言；工作流仍由所有者管理。

建议先写的测试及断言（本次未添加不具备实施前提的测试桩）：

- `release_unsigned_preview_all_quality_and_identity_gates_met_stays_nonformal`：新 tier/channel 的序列化与报告落盘；两个 formal 标志均 false，保留缺签名原因。
- `release_unsigned_preview_any_required_gate_missing_is_refused`：逐平台逐层枚举 fail/partial/not_run/缺失；逐安全位 false/缺失；截图、拒图、IDE/Stop、red_team、预算、兼容数、70%/85%/75% 边界，各自不能取得新档位。
- `release_unsigned_preview_unknown_signing_or_unapproved_identity_is_refused`：仅一个 false bool、不明确的逐平台状态、未裁定身份均不得准入；不能用 S9/S10 空白或旧签字填 true。
- `release_unsigned_preview_bench_missing_or_invalid_report_stays_closed`：bench 即使数值完美也不准入；缺报告、未知字段、过长 JSON、无效比例继续 fail-closed。
- `release_formal_signed_evidence_and_legacy_preview_behavior_are_preserved`：完整正式输入仍唯一 promote/default_formal；现有 alpha/beta、无预览选择的旧输入行为不变。App 阶段另补 Rust/TypeScript 共享样例与标签/i18n 回归。

## 验证与交付

- `git diff --check`：通过；新增文件另用 `git diff --no-index --check /dev/null <提案路径>` 检查，无空白错误诊断（退出 1 表示新增内容存在差异）。
- 本文相对链接存在性、工作区改动范围检查：通过（离线静态检查，不验证外链或签名服务资格）。
- `rcargo test --locked -p gcu-eval --test runner release`：**not_run**，未改 Rust；留给获批实现后的首次失败测试和回归。
- `rcargo test --locked -p gcu-eval`、`rcargo clippy --locked -p gcu-eval --all-targets -- -D warnings`、`cargo fmt --all -- --check`（computer-use 内）：**not_run**，纯文档不编译。
- 原生 Gatekeeper/TCC/SmartScreen、干净机安装、真实模型/红队/性能/长稳、平台签名公证与 cosign 发布验证：**not_run**；没有联网、下载、操作证书/账号或真实桌面。
- 仅新增本提案；未改 `.github/`、签名/密钥脚本、Stop/驱动、STATUS、任何门禁或现有准备度结论；未发布、推送、开 PR 或切换/合并分支。
- `git add -- <提案路径>`：被沙箱的只读 Git 索引阻止，未暂存或提交；保留单文件变更供总控审阅提交。

**NEEDS-DECISION:** ① 推荐先保留“未签名构建产物”供审阅，原生能力按现有门禁关闭，待预览身份方案完成 ADR/安全审阅后另派完整通道实现；② 若目标是立即面向外部测试者开放 macOS 原生，先批准独立的预览身份/TCC 设计任务（不能以风险提示替代安全门禁，结论可能仍要求 Developer ID）；③ 维护者提供正式签名材料，继续原有正式发布路径。三者都须补齐候选证据；本提案不代替该决定，也不表示当前已达到任何可发布档位。
