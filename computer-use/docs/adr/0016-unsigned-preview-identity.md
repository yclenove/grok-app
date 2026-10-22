# 0016 未签名预览的运行时身份方案

状态：**Accepted（方案 A；2026-10-23 项目所有者“按建议来”）**。接受仅表示采用 A 的设计方向；**不授权发布，不豁免任何门禁**：S9/S10 与 07 §7/§8 保持原要求，M1–M9 真机矩阵全部 not_run，实施须按本文清单分卡并另经评审，未满足前原生输入保持关闭。

日期：2026-10-22

范围：先讨论 macOS 路线 B（App → 随包 engine）的 `unsigned_preview`；不扩展已验收的平台范围。

## 给项目所有者的一页摘要

**一句话结论：没有 Developer ID 也能设计 Computer Use 预览，但精确哈希只能证明“与这个 Host 固定的引擎一致”，不能证明“来自经 Apple 验证的发布者”。**

**推荐 A：明确的 `unsigned_preview` 身份档位，最终引擎字节的 SHA-256 固定在随包 Host 中，每版重新批准，关闭「始终允许」。** 使用预览专用、按构建区分的 TCC 责任 App 身份；不使用 identifier-only 指定要求来维持跨版系统授权。先限定路线 B、已有准入的 macOS 27 arm64；CLI helper 和其他未验收路线继续关闭。实施前须批准本 ADR 并完成下文真机矩阵。

| 所有者得到什么 | 必须接受的代价 |
|---|---|
| 不购买 Apple 证书也有一条可评审的原生输入预览路线 | 没有 Developer ID 发布者证明、公证或正式版资格；初次安装来源仍需用户信任 |
| 正常 Host 可拒绝被单独替换、损坏或混装的 sidecar | 同用户恶意程序若能改 Host 和其内嵌哈希，也能绕过检查；不能声称防住本机恶意软件 |
| 每个候选构建和用户批准绑定，不静默继承旧许可 | 每版重新启用、重新批准目标应用；按构建的 TCC 身份可能增加系统设置中的授权条目和清理负担 |
| 保留 Stop、会话隔离、敏感确认等既有约束 | 禁止引擎独立后台更新、外部引擎和持久授权；更新必须成对交付 Host/engine，并重新验收 |

备选 B 仅供所有者本机开发自用；备选 C 维持 Developer ID 要求。A 的工作量高于“允许 ad-hoc”一个开关，主要在最终产物固定、启动竞态、授权换代和真实 TCC 验证。若 A 的前提无法满足，则保持原生输入关闭，选择 B 或 C；不能把缺失证据写为通过。

## 背景与证据边界

派工指定的前置提案 `docs/plans/computer-use-v2/reviews/2026-10-22-unsigned-preview-proposal.md` 在本检出及本地 `origin/feat/cu-v2` 中缺失。本稿仅采用派工给出的结论和 `unsigned_preview` 名称；未声称读过全文，也不复述其主 App 分发分析。总控取得该文后，须核对两份提案的档位定义和门禁映射，消除冲突后再裁定。

- [ADR 0006 决定 3](0006-macos-tcc-helper.md) 与 [ADR 0012 Q5](0012-real-driver-assembly.md) 的正式引擎要求是本项目的安全政策；Apple 不要求每个桌面 App 都具备 Developer ID。这里的“未签名”指没有受信发布者代码签名，macOS arm64 仍可能需要 ad-hoc 签名才能加载。上游能分发主 App 不自动决定原生输入的信任边界；安装提示见 [BUILD](../../../docs/BUILD.md)。
- [现有 Host](../../../src-tauri/src/computer_use/supervisor.rs) 的 `expected_identity()` 在 Team/signing id/cdhash 不齐时选择 `ContentSha256`；[build.rs](../../../src-tauri/build.rs) 将暂存 sidecar 的摘要编入 `GCU_ENGINE_SHA256`。这是一条内容一致性机制，不是新的发布授权。`MacDeveloperId` 的 identifier-only 拒绝与 `ContentSha256` 是不同分支，不能混称已有同等身份保证。
- [codesign 适配](../../../src-tauri/src/computer_use/codesign.rs) 先验证再采信签名显示字段；但 Content 身份在签名探针失败时仍保留文件哈希，已有 `content_identity_keeps_its_hash_when_codesign_is_unavailable` 测试。A 要求的 macOS 签名有效性检查不能假定已由此分支保证。
- 当前 `bundled_engine()` 接受 `GCU_ENGINE_PATH`；[Control::start](../../crates/gcu-host-protocol/src/supervisor/task.rs) 用保存的 `Admission.facts` 再次判定，不重新读取文件。已有路径、摘要和重启逻辑不能直接当作“每次实际执行的映像均已固定”的证据。以下对它们提出要求，不在本稿修复。
- [ADR 0006 (d)/(e)](0006-macos-tcc-helper.md) 仅在 Mac mini、macOS 27.0.1、arm64、SIP 开的探针中证明：子进程可继承责任 App 的 TCC；默认临时签名重建后失去授权；identifier-only 指定要求重建后仍保留授权。**同标识恶意副本的正向继承当时未成功构造**，不能把风险推断写成该攻击已实测成功。

