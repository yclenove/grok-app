# M1 Managed 分发、离线启用与发布治理

**状态：** 实施规格；签名安装包、发布和平台验收尚未运行。
**关联：** [总体设计](../2026-09-04-browser-rearchitecture-design.md)、[共用协议](00-contracts.md)、[M1 Runtime](m1-runtime.md)、[实施计划](../../plans/2026-09-05-browser-rearchitecture/m1-delivery.md)。
**边界：** M1-D01 至 M1-D06；App 与 Runtime 原子发行，独立 component updater 留给 M6。

## 文件职责与当前代码证据

| 文件 | 当前事实或计划职责 |
| --- | --- |
| `.github/workflows/release.yml` | 现有 tauri-action `releaseDraft: false`，签名可选，构建后另聚合 updater/checksum。 |
| `.github/workflows/ci.yml` | 现有 UI/Rust 多平台检查，未证明 Managed 真机输入与最终包生命周期。 |
| `scripts/package-windows-portable.sh` | 现有 Windows portable 打包入口，需要纳入完整 tuple/layout。 |
| `scripts/build-release-config.mjs` | 现有 Tauri updater 合并配置；不构成 Runtime manifest 验证。 |
| `src-tauri/src/updater.rs` | 当前非 Linux 一律允许 updater；`prepare_for_app_update` 会不可逆关闭 voice/IM/ACP/mirror，只能在成功安装或确认交接后执行。 |
| `src/hooks/useUpdater.ts` | 当前直接 `check -> download -> install -> prepare -> relaunch`；需要改为 Host 可信事务与状态投影。 |
| `src/lib/api/system.ts` | 现有 updater API；增加可信事务包装并纠正 install/teardown 顺序说明。 |
| `src-tauri/capabilities/default.json` | 现有前端 updater check/download/install 权限；Managed 官方更新须只经 Host 事务，移除直调绕过。 |
| `scripts/assemble-updater-manifest.sh` | 现有 updater 聚合入口，实现时需复核调用参数和 pointer 更新顺序。 |
| `scripts/publish-website-downloads.py` | 现有稳定别名与 downloads.json，需要在 publish 后更新。 |
| `src-tauri/build.rs` | 现有 updater compile cfg；新增分发资格需独立验证。 |
| `src-tauri/tauri.conf.json` | 现有 bundle 配置；Runtime resource/sidecar layout 由 target 配置生成。 |
| `src-tauri/Entitlements.plist` | 现有 App entitlement，不能原样下放到所有 helper。 |
| `docs/llm-wiki/release.md` | 发版唯一来源，CHANGELOG/正文/贡献者要求继续适用。 |
| `scripts/browser-runtime/` | 新建：manifest、平台签名验证、package matrix 和 release transaction。 |
| `scripts/browser-runtime/build-bundle-config.mjs` | M1-D01 创建固定公共接口，D02/D03 分别修改/消费平台布局，互不依赖对方任务产物。 |
| `browser-runtime/release/` | 新建：签名策略 schema、目标清单、公开验证 key 与 bootstrap 模板。 |
| `src-tauri/src/browser/runtime/distribution.rs` | 新建：官方分发 attestation 与本机资格。 |
| `src-tauri/src/browser/runtime/policy.rs` | 新建：bootstrap/cache/sequence/expiry/kill switch。 |

未来文件均为计划交付。脚本命名引用以实施时当前仓库复核为准，不跳过既有发布规则。

## 目标、非目标与发行类别

目标是让官方 Managed 包在完整签名、离线资源、真实平台验收全部通过后才对用户开放。
macOS arm64/x64 DMG 与 Windows x64 NSIS/portable 是 M1 Managed 支持矩阵。
现有 Linux 和未签名社区发行继续提供 Preview/manual，不能被 remote flag 或 UI setting 升格。
M1 不增加独立 Runtime 在线下载器，不把首次 Agent Browser 点击当下载入口。
不承诺无限期离线自动化；过期 bootstrap、撤销 tuple 或不可信系统时钟只开放 Preview。
不把 macOS 交叉编译、Windows 交叉编译、开发目录测试算作最终安装包运行证据。
M1 portable 为 manual-update-only，不能执行 NSIS 静默安装或写入安装版目录。

## 不可变 Tuple 与发行合同

