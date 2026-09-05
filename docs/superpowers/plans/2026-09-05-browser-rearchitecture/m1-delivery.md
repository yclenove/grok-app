# M1 Managed 交付与发布实现计划

> **面向 AI 代理的工作者：** 使用 subagent-driven-development 或 executing-plans 逐任务实现。步骤使用未勾选复选框追踪；本文件记录未执行待办，当前文档交付不代表产品完成。实施期间的 commit/push 遵循总索引，签名与正式发布遵循仓库 release 规则。

**目标：** 官方 Managed 包具备离线完整 tuple、平台签名、最终包矩阵、可验证 Draft 发布与受控回滚。
**架构：** App/Runtime 一体发行，Host 依分发 attestation 与签名 policy 开放 Managed；构建、重新下载验证、publish 分成明确事务阶段。
**技术栈：** GitHub Actions、Node/Rust、Apple codesign/notary、Windows Authenticode、Tauri updater、Playwright Test 与原生外置驱动。
**规格：** [M1 Delivery](../../specs/2026-09-05-browser-rearchitecture/m1-delivery.md)、[M1 Runtime](../../specs/2026-09-05-browser-rearchitecture/m1-runtime.md)。
**估算：** 32-45 工程日；硬件、证书和两个观察窗口等待时间另计。

## 文件职责与当前代码证据

| 路径 | 当前证据与本计划职责 |
| --- | --- |
| `.github/workflows/release.yml` | 当前 releaseDraft:false、签名可选；新增 Managed release 硬门与唯一 publish job。 |
| `.github/workflows/ci.yml` | 当前 UI/Rust 测试；扩展 contract、worker 与 package evidence 验证。 |
| `scripts/package-windows-portable.sh` | 当前复制单 exe/README 并可直接 gh upload；改完整资源布局与 staging-only。 |
| `scripts/assemble-updater-manifest.sh` | 当前聚合 updater；调整为 publish 后 pointer 更新。 |
| `src-tauri/src/updater.rs` | 当前非 Linux 一律允许更新，prepare_for_app_update 不可逆关闭 ACP/IM；D03/D05 接入实际判断和事务。 |
| `src/hooks/useUpdater.ts` | 当前直调 Tauri JS check/download/install；D03 约束 portable，D05 改 Host 可信事务。 |
| `src/lib/api/system.ts`、`src-tauri/capabilities/default.json` | 当前 updater 包装与直调 ACL；D05 增加可信 command API 并关闭绕过。 |
| `scripts/publish-website-downloads.py` | 当前稳定别名和 downloads.json；位于完整发行 publish 后。 |
| `src-tauri/build.rs` | 当前 updater compile cfg；新增 attestation 验证，不接受任意环境布尔值。 |
| `src-tauri/tauri.conf.json` | 当前 App bundle 配置；纳入目标 runtime resources。 |
| `browser-runtime/release/` | 新建公开 schema/keys、bootstrap、artifact matrix。 |
| `scripts/browser-runtime/` | 新建签名验证、发布事务与 evidence 聚合脚本。 |
| `tests/browser/` | 新建集成 fixture、Playwright Test 配置与外置原生驱动。 |

前置 M0-R01 的 worker 独立 lock/config 和 tuple 工具，M1-R06 的运行时状态/清理接口。
以 docs/llm-wiki/release.md 为发版唯一来源；现有社区/Linux 包在 M1 继续 Preview/manual。
新增文件和命令按所属任务创建，本文件不是已执行记录。
未来 evidence 根为 `docs/qa/browser-rearchitecture/m1/delivery/`。

### M1-D01：分发资格、签名 Bootstrap 与离线策略