若获批，本 ADR 只为明确的 `unsigned_preview` 增加例外，局部修订 ADR 0006 决定 3、ADR 0012 Q5 对该档位的解释。两者对正式签名档位的要求仍适用；其余 TCC、输入和停止决定不在本次变更范围内。

## 威胁模型：Host 究竟信任什么

保护对象是获得 TCC 的责任 App、它启动的实际引擎实例、独立控制连接，以及授给本地会话的观察/输入能力。模型、网页、名称、图标、引擎自报版本或摘要均不是身份来源。前提是用户安装的 Host 和其中固定值来自用户愿意信任的候选包，且 Host、OS 和构建链尚未被攻击者控制；该前提不能靠包内自己的哈希证明。

| 对手或故障 | A 能提供的边界 / 不能提供的保证 |
|---|---|
| 模型、网页或无匹配进程证明的本地连接者请求输入 | 仍须经过现有 Host 认证、目标授权、策略和 Stop；不能通过报出一个正确哈希取得权限 |
| 只替换引擎、混入另一版 sidecar、传入外部路径 | 正常可信 Host 应拒绝；前提是每次实际启动的文件/映像均重验，不能仅检查缓存 facts |
| 同用户进程同时替换引擎和期望哈希 | 若可重写未签名 Host、污染其编译输入，或修改作为期望值的外置文件，则可共同替换并绕过内容校验；内嵌值不构成独立安全根。替身是否另获 TCC 取决于 OS/指定要求，不能把绕过哈希直接写成已获得系统权限 |
| 同名 ad-hoc 替身试图继承系统权限 | identifier-only 要求可能允许继承；Host 的哈希门禁不能约束绕开 Host、直接访问 TCC 的替身进程。A 禁用这种跨版匹配方式，仍须真机验证实际隔离 |
| 已控制 OS、可信 UI/进程内存、构建/发布渠道，或注入可加载代码 | 不保证抵抗；精确主程序摘要也不覆盖未固定的 dylib、插件和可写加载路径 |

### 四种身份材料的区别

| 材料 | 能证明什么 | 来源、变化及风险 |
|---|---|---|
| 精确内容 SHA-256 | 在可信期望值、正确实现和竞态控制的前提下，候选文件与指定产物字节相同 | 维护者固定最终产物摘要；签名、重建、lipo 等改变字节就必须换值。若启动时对候选文件现算一个“期望值”，或从候选旁边可被同一攻击者改写的 SHA256SUMS 读取，只证明自洽 |
| ad-hoc cdhash | 经有效性核验的 CodeDirectory 身份，可作运行中映像与固定构建的辅助绑定 | 没有受信发布者证书；任何人均可用相同 identifier 自签其他代码，其 cdhash 通常不同。精确固定 cdhash 能区分这些构建，不能只验“有 ad-hoc 签名”。多架构切片需分别核对；cdhash 不等于整个文件 SHA-256 |
| identifier-only 指定要求 | 当前代码满足某个签名标识字符串 | 字符串不是秘密，不证明维护者、Team 或构建；不把它作为预览引擎准入凭据，也不将其作为责任 App 的跨版 TCC 稳定方案 |
| TCC 已授权 / preflight=true | OS 认为当前责任进程链拥有某类权限 | 不证明引擎是本项目产物，也不代表用户批准本版会话。重建后保留授权既方便开发，也扩大同标识替身的风险面；ADR 0006 的单机观察不能泛化到所有 OS/签名形态 |