Runtime manifest 固定 Node patch、playwright-core、Chromium revision、Guard、协议版本和 target triple。
manifest 同时包含 runtimeId、releaseVersion、App compatibility、Profile epoch、capability、license/SBOM/source commit。
每文件记录相对路径、类型、size、SHA-256，Framework symlink 另记录精确相对目标。
tuple 构建完成后签名并 immutable，App installer 内包含完整字节与嵌入 manifest。
archive 文件名必须版本化或 content-addressed；同名资产不得被替换成不同 digest。
外层 archive digest、manifest signature、平台 code signature 与 Tauri updater signature 分工独立。
任何包含 Managed capability 的官方包缺少其中任一适用签名即构建失败，保持 Draft。
编译 attestation 绑定 App source commit、target、runtime manifest digest 与发行类别。
Host 校验可信签名和 attestation 的一致性；任意环境变量或设置本身不授予官方资格。

```text
source + fixed dependency lock -> tuple -> platform signing -> immutable manifest
  -> App attestation + full package -> native verification -> Draft upload
  -> independent clean runner download/verify -> publish -> signed index switch
```

manifest 的签名 payload 使用固定 canonical JSON 编码，签名字段不参与自身摘要。
全字段使用 camelCase；schemaVersion 与 protocolVersion 独立，不混用 App semver 比较安全 sequence。
发行与策略 sequence 使用规范十进制 string，验证器用 BigInt 或等价无符号比较，禁止 JS Number 精度截断。
不在公开 manifest、SBOM、日志或 evidence 中存 secret、签名私钥或机器私有路径。
license 文件随 App、NSIS 和 portable 一致分发，不能仅在源仓库存在。

M1-D01 创建 `buildBundleConfig({schemaVersion: 1, targetTriple, distribution, tupleRoot, manifestPath, bootstrapPath, attestationPath})`，返回 `{config, layout}`；输入文件须已验证且 target/digest/distribution 一致。
CLI 固定 `--target --distribution --tuple --manifest --bootstrap --attestation --out`，只向 out 写 `tauri.browser-runtime.conf.json` 与 `bundle-layout.json`，不签名、不上传、不修改源配置。
distribution 枚举 `macos-dmg | windows-nsis | windows-portable`；layout 明确 App/resource/sidecar 相对位置、manifest digest、package kind 和 updater policy，禁止绝对运行路径及缺文件默许。
D02/D03 在此合同下扩充各自平台规则；生成输出按 target/distribution 分目录，portable 与 NSIS 必须分别编译绑定发行类别的 App binary，不能给同一 exe 换可编辑 marker 来改变更新权限。

## 离线 Bootstrap 与策略

官方包嵌入仅授权该精确 tuple/capability 的签名 bootstrap policy。
policy 包含 sequence、keyId、issuedAt、expiresAt、runtimeDigest、capabilities、mode 与 key/revocation metadata。
Host durable 缓存最高 sequence、revocation floor、已接受摘要和最后可信时间观察值。
低 sequence 拒绝；同 sequence 仅接受同 digest；key rotation 必须由当前信任链验证。
首次离线、无 cache、bootstrap 有效且时钟可信时可启用，写入最高 sequence。
已有更高 cache、撤销记录或 stop_all 时，嵌入 bootstrap 不得覆盖。
cache fetch 失败且原 policy 未过期可继续；过期或 clock rollback 异常变为 block_new。
离线的 clean install 与已撤销 reinstall 分别测，不能通过卸载清缓存绕过防重放记录。
durable 反重放记录的保留和重新安装识别须在 OS 支持边界内明确；丢失可信记录时需在线重新验证。

Host 在 Managed binding/Runtime 创建、lease 签发/续期、每 command admission 检查策略。
worker 每 primitive 检查 Host policy epoch、lease 和 fence，不能无限续跑旧 batch。
block_new 禁止新绑定、lease、续租与 command；已派发工作仅在原 lease 有效时有界 drain。
只有新鲜且明确签名的 stop_all 可因远程策略强制终止既有 Runtime。
stop_all 原子关闭 admission、递增 fence、撤销 lease/upload、取消队列，再 drain/kill 命中 Runtime。
本地安全故障、崩溃与用户关闭仍遵循 Supervisor 自身终止权限。
与停止竞态的副作用返回 unknown_outcome，不能因安全事件自动重试。
BrowserSession、Preview、Gateway 基础能力保持可用，UI 投影 reasonCode 由 i18n 文案映射。