**依赖：** M0-R06、M1-R01。
**负责：** 供应链工程师、Host 安全工程师。
**估算：** 5-7 工程日。
**文件：**
- 创建：`browser-runtime/release/distribution.schema.json`、`browser-runtime/release/policy.schema.json`、`browser-runtime/release/trusted-keys.json`。
- 创建：`src-tauri/src/browser/runtime/distribution.rs`、`src-tauri/src/browser/runtime/policy.rs`、`src-tauri/src/browser/runtime/policy_tests.rs`。
- 创建：`scripts/browser-runtime/build-attestation.mjs`、`scripts/browser-runtime/build-bootstrap.mjs`、`scripts/browser-runtime/policy-fixtures.test.mjs`。
- 创建：`scripts/browser-runtime/build-bundle-config.mjs`、`scripts/browser-runtime/build-bundle-config.test.mjs`、`browser-runtime/release/bundle-layout.schema.json`。
- 修改：`src-tauri/build.rs`、`src-tauri/src/browser/runtime/mod.rs`、`src-tauri/src/browser/runtime/activation.rs`。
- 测试：`src-tauri/src/browser/runtime/policy_tests.rs`、`scripts/browser-runtime/policy-fixtures.test.mjs`、`scripts/browser-runtime/build-bundle-config.test.mjs`。
**交付行为：** 精确 tuple 的官方资格/离线策略，加 D02/D03 可独立消费的公共 bundle-config 生成器。

- [ ] 定义 canonical JSON 签名 payload，attestation 绑定 source/target/runtimeDigest/distribution，policy 含 sequence/keyId/issuedAt/expiresAt/mode/capabilities；规范十进制 string 加超 2^53 fixture；固定 macos-dmg/windows-nsis/windows-portable 枚举与 bundle-layout schema。
- [ ] 写 unsigned/community、tuple/发行类别错配、过期 bootstrap、更新 cache、clock rollback、same-sequence 冲突、revocation floor 和三 target bundle layout 失败测试；portable/NSIS 相互替用 attestation 必须拒绝。
- [ ] 创建公共 buildBundleConfig({schemaVersion:1,targetTriple,distribution,tupleRoot,manifestPath,bootstrapPath,attestationPath}) -> {config,layout}；CLI --target/--distribution/--tuple/--manifest/--bootstrap/--attestation/--out，仅写 out 下 tauri.browser-runtime.conf.json 与 bundle-layout.json，校验输入身份/digest/资源存在，D02/D03 不依赖对方生成器工作。
- [ ] 实现 build-time attestation 验证/发行类别编译绑定与 Host 二次一致性检查、最高 sequence/摘要/revocation 原子 cache；bootstrap 不覆盖更高状态，可信记录丢失需在线复验，普通开发构建 Preview。
- [ ] 接 Gateway binding/lease/admission 与 worker policyRevision：block_new 有界 drain，fresh stop_all fence/revoke 后终止 Runtime，upload spool 仅在 descendants exit proof 后 purge；fetch 失败不强杀已有 Runtime。
- [ ] 跑断网有效/过期 bootstrap、撤销重装、clock 异常与 macOS 两架构/Windows 两发行类别生成 fixture，保证 Preview 可用；生成器无签名/上传副作用，输出按 target/distribution 隔离。

```rust
#[test]
fn bootstrap_never_replaces_newer_stop_all() {
    let cache = PolicyState::test_stop_all(12);
    let bootstrap = SignedPolicy::test_allow(10);
    assert_eq!(cache.select(&bootstrap).mode(), PolicyMode::StopAll);
}
```

运行：`cargo test --manifest-path src-tauri/Cargo.toml browser::runtime::policy_tests`。
运行：`node --test scripts/browser-runtime/policy-fixtures.test.mjs`。
运行：`node --test scripts/browser-runtime/build-bundle-config.test.mjs`，预期 D02/D03 可用同一冻结接口验证各自布局，缺参数/资源/身份错配均拒绝。
预期：有效cache断网可用；过期/时钟异常block_new；同sequence不同内容拒绝；fresh stop_all才执行远程终止。
**失败处理：** 无资格或无可信策略时Managed unavailable；不依赖PATH下载替代浏览器。
**验收：** bootstrap只授权精确tuple/capability，block_new与stop_all生命周期明显区分。

### M1-D02：macOS 完整 Tuple 签名与真实包验证