## 推荐决定 A（待批准）

### 固定值与启动链

1. **谁固定、固定在哪里：**维护者按候选 commit、平台、架构和 feature 集审阅构建结果。对所有字节变换完成后的最终 engine 取 SHA-256，编入同版 Host 的只读构建元数据；连同渠道、构建标识、规范化包内相对路径及协议/策略版本形成发布绑定。摘要不是秘密。发布清单可供人工核对，但不能替代 Host 内嵌期望值或单独证明渠道真实性。
2. **固定的顺序：**先完成引擎的 ad-hoc 签名和其他打包变换，再固定摘要、构建 Host，最后解包实际发行产物核对。若打包器此后又改了引擎，候选必须失败并重做构建绑定，不能在用户机器上把新文件自动“学习”为可信。现有 build.rs 的暂存时机是否等于最终产物时机须另行核验。
3. **谁能改哈希：**运行时不从环境、用户配置、网络或 sidecar 旁的清单覆盖期望值。攻击者若能同时改 Host/内嵌值，则本机制失效；即使 SHA256SUMS 和包来自同一被攻陷渠道也无法辨别伪造发布。Tauri updater 的 minisign 只在其独立可信公钥和校验链成立时认证更新包，不取代 TCC 身份或本机映像检查。
4. **每次启动重验：**以 release 的拒绝外部引擎规则运行，另加显式预览身份策略，不能借 `HostBuild::Dev` 放宽准入。限制为真实安装 bundle 内固定路径，拒绝环境路径覆盖、逃逸的符号链接、未知加载来源和版本混装；在首次启动及每次重拉引擎时重新读取/验证最终 SHA-256 和 macOS 代码有效性。缺摘要、缺必要 OS 证据、签名损坏或验证失败均拒绝，不回退 identifier-only 或自报事实。
5. **绑定实际实例：**保留继承私有连接、独立 Stop、双向挑战及 OS 进程实例绑定（[03 §15](../../../docs/plans/computer-use-v2/03-architecture.md)）。还须把“已检查文件”与“实际拉起并受核验的映像”绑定，覆盖读取期间、校验后到 exec、重启期间的替换竞态；证明前不激活输入。PID/启动时间和连接秘密只能证明实例/通道，单独不能证明代码字节。主程序哈希不覆盖依赖加载，需审计并约束可写 dylib/插件来源。具体 OS 实现须另审，不能把再次 canonicalize 当作已消除 TOCTOU。

A 的承诺是“可信 Host 固定的候选引擎 + 已验证实例”，不是对任意同用户攻击者的代码执行隔离。若上述文件/实例绑定无法实现或证实，原生输入保持关闭。

### 更新、TCC 和重新授权

- **每版换固定值：**仅允许用户明确安装一对新的 Host/engine；新版本、重新构建、摘要/渠道/策略变化及回滚，都先关闭投递、撤销旧 token/会话和待审批，再要求用户批准。相同引擎字节也不能仅因版本显示未变而继承旧 Host 的授权。禁止后台自更新到未固定哈希、引擎独立热替换或首次运行 TOFU 固定；完整包的有效更新签名也不能跳过本版 CU 批准。
- **应用授权更严：**首次进入该模式必须重新明确启用和批准，旧正式/Dev/fake/其他预览版本的授权不迁移。首版 A 关闭「始终允许」，既不读入既有 Always 放行，也不写入新 Always；本轮/本会话授权仍绑定本版 Host/引擎、目标映像与会话。换引擎/更新/回滚使这些批准失效。[ADR 0015](0015-persistent-grant-identity.md) 约束的是目标应用身份，不能单独替代 Host/引擎换版隔离。以后若要持久授权，须另审绑定全部构建证据的方案，不跨版本继承。
- **系统授权另算：**采用预览专用、按构建区分的 **TCC 责任 App** 标识，并避免 identifier-only 的自定义指定要求，使新构建走新的系统授权对象；仅换 sidecar identifier 没用。首次预览及每版由用户本人完成所需辅助功能、屏幕录制、输入监控授权，不自动点击、编辑 TCC 数据库或清除系统限制。旧条目由用户在系统设置中撤销；OS 没弹窗不代表本版批准。若实际 OS 仍复用旧权限、不能确认责任对象或不允许可靠隔离，保持关闭并重新审案，不能以 App 内点一次同意冒充系统重授。
- **不能保证权限已被擦除：**App 内撤销/停止不等于 OS 删除旧 TCC，也无法强迫一个已被替换的 Host 遵守策略。用户若曾授权 identifier-only 的旧包，应获知须手动撤销旧系统权限；换新身份只能约束新设计，不能追溯保护旧安装。
- **安全状态不能随身份清零：**按构建换 App 标识不等于全新安全状态。同渠道版本间必须保留持久 Stop，切版先结束旧引擎，不能因新数据目录、并装旧版或卸载重装而默认恢复输入；无法安全继承停止状态时保持停止。TCC 隔离与授权重批不能成为重置 Stop 的旁路。