## macOS 最终包

覆盖 Apple Silicon 原生运行与 Intel 真机/x64 VM；最低支持 OS 和当前稳定 OS 分别记录。
每个 Node、Guard、Chromium helper app、Framework、Mach-O、dylib 都需要枚举验证。
检查 Team ID、designated requirement、hardened runtime 与该角色最小 entitlement。
允许 JIT 的范围必须来自固定 Chromium/Node 的实际需求，不能把 App entitlement 复制到所有文件。
签名先内后外，manifest 明确 hash 所针对的签名后文件；签名再改变文件后必须重算 tuple。
`codesign --verify --deep --strict` 只是补充，不能替代逐项检查。
最终 app/DMG 完成 notarization、staple validation 和 Gatekeeper spctl。
allowlisted Framework symlink 在提取与最终 app 中都逐跳复验，bundle 外目标立即失败。
从已下载带 quarantine 的 DMG 安装后验证 Input Monitoring 权限、拒绝/撤销与 sleep/wake。
签名身份或路径变化后检查权限是否需要重新授权，不能继承开发构建的权限结论。
安装路径含空格和非 ASCII 时完整启动、输入接管、上传/下载、退出；不把路径写入 audit。

## Windows 最终包

覆盖普通用户 Windows x64、最低支持 OS 与当前稳定 OS，保留 Chromium sandbox。
NSIS 与 portable 解包后枚举每个 PE，对照 Runtime manifest 并验证 publisher/chain/timestamp。
每个 PE 使用 `signtool verify /pa /all`；Tauri `.sig` 不能替代 Authenticode。
Guard 必须在最终安装路径证明 suspended-before-Job、KILL_ON_JOB_CLOSE、无 breakaway。
Defender/Gatekeeper 类结果属于平台证据，不能以关闭防护软件过门。
portable 包含 Grok.exe、完整 immutable tuple、manifest、license 和相对 bootstrap layout。
`updater.rs::is_auto_update_supported` 与 `updater_status` 从已验证、编译绑定的 distribution 判定；windows-portable/未知或错配身份均返回 manual，任何 check/download/apply Host command 再次拒绝安装。
`useUpdater.ts` 的 About、后台检查和 Apply 共用该资格；portable 可查看手动下载，不能创建 Tauri Update handle、下载 NSIS 或调用安装；仅测试 packaging metadata 不算完成。
portable 移动目录或盘符后使用新 app-data resolver，保持 executable containment 验证。
portable 写入权限不足时显示原因，不能回退 PATH Node 或在线安装 Chromium。
安装版 WebView2 与 Managed Chromium 是不同依赖；离线首启 fixture 需声明 WebView2 前置条件。
若最低支持 OS 缺 WebView2，提供产品现有安装路径的明确结果，不声称安装完全无前置条件。
下载 export 应保留/应用并验证 MOTW；导出文件不会被自动打开。
无 administrator/uiAccess 的原生 input Provider 正常归属窗口且不被 Playwright 输入自触发。

## CI 与确定性 Package Matrix

保留既有 typecheck/test/lint/build、quality gates 与 Rust fmt/clippy/test。
新增 unit/contract fixture 验证可在 PR 运行，但不能伪装成原生安装包 gate。
受控签名 runner 构建，独立 clean runner 重新下载，原生 runner 安装运行。
macOS x64 的执行 runner 必须是 Intel/x64 VM；arm 上 build 成功仅算构建行。
每支持 package/OS 对声明 fixture 连续三轮 100% 通过；不以 beta 98% 替代。