**依赖：** M1-D01、M1-R06。
**负责：** macOS 发布工程师、原生 QA。
**估算：** 4-6 工程日。
**文件：**
- 创建：`browser-runtime/release/macos-signing-rules.json`、`browser-runtime/release/entitlements/guard.plist`、`browser-runtime/release/entitlements/node.plist`。
- 创建：`scripts/browser-runtime/sign-macos.mjs`、`scripts/browser-runtime/verify-macos.mjs`、`scripts/browser-runtime/verify-macos.test.mjs`。
- 创建：`browser-runtime/release/macos-layout.json`；生成：`.artifacts/browser-runtime/<target>/macos-dmg/tauri.browser-runtime.conf.json`（非源文件）。
- 修改/消费：D01 的 `scripts/browser-runtime/build-bundle-config.mjs`、`scripts/browser-runtime/build-bundle-config.test.mjs`；仅扩 macOS 规则，公共接口保持固定。
- 修改：`src-tauri/tauri.conf.json`、`.github/workflows/release.yml`、`scripts/build-local.sh`。
- 测试：`scripts/browser-runtime/verify-macos.test.mjs`、`scripts/browser-runtime/verify-macos.mjs` 原生包模式。
**交付行为：** arm64/x64最终App与DMG包含完整tuple，内外签名、公证和原生input/cleanup全部可验证。

- [ ] 定义每种Mach-O/helper/framework角色的Team ID、designated requirement与最小entitlement白名单，写缺helper签名/过宽权限/未声明dylib反例。
- [ ] 实现按依赖内到外签名，Guard/Node/Chromium各用声明权限；manifest针对签名后文件生成，避免签名修改字节后hash过期。
- [ ] 调用 D01 生成器的 macos-dmg 分支生成 Tauri resources/sidecar 与完整相对布局，输出到架构独立目录；校验 Framework 精确 symlink 目标、无循环和 bundle 外引用，不依赖 D03。
- [ ] 实现逐Mach-O/dylib/helper枚举验证，附加deep strict校验；执行notary/staple/Gatekeeper并把blocking warning视为失败。
- [ ] 在Apple Silicon与Intel/x64 VM从下载DMG安装，运行权限拒绝/撤销、窗口变更、sleep/wake、Guard EOF与offline startup；开发目录不计通过。
- [ ] 验证空格/非ASCII安装路径、旧版升级与含quarantine下载，记录每个最终包digest与签名明细并上传为Draft候选。

```text
enumerate manifest binaries -> verify Team ID + requirement + entitlements
                           -> verify file hashes + Framework links
                           -> notarization + staple + Gatekeeper
                           -> install DMG -> native input/cleanup probes
```

运行：`node --test scripts/browser-runtime/verify-macos.test.mjs`。
运行：`node scripts/browser-runtime/verify-macos.mjs --from-index .artifacts/browser-runtime/package-index.json --native --evidence docs/qa/browser-rearchitecture/m1/delivery/macos`。
补充验证：`codesign --verify --deep --strict .artifacts/browser-runtime/macos/Grok.app`；脚本逐项验证仍是主门。
**失败处理：** 缺证书、公证失败或权限回归时保留Draft，不能将包降为unsigned后保留Managed capability。
**验收：** 两架构各有真实运行记录；签名后所有manifest hash匹配，sandbox未关闭。

### M1-D03：Windows Authenticode、NSIS 与 Portable