### 必须关闭或保留的限制

| 能力 | `unsigned_preview` 约束 |
|---|---|
| 持久授权、自动恢复和后台升级后自动执行 | Always 关闭；升级、崩溃/断线、重启均不能解除持久 Stop；恢复仍只有可信用户入口 |
| 外部引擎、路径覆盖、Dev 准入宽松规则 | 关闭；缺 Developer ID 不自动降级为预览，档位须由受审构建元数据明确选择 |
| SSH、Remote IM、定时任务、official-aux 的输入能力 | 沿用禁止注入；不因预览身份扩大到无人值守任务 |
| CLI helper、未验收系统/架构、L1/L2 和其他未交付路线 | 本 ADR 不开启；路线 A 若要采用该身份方案，须另补 helper 自身固定值、audit-token 对端和 TCC 责任链设计/证据 |
| 观察/动作、敏感确认、前台输入、Stop | 观察也须授权；拒绝名单、敏感确认、前台 Ask/活动守卫、脱敏、失败不升级及 unknown 不重放照常适用 |

Stop 的 100ms 心跳、4750ms `DRIVER_DEADLINE`、原生调用 5s 上限及 fail-closed 语义不是本提案的可调项。

### 用户可见的披露草案

首次启用与每版重新批准时，应同时呈现档位、构建版本、引擎 SHA-256、实际 TCC 责任 App，以及可取消的授权入口；设置/诊断可查完整摘要。匹配状态只能写“与本安装包固定记录一致”，不能显示“Apple 认证”“可信发布者”或把 ad-hoc 写成 Developer ID。校验失败要明确拒绝并指引重新安装匹配的完整包，不能提供“忽略继续”。

> 此 Computer Use 预览没有 Developer ID 发布者认证。引擎与本安装包固定的哈希一致；能同时替换 App 和校验记录的本机程序仍可绕过检查。

> 本版需要你重新启用并批准 Computer Use，且不提供「始终允许」。系统权限与 App 内批准相互独立；旧系统授权可能仍存在，请在系统设置中撤销不再使用的预览版本。

以上是文档文案草案。本次不加入 UI 字符串；实现时以 `computer-use/ui/locales/en.json` 为 authority，同步 15 语言、导出 App catalog，使用既有 App 对话框和设置登记规则。

## 与 S9 / S10 和发布档位的关系

下表的“保留”表示要求不放宽，**不表示本次验证通过**；既有 `partial/not_run` 不因提出 A 而改变。

| 原门禁条目 | A 的关系与必须修订处 |
|---|---|
| S9：私有 API 探测、关闭、失败不升级 | 保留；内容身份不授予 SPI 能力，不把身份检查失败变成另一条投递路线 |
| S9：TCC 撤销、错误 PID、安全提示实测 | 保留并增加按版身份、重建、替身、权限继承和 App → engine 产品链用例；仍需真机，不能以哈希单测替代 |
| S9：正式引擎签名/构建核验 | 精确构建匹配部分可由 A 的最终产物固定与实例绑定承担；Developer ID/Team 发布者证明部分 **不满足旧要求**。须明文增加仅预览的内容身份分支；正式分支不变，不能给旧 S9 整项填通过 |
| S9：外部引擎独立 Dev TCC | 保留；A 禁外部引擎，预览、Dev、正式责任 App 不共享授权；独立 Dev 的真实隔离证据仍须补齐 |
| S9：Windows 提升/UIAccess；Wayland token 生命周期 | 不修改、不借 macOS 提案豁免。Windows 无证书时的运行时身份须按其平台另行定义，不从 SmartScreen 提示推导安全保证 |
| S10：cargo deny、actions pin SHA、SBOM | 保留；需对应候选产物的真实结果。文件摘要不是依赖、构建环境或 SBOM 完整性审计 |
| S10：签名 | A 不具备 Developer ID/公证证明，也不把 ad-hoc 或 SHA256SUMS 等同于受信代码签名。已有更新包签名/产物认证只按各自实际链记录；须分列平台代码签名、更新包认证及哈希一致性，不能把 S10 总签名位伪装成 true |