| 场景组 | 断言 |
| --- | --- |
| clean/offline | 完整 tuple 本地存在；有效 bootstrap 可用，过期 bootstrap 仅 Preview。 |
| upgrade/downgrade | App N-1/N x Runtime N-1/N，活跃 Runtime 固定 generation，不改写 active 目录。 |
| Profile epoch | 兼容 generation 恢复；不兼容拒绝，原数据保留。 |
| crash injection | pointer 前后、receiving/exporting、promotion owner/journal、Host/worker/Guard 崩溃。 |
| partial release | 缺资产、404、坏 hash、错误平台、下载中断均保持当前版本。 |
| storage | 低磁盘、坏 archive、no-follow traversal、symlink/reparse、ACL、备份与路径字符。 |
| teardown | 两份 App 竞争、用户 Chrome 同开、sleep/wake，30 秒本 run descendants=0。 |
| privacy | 200 MiB ACK 后晚读、撤权后 spool 保留占额、Runtime exit proof 后 purge、Artifact owner、export metadata、audit/support redaction。 |
| updater client | 可信 index/实际 payload 验证、portable 拒绝、Browser install 前 quiesce、Windows 交接失败、ACP/IM 存活、无 Agent 自动恢复。 |

fixture server 固定响应、明确 barrier、seed 与 fault index；无需外部网站和真实登录凭据。
Golden path 联合 Gateway、Runtime、Workbench 验证，检查服务器副作用计数和文件系统 oracle。
五个视觉压力 locale 为 en/zh/de/ru/ta；15 locales 做 key parity 与非空检查。
证据写 `docs/qa/browser-rearchitecture/m1/delivery/`，包括 package digest、平台、测量边界和失败行。
当前没有这些运行结果；预先建的样例只能标 schema/example，不得进入通过分母。

## Draft 到 Publish 的事务

构建 job 只创建或复用同一 tag 的 Draft release，校验 CHANGELOG 对应版本章节。
独立资产清单精确列平台、架构、文件名、size、digest、签名及 capability 资格。
所有 required assets 上传成功后 clean runner 从远端重新下载并逐项验证。
缺包、blocking warning、签名错配、下载失败或内容变化均 job fail，Release 保持 Draft。
通过后唯一 publish job 发布 Release；不让矩阵中的单个平台提前公开。
发布成功后才切签名 versioned index/updater pointer，最后生成稳定别名/downloads.json。
客户端 Host 验证受信 key 签名的 versioned index、规范 sequence/防回放 floor、expiry、App/platform/distribution/runtimeDigest、完整资产清单及独立复验 receipt；再验证当前目标所需全部 payload 实际可下载且 size/hash/平台签名匹配，404/半发布继续当前版本。
GitHub 多资产不是原子事务，当前 pointer 一直保留到完整新集可用。
重新执行同 digest 可幂等；已发布同名不同 digest 必须新版本，不能悄悄覆写。
Release 正文仍由 changelog-for-release.py 生成，只含版本变更。
不得手改已发布 CHANGELOG；发版刷新 README 圆头像并遵守首句长度要求。

## 实际客户端更新事务

M1-D03 接入 portable 资格，D05 修改真实 `updater.rs`、`useUpdater.ts`、API/command/ACL 与安装适配；只改发布 fixture 或 configuration 不算完成。
Host 新建 `app_update_check_trusted`、`app_update_download_trusted`、`app_update_apply_trusted`、`app_update_close`；UI 只持有绑定 hostBootId/indexDigest/distribution 的 opaque updateId，不能自报 endpoint、payload 路径或 Runtime identity。
check：验证 index 和资格，再用真实 Rust `UpdaterExt::updater_builder().endpoints(...).build()?.check().await`，核对返回 Update 的 version/target/download_url/signature 与 index 完全匹配；持久防回放状态不与 Browser policy 或 component sequence 混用。
download：Host 的 `Update::download` 验证 Tauri 签名，再核对 index size/hash、适用平台签名与内嵌完整 tuple；全部成功才 ready。About/后台检查停在 ready，不开始 Browser quiesce。
Apply：重新检查资格/策略/index 期限与不可变已下载 bytes，取得单一事务锁，再调用 R06 `beginAppUpdateQuiesce(updateId)`；完成 Browser admission close/fence/revoke、上下文持久化、全部 Runtime drain/descendants=0/spool purge 后才允许 install。
quiesce 不调用 `prepare_for_app_update`，其部分失败必须清除更新锁并保留 Browser user latch、提供人工重开/Preview；ACP/IM/voice/mirror 继续运行。
macOS（及原有可更新 AppImage 的非 Managed 路径）：Host 执行真实 `Update::install(bytes)`；返回成功后 commit Browser quiesce -> `prepare_for_app_update` -> `AppHandle::restart`。install 返回错误则 abort quiesce，不执行后两步，不恢复 Agent lease/队列。