**依赖：** M1-D01、M1-R06。
**负责：** Windows 发布工程师、原生 QA。
**估算：** 5-7 工程日。
**文件：**
- 创建：`scripts/browser-runtime/sign-windows.ps1`、`scripts/browser-runtime/verify-windows.ps1`、`scripts/browser-runtime/verify-portable.test.mjs`。
- 创建：`browser-runtime/release/windows-signing-rules.json`、`browser-runtime/release/windows-layout.json`、`browser-runtime/release/portable-layout.json`、`src/hooks/useUpdater.test.tsx`。
- 修改/消费：`scripts/package-windows-portable.sh`、`.github/workflows/release.yml`、D01 的 `scripts/browser-runtime/build-bundle-config.mjs` 与 `scripts/browser-runtime/build-bundle-config.test.mjs`；仅扩 Windows 规则，不依赖 D02。
- 修改：`src-tauri/src/browser/runtime/distribution.rs`、`src-tauri/src/updater.rs`、`src/hooks/useUpdater.ts`、`src/lib/api/system.ts`、`src-tauri/src/lib.rs`、`src-tauri/tauri.windows.conf.json`。
- 测试：`src-tauri/src/updater.rs` 的 `#[cfg(test)]` 模块、`src/hooks/useUpdater.test.tsx`、`scripts/browser-runtime/verify-portable.test.mjs`、`scripts/browser-runtime/verify-windows.ps1`。
**交付行为：** NSIS和portable都含完整tuple及签名；portable仅手动更新，不转安装版。

- [ ] 写 portable 缺 helper/license、路径/架构/签名错配与 updater 资格测试；真实 useUpdater hook 覆盖 About/后台/Apply，portable 下 plugin check/download/install 均零调用，Host apply 也拒绝。
- [ ] 调 D01 生成器分别编译绑定 windows-nsis/windows-portable attestation 的 binary 与完整 layout，不复制 NSIS exe 冒充 portable；逐 PE 签名/时间戳后生成 manifest，普通用户不依赖外部 Node/Chrome。
- [ ] 修改 updater.rs 的 is_auto_update_supported/updater_status 与 lib.rs 插件注册：核对编译绑定发行类别，portable/未知/错配为 manual；useUpdater.ts 及 system.ts 共用该资格并支持手动链接，不能只改 packaging metadata。
- [ ] 修改 portable 完整 layout 复制和 staging-only 输出，移除独立 gh upload；验证每 PE signtool /pa /all、chain/timestamp 和 manifest，移动目录/盘符仍定位唯一 tuple；WebView2/CLI 前置说明保持明确。
- [ ] 从最终 NSIS/portable 跑 Job、无 breakaway、Guard kill-on-close、Raw Input/IME/sleep/wake 与 Defender，覆盖真实 About/后台/Apply updater 分支；D05 接可信事务后重跑这些断言。
- [ ] 跑低磁盘、只读/非 ASCII 路径、uninstall/reinstall、portable/NSIS attestation 互换和 MOTW export；未知身份 fail closed，portable 零 NSIS 执行，不回退 PATH 下载或自动打开导出文件。

```powershell
param([string]$PackageRoot, [string]$ManifestPath)
$ErrorActionPreference = 'Stop'
$manifest = Get-Content -Raw -LiteralPath $ManifestPath | ConvertFrom-Json
foreach ($file in $manifest.files | Where-Object { $_.kind -eq 'pe' }) {
  & signtool verify /pa /all (Join-Path $packageRoot $file.path)
  if ($LASTEXITCODE -ne 0) { throw "PE signature verification failed" }
}
```

运行：`node --test scripts/browser-runtime/verify-portable.test.mjs`。
运行：`cargo test --manifest-path src-tauri/Cargo.toml updater::tests`、`pnpm exec vitest run src/hooks/useUpdater.test.tsx src/lib/appUpdateHonesty.test.ts`。
运行：`pwsh -File scripts/browser-runtime/verify-windows.ps1 -PackageIndex .artifacts/browser-runtime/package-index.json -Native -Evidence docs/qa/browser-rearchitecture/m1/delivery/windows`。
预期：NSIS/portable完整签名、启动离线资源齐全；Job绑定前first-instruction counter为零。
**失败处理：** Authenticode缺失时官方Managed job失败，不能用Tauri updater.sig替代平台签名。
**验收：** portable更新入口不调用NSIS，用户自己的Chrome未被teardown影响。

### M1-D04：最终安装包的确定性集成矩阵