**与前置提案的 `unsigned_preview` 衔接：**将它视为需要显式批准的发行身份档位；本 ADR 定义其中 macOS 原生输入的附加条件，不推定其自动获得 A-α 或正式资格。现行 [07 §7/§8](../../../docs/plans/computer-use-v2/07-quality-gates.md) 没有这个档位；A-α 即使只列安全前六项，也没有废止 ADR 0006/0012 的身份约束。现有 [Rust 判定器](../../crates/gcu-eval/src/release.rs) 与 [App 镜像](../../../src/lib/computer-use/releaseGate.ts) 均没有 `unsigned_preview`。

建议将身份档位与质量等级分别记录：预览只能在本 ADR 的身份证据及适用 A-α 质量门槛齐备后开放；性能/成功率、Stop、脱敏、真实模型和已开放能力的安全项不因无证书而豁免。不得映射为 `formal`、设置 `signed_and_notarized=true`，也不得将现有 `experimental` 标签当成授权旁路。具体枚举/schema 与前置提案对齐后再批准实现，不在本稿发明已生效的字段。

### 待实施的 ADR、文档和测试改动清单（本次均不实施）

| 位置 | 具体变更 |
|---|---|
| ADR 0006 决定 3、ADR 0012 Q5 / macOS engine 段落 | 加入指向本 ADR 的有限预览例外，列明正式 Developer ID 要求仍有效、路线 A 未获此例外；保留原始探针事实与未测范围 |
| [03 §7/§15](../../../docs/plans/computer-use-v2/03-architecture.md)、[05 路线 B](../../../docs/plans/computer-use-v2/05-route-b-integrated.md)、[llm-wiki](../../../docs/llm-wiki/computer-use.md) | 明确渠道/构建绑定、可信摘要来源、实际实例重验、无 Always、按版 TCC 身份及用户重新批准，不把同 UID 当身份 |
| 07 §7 S9/S10、§8；[security](../security.md) G1/G6/G7；[release-readiness](../release-readiness.md)；[审阅包](../external-review-package.md) | 分开记录预览例外与未满足的证书证明，补按构建原生证据；保持 not_run。已有所有者豁免第三方签字的决定不等于豁免技术证据，无需新增第三方审批流程 |
| [BUILD](../../../docs/BUILD.md)、[CU README](../../README.md)、[中文 README](../../README_ZH.md) | 链接档位条件、安装来源限制、每版授权和旧 TCC 手动撤销说明；不重写已发布 CHANGELOG 或复制主 App 分发分析 |
| Host `supervisor.rs` / `codesign.rs` / build.rs；协议 `supervisor/policy.rs`、`task.rs`、`session.rs` | 实现显式档位、最终摘要来源、包内路径限制、每次启动重新探测和实际映像绑定；保留正式准入与独立控制认证。打包后修改字节的风险需另审最终产物流程，本次不改签名脚本 |
| [Host supervisor 测试](../../../src-tauri/src/computer_use/tests/supervisor.rs)、[协议 supervisor 测试](../../crates/gcu-host-protocol/tests/supervisor.rs) | 保留 `release_identifier_only_is_refused` 和正式 Developer ID 正/负例；新增预览匹配通过、单字节变化/缺固定值/混版/外部路径/环境覆盖/逃逸链接拒绝、签名失败无回退、重启重新探测、校验与启动间替换拒绝；标明 fake probe 与真机的差别 |
| [审批存储](../../crates/gcu-host-protocol/src/approval/disk.rs)、[App policy store](../../../src-tauri/src/computer_use/policy_store.rs)、[目标授权比较](../../crates/gcu-protocol/src/grant.rs)及其测试 | 新增旧正式/Dev/fake/预览 Always 不放行、预览 Always 写入拒绝、换 Host/engine/策略/渠道与回滚均重批、取消/超时保持关闭；保持 ADR 0015 的目标完整身份比较 |
| Rust `release.rs`、[runner_release 测试](../../crates/gcu-eval/tests/common/runner_release.rs)、App `releaseGate.ts` / [测试](../../../src/lib/computer-use/releaseGate.test.ts)、[security 测试](../../crates/gcu-eval/tests/security.rs) | 对齐显式身份档位与质量判定；新增缺身份/真机证据不能开放、无签名不能升 formal、缺 Team 不自动降级、bench/partial/not_run 不提升；证据检查承认具名例外但禁止伪造签名通过 |
| CU 15 语言 catalog、审批卡/设置/诊断与既有 UI 测试 | 完整覆盖首次/更新/取消/忙碌/错误/不支持路径和明确身份披露；Always 不可用并说明原因；新设置必须登记，App shell 不增长 |