固定依赖 `tauri-plugin-updater 2.10.1` 的 Windows `Update::install` 在 `on_before_exit` 后调用 `ShellExecuteW`，忽略启动返回值并 `process::exit(0)`；不能按 JS await 后续清理设计，也不能把 on_before_exit 当成功安装通知。
Windows NSIS 使用 D05 新建 Host handoff adapter：只接受 index 声明的已验证 NSIS exe bytes，落 user-only immutable staging 并复验 Authenticode；不调用插件 Windows install，也不接受未知 archive 格式。
adapter 以 `ShellExecuteExW(SEE_MASK_NOCLOSEPROCESS)` 检查启动错误/UAC 取消并持有 installer process handle；签名、版本锁定的 NSIS template 在任何 kill/uninstall/文件替换前完成无副作用 preflight，发送 ready，再等 commit 与已绑定旧进程退出。
握手使用当前用户 ACL、随机 nonce 的私有 ready/commit/abort event，进程身份含 PID/start time，禁止由命令行 marker 单独确认；安装器未 ready 的早退/超时使 Host abort，ACP/IM 不停且 Browser 仅人工恢复。
ready 与进程存活核实后持久 `handoffAccepted`，才 commit Browser quiesce -> `prepare_for_app_update` -> signal installer commit -> `AppHandle::exit(0)`；Windows 不靠 JS 收到返回，也不另调 relaunch。
安装器在旧 PID 确认退出后才替换 App/完整 tuple并记录结果、启动新 App；新 App 仍按 activation/canary/epoch 验证，Browser 不自动续跑。
失败保证的边界是 macOS install 返回失败或 Windows handoffAccepted 前失败，均不关闭 ACP/IM；交接后的磁盘/安装器故障不能保证已退出旧进程仍存活，必须持久 failed/unknown 结果并由下次启动/人工修复处理，不能报告安装成功。
取消或卸载 Hook 不得让已派发 install 重复执行；opaque updateId 具有 single-apply 状态，安装未知结果不自动 retry，手动再次操作必须重新检查当前版本与事务记录。

```text
checkTrusted -> index verify -> Tauri check -> match Update
downloadTrusted -> Tauri download/signature -> payload/tuple verify -> ready
confirmed Apply -> revalidate -> Browser quiesce + exit proof + spool purge
  macOS: Update.install success -> commit -> prepare_for_app_update -> restart
  Windows: checked spawn -> installer ready -> durable handoffAccepted
           -> commit -> prepare_for_app_update -> signal -> exit -> installer result
before-success/acceptance failure -> abort -> ACP/IM alive + Browser user control
```

D05 在真实 hook/Host 测试中断言以上调用顺序，覆盖 portable、坏 index/404/截断、quiesce 部分失败、install 错误、Windows 取消/早退/握手超时和交接后失败；D04 harness 重跑新生成最终包，D06 只接收该实际证据。

## 回滚与发布环

App/Runtime 一起升级；活跃 Profile 先 drain，兼容 canary 成功后切 pointer。
canary 失败最多一次本地 LKG；被撤销或 epoch 不兼容的旧 tuple 不可使用。
正式 rollback 需要更高 sequence 的签名指令，精确旧 tuple 且不降 revocation floor。
恢复使用兼容 Profile generation，不能原地向后打开、wipe 或 merge 新 epoch 数据。
无可用回滚组合时禁用 Managed、保留数据和 Preview，并给出修复诊断。

发布顺序为 fixture -> internal dogfood -> opt-in beta -> platform cohort -> default。
每次升环两个 24-48 小时窗口，每个平台/Runtime build 至少 100 sessions、1000 正常动作。
同时要求 30 冷启、10 crash/cleanup、5 upgrade/rollback；分母不够延长观察。
beta 指标为成功率 >=98%、unknown_outcome <=0.5%、crash-free >=99.5%，自动 replay=0。
安全越权、secret 泄漏、Profile 串用、数据丢失、orphan 或接管后动作任一例立即停止升环。
仅用明确同意的匿名计数或本地 dogfood 记录，没有全量生产 telemetry 的默认承诺。
M1-D06 输出逐平台升环/阻断结论；未获 release approval 保持 opt-in，不由时间自动升环。
本文件规划发布行为，不代表本轮执行了签名、上传、publish、tag 或回滚。