**依赖：** M1-D02、M1-D03、M1-R06、M1-G06。
**负责：** QA 自动化工程师、三平台负责人。
**估算：** 5-7 工程日。
**文件：**
- 创建：`tests/browser/playwright.config.ts`、`tests/browser/harness/fixture-server.ts`、`tests/browser/harness/native-driver.ts`、`tests/browser/package-lifecycle.spec.ts`、`tests/browser/security-lifecycle.spec.ts`。
- 创建：`tests/browser/native/macos/Package.swift`、`tests/browser/native/macos/BrowserPackageTests.swift`、`tests/browser/native/windows/BrowserPackageTests.ps1`。
- 创建：`browser-runtime/release/package-matrix.json`、`scripts/browser-runtime/run-package-matrix.mjs`、`scripts/browser-runtime/validate-package-evidence.mjs`。
- 修改：`package.json`、`pnpm-lock.yaml`、`.github/workflows/ci.yml`；创建：`.github/workflows/browser-package-qa.yml`。
- 测试：`tests/browser/package-lifecycle.spec.ts`、`tests/browser/security-lifecycle.spec.ts` 及两个原生测试文件。
**交付行为：** 可复用fixture harness在真实安装包上执行完整成功/失败/恢复，三轮无安全违规。

- [ ] 锁定与tuple一致的@playwright/test版本，配置独立testDir=tests/browser；不运行playwright install，使用本地tuple与外置原生driver，避免根src/**测试配置漏扫。
- [ ] 实现固定双 origin server、barrier、seed 和副作用计数；native-driver 统一 startInstalledApp/clickControl/readStatus/stopApp，macOS SwiftPM XCTest 使用 AXUIElement，Windows 用 UIA，在产品外运行；实验 runner 所需辅助功能/屏幕录制授权单独记录，不增加产品权限绕过。
- [ ] 写Preview升级、任务组/窗口同步、origin越权、fence/return、秘密接管、高风险确认、popup/frame/Shadow DOM支持范围与unsupported场景。
- [ ] 写 App N-1/N x Runtime N-1/N/epoch、active update、pointer crash、LKG 缺失/撤销、200 MiB 晚读/撤权后保留/Runtime exit 后 purge、满预算/共享 close、download/export/promotion 与两 App 争锁；D05 添加真实客户端安装事务场景后复用此 harness 重跑最终包。
- [ ] 编排最低/当前OS、三架构、DMG/NSIS/portable三轮；外置driver不加入生产权限绕过，原生输入安装包手工drill单列，不以synthetic事件替代物理来源结论。
- [ ] 接CI与独立clean runner evidence gate，检查15locale parity和en/zh/de/ru/ta视觉压力，未跑行拒绝生成pass，记录所有失败和复跑次数。

```ts
import { defineConfig } from '@playwright/test';
export default defineConfig({
  testDir: '.', testMatch: '**/*.spec.ts', retries: 0, workers: 1,
  reporter: [['json', { outputFile: '.artifacts/browser-package-results.json' }]],
});
```

运行：`pnpm exec playwright test --config tests/browser/playwright.config.ts`。
运行：`node scripts/browser-runtime/run-package-matrix.mjs --native --repeat 3 --evidence docs/qa/browser-rearchitecture/m1/delivery`。
聚合：`node scripts/browser-runtime/validate-package-evidence.mjs --require-all-targets --evidence docs/qa/browser-rearchitecture/m1/delivery`；每 native runner 只报告自己的平台，聚合器拒绝缺行。
预期：required行三轮100%，自动replay/越权/Profile串用/orphan=0；至少30样本才报告P95。
**失败处理：** 部分平台缺包、缺原生设备或测试flaky均阻断对应package；不以跳过/重试掩盖安全失败。
**验收：** Playwright Test负责测试编排，不宣称可直接控制WKWebView；实际UI由外置native-driver完成。

### M1-D05：可信客户端更新、Windows 交接与发布事务

**依赖：** M1-D04。
**负责：** 发布工程师、仓库维护者。
**估算：** 9-13 工程日。
**文件：**
- 创建：`scripts/browser-runtime/release-transaction.mjs`、`scripts/browser-runtime/release-transaction.test.mjs`、`browser-runtime/release/required-assets.json`。
- 创建：`src-tauri/src/updater/release_index.rs`、`src-tauri/src/updater/transaction.rs`、`src-tauri/src/updater/payload.rs`、`src-tauri/src/updater/windows_handoff.rs`、`src-tauri/src/updater/tests.rs`、`browser-runtime/release/update-index.schema.json`。
- 创建：`browser-runtime/release/windows-updater-template.nsi`、`browser-runtime/release/windows-updater-hooks.nsh`、`tests/browser/updater-lifecycle.spec.ts`。
- 修改：`src-tauri/src/updater.rs`、`src/hooks/useUpdater.ts`、D03 的 `src/hooks/useUpdater.test.tsx`、`src/lib/api/system.ts`、`src/lib/appUpdateHonesty.test.ts`、`src/lib/updateSim.test.ts`、`src-tauri/src/lib.rs`、`src-tauri/capabilities/default.json`、`src-tauri/Cargo.toml`、`src-tauri/Cargo.lock`。
- 修改：`.github/workflows/release.yml`、`scripts/assemble-updater-manifest.sh`、`scripts/publish-website-downloads.py`、`scripts/build-release-config.mjs`。
- 修改：`scripts/browser-runtime/build-bundle-config.mjs`、`scripts/browser-runtime/build-bundle-config.test.mjs`、`scripts/browser-runtime/run-package-matrix.mjs`、`scripts/browser-runtime/validate-package-evidence.mjs`、`src-tauri/tauri.windows.conf.json`、`browser-runtime/release/package-matrix.json`、十五语言 updater 领域目录。
- 修改：`docs/llm-wiki/release.md`、`docs/BUILD.md`。
- 测试：`src-tauri/src/updater/tests.rs`、`src/hooks/useUpdater.test.tsx`、`tests/browser/updater-lifecycle.spec.ts`、`scripts/browser-runtime/release-transaction.test.mjs`、`scripts/publish-website-downloads.py --self-test`。
**交付行为：** 实际客户端只安装 trusted index 对应完整 App/tuple，Browser 在 install 前独立 quiesce，失败边界保住 ACP/IM；Windows 有可检查交接，完整发布后才切 pointer。

- [ ] 定义 signed index/receipt 与 draft/uploaded/verified/published/indexed、客户端 checking/downloaded/quiesced/installed/handoffAccepted/failed 状态；Rust 与真实 hook 先写坏签名/sequence/expiry/404/错平台/错 hash/截断/portable、quiesce 失败、install 错误及 Windows spawn/ready 失败的调用顺序反例。
- [ ] 实现 Host app_update_check_trusted/download_trusted/apply_trusted/close，验证可信 index 的签名/sequence/expiry/目标完整资产与 clean-runner receipt，再用真实 UpdaterExt::updater_builder().endpoints(...).build()?.check().await；核对返回 Update，Update::download 验证 Tauri 签名后复验当前目标全部 bytes/hash/平台签名/tuple，ready 前不得 quiesce。
- [ ] 将 useUpdater.ts 改 opaque updateId + Host 事件，注册 command/API 并移除 JS updater check/download/install ACL 绕过；保持 About/后台只 ready、显式 Apply、sim/manual/i18n 行为。Apply 重验资格/期限/bytes、事务独占，再 R06 beginAppUpdateQuiesce；macOS Update::install 成功后 commit -> prepare_for_app_update -> AppHandle::restart，任何前置或 install 错误 abort 且不停止 ACP/IM、不恢复 Browser Agent。
- [ ] 实现 Windows verified NSIS exe adapter，以 ShellExecuteExW(SEE_MASK_NOCLOSEPROCESS) 检查启动/UAC 取消并持有进程；锁定 Tauri NSIS template 及 source/license，在 kill/uninstall/覆盖前完成无副作用 preflight，当前用户 ACL+nonce 的 ready/commit/abort event 握手；ready 前早退/超时 abort，ready 后 durable handoffAccepted -> Browser commit -> prepare_for_app_update -> signal commit -> AppHandle::exit，安装器等待已绑定旧 PID 退出才写文件并记录结果，禁止调用现插件 Windows install。
- [ ] 实现唯一发布 job：矩阵仅 staging/Draft、不可变命名，独立 clean runner 重新下载 required 全资产并验证签名/hash/最终 package evidence，完整才 publish -> signed versioned index -> updater pointer -> 网站别名；失败不改当前 pointer，保留 CHANGELOG/贡献者规则和幂等同 digest。
- [ ] 在本地 GitHub API fixture 演练所有发布中断，并以 D04 原生 harness 从重建签名包跑真实 updater 顺序、active Runtime/spool quiesce、macOS install 失败、Windows 拒绝/早退/超时/交接后故障及 portable；验证旧 ACP/IM 存活边界、Browser 人工恢复、无重复 install，生成 D06 证据并更新 release.md，未取得正式发布授权不调用真实 publish。

```js
assert.equal(decideRelease({ required: 4, verified: 3, blockers: 0 }), 'keepDraft');
assert.equal(decideRelease({ required: 4, verified: 4, blockers: 1 }), 'keepDraft');
assert.equal(decideRelease({ required: 4, verified: 4, blockers: 0 }), 'publish');
```

真实调用顺序与失败断言（Rust mock adapter + hook + 最终包三层验证，不能只测独立 fixture）：

```text
checkTrusted -> verifyIndex -> Tauri.check -> matchMetadata
downloadTrusted -> Tauri.download -> verifyPayload -> ready
Apply -> revalidate -> beginBrowserQuiesce -> descendantsZero -> purgeSpools
  macOS -> Update.install succeeds -> commit -> prepare_for_app_update -> restart
  Windows -> checkedSpawn -> ready -> persist handoffAccepted -> commit
          -> prepare_for_app_update -> installerCommit -> exit -> installerResult