## 备选比较与推荐理由

| 方案 | 安全代价 | 实现工作量 | 用户影响 |
|---|---|---|---|
| **A：精确哈希固定 + 每版重新授权（推荐）** | 依赖用户最初信任的未认证 Host/发布渠道；不防能同时改 Host/固定值的同用户恶意软件；TCC 和实际映像绑定须额外证明 | 中高：现有 ContentSha256 可复用，但需收紧路径/重启事实、最终产物校验、授权换代、TCC 身份和档位/UI/证据判定 | 可向知情用户提供有限预览，无证书费用；每版多次批准、无 Always、不自动接续，系统权限条目增多 |
| **B：仅本机自用的开发身份** | 用户自行信任自己构建的代码；identifier-only 跨重建保留 TCC 的便利及替身风险由该本机开发环境承担，不宣称适合分发 | 低：复用独立 Dev App/helper 与明确开发授权；独立身份/TCC 仍需验证，不把现有 debug 宽松策略当产品方案 | 所有者可本机开发试用，每人自行构建/授权；不能用 Dev 包替代公开 CU 预览，不能继承正式/预览 TCC |
| **C：维持现状，原生发布需要 Developer ID** | 保留受信发布者链；仍需防重放、完整映像/依赖绑定和本机被攻陷等剩余风险，证书不使其他门禁自动通过 | 身份政策变更少；需要所有者取得 Apple 账号/证书并完成真实签名、公证、正负链和 TCC 验收 | 目前无证书则原生输入不开放；主 App 既有分发不受这项决策阻断，未来授权体验按正式策略验收 |

推荐 A 是因为它把“允许未认证发布者的有限预览”写成可审阅的政策选择，且能利用已有精确摘要机制；它没有假装提供 Developer ID 的发布者保证。成本和能力缩减是明确条件，而非用免责声明代替校验。若所有者只要本人自用，B 更省工作；若不接受 A 的残余风险和重新授权负担，选 C。

## 实施及开放前必须完成的真机验证

**全部 not_run（本任务不连接机器、不运行 cargo、不操作权限）。** 测试只用无账号的夹具与系统自带 App，由有权限的工程师和机器前用户执行。先记录 mac-mini / mac-pro 的实际 OS、架构、SIP、候选 commit/渠道和最终产物摘要；mac-mini 优先验证已准入的 macOS 27 arm64。历史 mac-pro 的 macOS 15.8.1 x86_64 记录只可作参考，不能假定当前配置，更不能由它开启未准入平台。