pre-success/acceptance failure -> abort; ACP/IM stopCalls=0; Agent resumeCalls=0
```

现有 `tauri-plugin-updater 2.10.1` Windows install 在 on_before_exit 后忽略 ShellExecuteW 结果并直接 exit；计划的 Windows adapter 使用真实可检查 API，不能假定 JS await 会恢复执行。
`NSIS_HOOK_PREINSTALL` 单独不足以证明早期没有 kill/uninstall：D05 校验锁定 template 的全部执行顺序，所有破坏操作置于 commit + 旧 PID exit 之后，普通手动安装分支保持可用。
交接前拒绝/失败不关闭 ACP/IM；handoffAccepted 后 installer/磁盘失败只保证 durable failed/unknown、下次启动/人工修复，不承诺已退出旧进程仍存活。UI 的 installing/restarting 不得提前宣称安装成功。

运行：`node --test scripts/browser-runtime/release-transaction.test.mjs`。
运行：`cargo test --manifest-path src-tauri/Cargo.toml updater`、`pnpm exec vitest run src/hooks/useUpdater.test.tsx src/lib/appUpdateHonesty.test.ts src/lib/updateSim.test.ts`。
运行：`pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/updater-lifecycle.spec.ts`；签名原生包另运行 `node scripts/browser-runtime/run-package-matrix.mjs --native --repeat 3 --suite updater --evidence docs/qa/browser-rearchitecture/m1/delivery/updater`，本任务添加 suite 过滤并使聚合器要求该结果。
运行：`node scripts/browser-runtime/release-transaction.mjs --simulate-all-failures --evidence docs/qa/browser-rearchitecture/m1/delivery/release-transaction`。
运行：`python3 scripts/publish-website-downloads.py --self-test`。
**失败处理：** 资产验证失败保持 Draft/current；Browser quiesce/install/交接前失败 abort 后只允许人工恢复，零 ACP/IM teardown；交接后未知结果人工修复，不自动重复安装。
**验收：** 真实 updater/API/ACL/hook 与 Windows template 均已测试，publish 前零 pointer 更新；安装早于 Browser exit proof/spool purge 的调用数为零，Release 正文来自 CHANGELOG。

### M1-D06：回滚演练、发布环与运维交接

**依赖：** M1-D05、M1-G06、M1-R06。
**负责：** 发布负责人、QA 负责人、安全负责人。
**估算：** 4-5 工程日，另需两段24-48小时观察窗口。
**文件：**
- 创建：`scripts/browser-runtime/validate-release-gate.mjs`、`scripts/browser-runtime/validate-release-gate.test.mjs`。
- 创建：`browser-runtime/release/cohort-gates.json`、`docs/qa/browser-rearchitecture/m1/delivery/README.md`、`docs/qa/browser-rearchitecture/m1/delivery/operations.md`。
- 修改：`docs/llm-wiki/release.md`、`docs/llm-wiki/media-delivery.md`、`docs/BUILD.md`。
- 测试：`scripts/browser-runtime/validate-release-gate.test.mjs`、`scripts/browser-runtime/validate-release-gate.mjs`。
**交付行为：** 每平台能回答可否升环、如何stop_all/rollback、何时保持Preview且保留数据。

- [ ] 定义每platform/runtime build分母：100sessions、1000正常动作、30冷启、10crash/cleanup、5upgrade/rollback，两观察窗均满足才可建议升环。
- [ ] 写不足样本、单例安全越权、orphan、Profile串用、数据丢失和secret泄漏的阻断测试；beta百分比不能覆盖确定性门失败。
- [ ] 在真实包演练更高sequence授权旧tuple、revocation floor、兼容Profile generation、一次LKG与无LKG；不降低sequence，不wipe/merge Profile。
- [ ] 演练 block_new 有界 drain、fresh stop_all fence -> Runtime exit proof -> upload spool purge，另验 pendingBytes/满预算/用户停止清理、离线过期 bootstrap、时钟异常与 pointer 恢复，记录 Preview/人工恢复路径。
- [ ] 汇总三平台最终签名包及 D05 实际客户端 updater evidence：可信 index、portable manual、install 前 Browser quiesce、ACP/IM 存活边界、Windows handoffAccepted 后失败恢复；风险/unsupported 和 metadata 脱敏完整，无 telemetry 时仅计算记录完整的 dogfood 分母。
- [ ] 更新发布runbook、CHANGELOG短句/贡献者刷新操作，输出逐平台opt-in/hold结论；维护者批准后才升环，关闭能力不影响Preview。

```js
const gate = evaluateCohort({
  windowsPassed: 2, sessions: 100, actions: 1000, coldStarts: 30,
  cleanupDrills: 10, rollbacks: 5, safetyIncidents: 1,
});
assert.equal(gate.advance, false);
assert.equal(gate.reasonCode, 'safety_incident');
```

运行：`node --test scripts/browser-runtime/validate-release-gate.test.mjs`。
运行：`node scripts/browser-runtime/validate-release-gate.mjs --evidence docs/qa/browser-rearchitecture/m1/delivery --require-two-windows`。
运行：`pnpm typecheck`、`pnpm test`、`pnpm lint`、`pnpm build:ui`与既有Rust fmt/clippy/test及quality gate。
**失败处理：** 分母不足延长观察；安全事故停止升环并用授权签名策略阻断，保留Profile与恢复证据。
**验收：** 后续M2-M6只引用本门的实际证据，当前文档不宣称已发布或已获beta批准。

## 实施后检查

- [ ] 当前所有未运行命令与证据保持未勾选，单元测试通过不替代最终包运行。
- [ ] M1无独立Runtime downloader，portable只手动更新，community/Linux保持Preview/manual。
- [ ] 原始Trace、secret、机器私有路径和签名材料未进入公开evidence或support bundle。
- [ ] 发布规则只改新版本CHANGELOG，完成贡献者刷新且不绕过docs/llm-wiki/release.md。