| 编号 / 机器 | 必测行为与预期 |
|---|---|
| M1 / 两台 | 对解包的最终 App/engine 核对 SHA-256、`codesign --verify --strict`（App bundle 加 `--deep`）、`codesign -dvvv`、`codesign -d -r-`；确认没有 Team、实际 DR 非 identifier-only、实际切片和内嵌固定值一致。包内再次签名/重打包造成摘要变化必须被发现 |
| M2 / 两台 | 用隔离夹具比较同 identifier 默认 ad-hoc 重建、identifier-only 重建、同 identifier 不同无害替身、不同 identifier 及按构建身份；逐项记录辅助功能/屏幕录制/输入监控是否保留或重授。补 ADR 0006 未完成的同标识替身正向测试；无法重现也如实记录，不以探针一次 false 宣称无风险 |
| M3 / mac-mini；mac-pro 仅受支持部分 | 从正常 GUI/Launch Services 启动责任 App，再由 Host 启动真实 engine；与终端直接运行、外部 Dev、旧预览和正式身份区分。确认权限归属、审批显示对象及按版 TCC 隔离；不是只测 `/usr/bin/true` 或终端 preflight |
| M4 / mac-mini；mac-pro 做权限探针 | 用户分别撤销三类 TCC，重启责任 App/系统会话后复核；权限缺失、输入监控失效、安全提示、错误 PID/旧窗口实例时关闭依赖能力并停止，不借父进程/另一路径继续 |
| M5 / mac-mini | 首次安装、旧 identifier-only 迁移、更新、重建、同版本号不同字节、不同 Host 搭配相同 engine、回滚、重装、旧新版并装；均不继承旧 CU 批准，不读写 Always，不自动 resume；新 App 标识/数据目录不能清旧 Stop，切版不能留下旧引擎投递；新责任身份由用户手动授权，旧系统条目仍存在时明确提示手动撤销 |
| M6 / 两台 | 用无输入夹具在读取、校验后到 exec、运行后重拉间替换/rename engine 或路径链接；替换为另一有效 ad-hoc 文件；旧 facts 不得放行。核对运行中实际映像，不接受仅 PID 正确或连接握手成功。若不能保证身份，关闭原生输入 |
| M7 / 两台 | 缺/错固定值、篡改引擎、损坏签名、签名探针失败/超时、环境路径覆盖、包外链接、可写 dylib/插件及加载环境影响；确认失败关闭、无名字/自报哈希回退。替换 Host+固定值的无害演示仅用于确认披露的保护边界，不把该攻击预期成功记为方案防御通过 |
| M8 / mac-mini | 真实 App → engine 链中执行首次读取/截图/输入、用户取消、审批超时、Stop/断线/引擎重启/版本切换；确认输入释放、持久 Stop、用户独立恢复、旧 token/审批/图片失效，维持既有物理 Stop 时限。身份更新工作不能阻塞 Stop |
| M9 / mac-mini；mac-pro 核对负例 UI | 在机器前核对首次和升级披露、摘要与责任 App 显示、Always 关闭、缺证据入口关闭、取消无输入；诊断不能把 ad-hoc/摘要匹配显示为 Apple 认证。未准入 OS/架构仍拒绝原生装配 |

实施后的回归入口包括：在 `computer-use/` 串行运行 `rcargo test -p gcu-host-protocol --test supervisor`、`rcargo test -p gcu-eval --test runner runner_release`；在匹配 Mac 的 `src-tauri/` 运行 `cargo test -p grok-app computer_use::supervisor`；前端运行 `corepack pnpm exec vitest run src/lib/computer-use/releaseGate.test.ts`。安全证据引用测试需完整检出（含 App 和 workflow），`rcargo` 缺外部文件时的 SKIP 不算通过。上述单测均不能代替 M1–M9，且本次均为 not_run。

每项真机证据须区分 pass/fail/skip/not_run，关联候选构建与原始输出/受控截图；记录首次失败，不使用真实账号、钥匙串凭据或修改 SIP 来绕过限制。Mac 结果不外推 Windows/Linux。

## 退出条件与待裁定事项

若 TCC 按构建隔离、实际执行映像绑定、最终产物固定或 Stop 不能得到证实，则保持原生输入关闭；修订设计或退回 B/C。发现替身复用权限、加载路径绕过或发布来源被攻陷时，撤下受影响候选并指引用户撤销旧系统权限；不自动迁移授权。获得 Developer ID 后切换到正式身份也需要新的构建绑定、用户批准和对应验收，不自动提升发布等级。

**NEEDS-DECISION:** 所有者选择 A（推荐：有限未签名预览，接受上述残余风险与每版重授成本）、B（仅本机 Dev 自用）或 C（维持正式 Developer ID 门禁）。本稿维持 Proposed；需先取得前置提案全文并对齐档位定义，任何选择都不替代未完成的真机证据或授权本任务实施代码。
