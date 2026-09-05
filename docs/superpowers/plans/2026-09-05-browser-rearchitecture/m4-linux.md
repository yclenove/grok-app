# M4 Linux 功能对等实现计划

> **面向 AI 代理的工作者：** 使用 subagent-driven-development 或 executing-plans 按任务执行；所有步骤以未勾选状态交接。本文只规划，开发、实验、提交与发布均未执行。

**目标：** 在明确支持的 Linux 桌面与 AppImage/deb/rpm 上交付经过安全证明的 Managed 能力。
**架构：** 扩展 M1 Host/Runtime 的 Linux platform adapter，复用统一 Gateway 和 worker。输入归因、sandbox、credential-at-rest 任一准入失败，Host 保留 Preview 并拒绝不安全能力。
**技术栈：** Rust/Tauri、系统 OpenSSH、Chromium/Playwright 固定 tuple、XInput2、Secret Service、Linux 原生桌面测试。
**规格：** [M4 设计](../../specs/2026-09-05-browser-rearchitecture/m4-linux.md)、[共用合同](../../specs/2026-09-05-browser-rearchitecture/00-contracts.md)。

## 当前证据与文件职责

`.github/workflows/ci.yml` 已有 Linux Rust job；`.github/workflows/release.yml` 在 Ubuntu 22.04 构建三种 Linux 包。
`package.json` 的 `build:linux` 已调用 `scripts/build-local.sh`；这些不是 Managed 运行证据。
`src-tauri/src/secrets.rs` 有 Secret Service 与明文文件回退，不能直接授权浏览器 persistent Profile。
`docs/BUILD.md` 已记录 AppImage bundled WebKit 的 AMD/Hyprland EGL 问题。
`src-tauri/src/updater.rs` 只允许 AppImage 走 Linux App 自动更新，deb/rpm 为手动。

| 路径 | 职责/来源 |
| --- | --- |
| `src-tauri/src/browser/platform/linux/mod.rs` | 新建，Linux adapter 装配 |
| `src-tauri/src/browser/platform/linux/capabilities.rs` | 新建，环境报告与能力准入 |
| `src-tauri/src/browser/platform/linux/input_fence.rs` | 新建，输入归因与健康检测 |
| `src-tauri/src/browser/platform/linux/sandbox.rs` | 新建，沙箱和进程身份 probe |
| `src-tauri/src/browser/platform/linux/credentials.rs` | 新建，浏览器凭据存储验证 |
| `src-tauri/src/browser/runtime/profile_guard.rs` | M1-R 前置产物，增加 Linux 监管实现 |
| `src/components/browser/platform/LinuxCapabilityNotice.tsx` | 新建，能力限制与修复交互 |
| `scripts/browser/verify-linux-package.mjs` | 新建，最终包验证 |
| `tests/browser/linux/` | 新建，逐任务列明测试文件 |
| `tests/browser/playwright.config.ts` | M1-D04 前置产物，复用确定性 fixture runner |
| `tests/browser/harness/native-driver.ts` | M1-D04 前置产物，现有 macOS/Windows 调度，增加 Linux adapter |
| `tests/browser/native/linux/driver.py`、`tests/browser/native/linux/README.md` | 新建，AT-SPI2 外置 App UI 驱动与依赖/权限说明 |
| `scripts/browser/run-linux-native.mjs` | 新建，启动 native driver、对照真实包身份、汇总原生证据 |

所有命令从仓库根执行；Rust 使用 `--manifest-path`。桌面/安装包命令必须在指定 Linux VM 或真机运行。
任务引用的 M1 文件由前置任务创建，不能把它们当作当前已有实现。
证据中的硬件名、路径和账户须脱敏；发布 flag 默认关闭。
执行依赖固定为 M4-01 -> M4-03 -> M4-04 -> M4-02 -> M4-05 -> M4-06。
纯模型 fixture 可提前并行准备；实际输入实验必须等待 Linux Runtime 监管和 Profile 准入完成。

### M4-01：建立 Linux 能力探测与支持矩阵

**依赖：** M1-G06、M1-R06、M1-D06。
**负责：** Linux 平台工程师、QA。
**估算：** 4-6 工程日，包含 Linux 原生 driver 与 Wayland 截图/无障碍可行性。
**文件：** 新建 `src-tauri/src/browser/platform/mod.rs`、`src-tauri/src/browser/platform/linux/mod.rs`、`src-tauri/src/browser/platform/linux/capabilities.rs`；修改前置 `src-tauri/src/browser/mod.rs`、`tests/browser/harness/native-driver.ts`；新建 `tests/browser/linux/platform-matrix.json`、`scripts/browser/probe-linux.mjs`、`scripts/browser/run-linux-native.mjs`、`tests/browser/native/linux/driver.py`、`tests/browser/native/linux/README.md`、`tests/browser/linux/native-driver.spec.ts`；测试 `src-tauri/src/browser/platform/linux/capabilities_tests.rs`。

- [ ] 1. 固定 Ubuntu 22.04/24.04、Debian 12、Fedora 44 的 package/display/credential 矩阵；在测试 VM 安装 `python3-pyatspi`、`python3-gi`/AT-SPI2，记录 session bus、截图 portal 与权限。
- [ ] 2. 写 probe fixture，未知桌面、缺少 Secret Service、ABI 超线均不得产生 writer 能力。

```json
{"os":"ubuntu-24.04","package":"deb","display":"wayland","inputAttribution":"blocked","sandbox":"proven","credentialStorage":"encrypted","writerAllowed":false}
```

- [ ] 3. 实现 Host 报告与 Linux AT-SPI2 driver 的 startInstalledApp/clickControl/readStatus/stopApp；复用 M1 native-driver 接口，核对真实 App PID/窗口，Wayland 截图须有授权 portal 证据。

```rust
let writer_allowed = report.input_attribution == Proven
    && report.sandbox == Proven
    && profile_storage_allowed;
```

- [ ] 4. 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::platform::linux::capabilities_tests`；预期所有缺失/未知 fixture 拒绝，支持报告通过。
- [ ] 5. 运行 `node scripts/browser/probe-linux.mjs --matrix tests/browser/linux/platform-matrix.json --out docs/qa/browser-rearchitecture/m4/M4-01/probe.json` 和 `node scripts/browser/run-linux-native.mjs --matrix tests/browser/linux/platform-matrix.json --scenario driver-smoke --out docs/qa/browser-rearchitecture/m4/M4-01/native.json`；预期启动实际包并读取/操作 App 控件。
- [ ] 6. 保存 native-driver 类型、包 digest、实际窗口截图和准入报告；普通 Playwright 网页通过不得计 App UI PASS，权限/树/驱动不可用记 blocked，probe 不修改系统安全策略。

**验收/证据：** `docs/qa/browser-rearchitecture/m4/M4-01/`，目前未执行；所有 writer 结论均能追溯到探测项。

### M4-02：验证输入归因 Provider 并作 Go/No-go

**依赖：** M4-04。
**负责：** 原生输入工程师、安全复核、QA。
**估算：** 4-7 工程日；其中原生 Wayland 可行性实验独立计时并允许 No-go。
**文件：** 新建 `src-tauri/src/browser/platform/linux/input_fence.rs`、`src-tauri/src/browser/platform/linux/input_fence_tests.rs`；修改前置 `src-tauri/src/browser/runtime/input_fence.rs`；新建 `tests/browser/linux/input-attribution.spec.ts`、`scripts/browser/run-linux-input-probe.mjs`。

- [ ] 1. 先写外部输入、自动化输入、焦点切换、窗口复用、显示服务器断开 fixture；窗口标题和 `_NET_WM_PID` 单独匹配必须失败。
- [ ] 2. 实现 XInput2 候选 Provider，将原生事件映射到已登记 PID/startTime 与窗口集合，复用 M1 ControlDomain fence。

```ts
const cases = [
  { source: "gateway-cdp", expectedFenceDelta: 0 },
  { source: "external-key", expectedFenceDelta: 1 },
  { source: "provider-disconnected", expectedState: "blocked_input_attribution" },
];
```

- [ ] 3. 仅在 M4-03 监管和 M4-04 Profile 准入通过的实际 Runtime 上，用 M4-01 native driver 编排 GNOME Wayland、XWayland 和 KDE 实验；缺合格 API 时写 No-go，不能只驱动普通网页过门。
- [ ] 4. 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::platform::linux::input_fence_tests`；预期旧 generation、映射冲突、Provider 失效均撤销 lease。
- [ ] 5. 运行 `node scripts/browser/run-linux-input-probe.mjs --matrix tests/browser/linux/platform-matrix.json --external-events 200 --gateway-actions 200 --faults 30 --out docs/qa/browser-rearchitecture/m4/M4-02/input.json`；报告漏 fence、自触发及 Host 到 ACK 延迟。
- [ ] 6. 形成 `docs/qa/browser-rearchitecture/m4/M4-02/decision.md`；只有零漏 fence、零自动化自触发且 p95 <= 250 ms 的声明环境为 Go。

**验收/证据：** M4-02 不以“窗口打开”通过；No-go 平台不授予 writer，不宣称 Linux 完整对等。目前未执行。

### M4-03：实现 sandbox 验证与 Linux 进程清理

**依赖：** M4-01。
**负责：** Rust Runtime 工程师、安全工程师。
**估算：** 4-6 工程日，包含 Linux 候选 tuple、guard 和默认系统沙箱证明。
**文件：** 新建 `src-tauri/src/browser/platform/linux/sandbox.rs`、`src-tauri/src/browser/platform/linux/sandbox_tests.rs`；修改前置 `src-tauri/src/browser/runtime/profile_guard.rs`、`browser-runtime/runtime-lock.json`、`scripts/browser-runtime/build-tuple.mjs`；新建 `tests/browser/linux/sandbox-cleanup.spec.ts`、`scripts/browser/probe-linux-descendants.mjs`。

- [ ] 1. 添加 userns 被禁、AppArmor 拒绝、seccomp 异常、renderer 假 PID、Host EOF 的失败 fixture。
- [ ] 2. 在现有 tuple builder 增加固定 Linux x64 Node/Chromium/helper 与 ABI/license/hash；生成 verified 候选后检查 renderer 的 `/proc`/namespace，绑定 PID/startTime/executable ledger，宿主与 renderer 分别断言。

```json
{"role":"renderer","requires":{"noNewPrivs":true,"seccompFilter":true,"sandboxProbe":"pass"},"forbiddenArgs":["--no-sandbox","--disable-setuid-sandbox"]}
```

- [ ] 3. 将 Linux TERM/KILL/reap 接入 guard；优先用 pidfd 验证身份，保留 run nonce ledger，cleanup_failed 时不释放 Profile lock。
- [ ] 4. 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::platform::linux::sandbox_tests`；预期任何安全 probe 失败均关闭 Managed。
- [ ] 5. 运行 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/linux/sandbox-cleanup.spec.ts --repeat-each=3`；测试并存用户 Chrome、连续 kill Host/worker/Chromium、sleep/wake。
- [ ] 6. 运行 `node scripts/browser/probe-linux-descendants.mjs --ledger docs/qa/browser-rearchitecture/m4/M4-03/ledger.json --after-seconds 30`；预期本 run descendants=0，外部 Chrome 仍活跃；记录低内核特性环境的明确拒绝。

**验收/证据：** `docs/qa/browser-rearchitecture/m4/M4-03/`，目前未执行；零 broad kill、零提前解锁、零禁用 sandbox 参数。

### M4-04：验证浏览器凭据静态加密与 Profile 恢复

**依赖：** M4-03。
**负责：** Profile 工程师、Linux 安全 QA。
**估算：** 3-4 工程日。
**文件：** 新建 `src-tauri/src/browser/platform/linux/credentials.rs`、`src-tauri/src/browser/platform/linux/credentials_tests.rs`；修改前置 `src-tauri/src/browser/profiles/store.rs`；新建 `tests/browser/linux/credentials.spec.ts`、`tests/browser/linux/credential-fixture.json`。

- [ ] 1. 写 Secret Service 正常/锁定/缺失、D-Bus 中断、Chromium basic 回退 fixture，测试 persistent 不接受未证明的加密。

```json
{"cookie":{"name":"fixture_session","value":"synthetic-linux-cookie-3847"},"secretService":"locked","profileMode":"persistent","expected":"credential_store_locked"}
```

- [ ] 2. 使用 disposable Profile 写入合成 Cookie，再验证 OS crypt 密钥来源、数据库非明文及正常重启可恢复；避免使用用户实际 Cookie。
- [ ] 3. 实现独立 Browser 存储准入，锁定/无存储拒绝 persistent；ephemeral 仅用经验证私有 tmpfs，缺失时拒绝，不套用 `secrets.rs` 明文回退。

```rust
match (profile_mode, credential_probe, private_tmpfs) {
    (Persistent, Encrypted, _) | (Ephemeral, _, Available) => allow(),
    _ => deny("credential_store_unavailable"),
}
```

- [ ] 4. 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::platform::linux::credentials_tests`；预期 unsafe parent、跨 UID、basic fallback 均拒绝。
- [ ] 5. 运行 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/linux/credentials.spec.ts --repeat-each=3`；验证 epoch 不兼容、正常重启、解锁重试与原 Profile 不变。
- [ ] 6. 生成不含真实凭据的 `docs/qa/browser-rearchitecture/m4/M4-04/storage.json`；验收写盘前准入、加密证据与恢复三者齐全。

**验收/证据：** `docs/qa/browser-rearchitecture/m4/M4-04/`，目前未执行；不能只用字符串扫描证明加密。

### M4-05：完成三种 Linux 包与能力限制交互

**依赖：** M4-02。
**负责：** 发布工程师、前端工程师、i18n 维护者。
**估算：** 3-5 工程日。
**文件：** 修改 `.github/workflows/release.yml`、`scripts/build-local.sh`、`docs/BUILD.md`；新建 `scripts/browser/verify-linux-package.mjs`、`src/components/browser/platform/LinuxCapabilityNotice.tsx`、`src/components/browser/platform/LinuxCapabilityNotice.test.tsx`；修改 `src/lib/settingsCatalog/entries/runtime.ts` 及下列语言文件。

语言文件：`src/i18n/messages/en/workspace.ts`、`src/i18n/messages/zh/workspace.ts`、`src/i18n/messages/zh-TW/workspace.ts`、`src/i18n/messages/de/workspace.ts`、`src/i18n/messages/es/workspace.ts`、`src/i18n/messages/fil/workspace.ts`、`src/i18n/messages/fr/workspace.ts`、`src/i18n/messages/id/workspace.ts`、`src/i18n/messages/it/workspace.ts`、`src/i18n/messages/ja/workspace.ts`、`src/i18n/messages/ko/workspace.ts`、`src/i18n/messages/pt-BR/workspace.ts`、`src/i18n/messages/ru/workspace.ts`、`src/i18n/messages/ta/workspace.ts`、`src/i18n/messages/uk/workspace.ts`。

- [ ] 1. 为 verifier 加缺 Runtime、ELF ABI 超线、非法权限、漏签名/manifest fixture；每种包都必须验证实际提取结果。
- [ ] 2. 扩展当前打包步骤放入完整 tuple，使用 Draft 与 clean-runner 验证链；保持 Ubuntu 22.04 基线。

```json
{"targets":["AppImage","deb","rpm"],"arch":"x86_64","maxRequiredGlibc":"2.35","requireRuntimeManifest":true,"requirePackageSignature":true}
```

- [ ] 3. 增加 Host 投影的 blocked/busy/retry/Preview 状态；复用 App 控件、完整 i18n 与设置搜索 `runtime.browser.linuxDiagnostics`，不允许绕过按钮。
- [ ] 4. 运行 `pnpm test -- src/components/browser/platform/LinuxCapabilityNotice.test.tsx src/i18n/messages.test.ts src/lib/settingsCatalog.test.ts` 和 `pnpm typecheck`；预期所有状态有明确交互且 catalog keys 对齐。
- [ ] 5. 在 Linux 构建机运行 `pnpm build:linux`，再运行 `node scripts/browser/verify-linux-package.mjs --bundle-dir src-tauri/target/x86_64-unknown-linux-gnu/release/bundle --matrix tests/browser/linux/platform-matrix.json --out docs/qa/browser-rearchitecture/m4/M4-05/packages.json`；预期三包齐全且逐文件通过。
- [ ] 6. 安装三种最终包，核对 AppImage FUSE/EGL、deb/rpm 系统 WebKit、离线有效/过期 policy、非 ASCII 路径与卸载保留数据；修订支持矩阵仅列真实通过行。

**验收/证据：** `docs/qa/browser-rearchitecture/m4/M4-05/`，目前未执行；未签社区包 Preview/manual，不伪装 Managed。

### M4-06：执行 Linux 全链路验收与发布准入

**依赖：** M4-05。
**负责：** 独立 QA、安全复核、发布负责人。
**估算：** 3-5 工程日，不含按样本不足延长的 dogfood 时间。
**文件：** 新建 `tests/browser/linux/golden-path.spec.ts`、`tests/browser/linux/package-lifecycle.spec.ts`、`scripts/browser/check-linux-gate.mjs`；修改 `.github/workflows/ci.yml`、`docs/llm-wiki/release.md`；新建 `docs/qa/browser-rearchitecture/m4/M4-06/acceptance.md`。

- [ ] 1. 创建最终包 golden path，覆盖原生接管、grant/fence、下载上传、Artifact、Profile、所有崩溃层次和人工恢复；M2 能力仅在其证据通过后加行。

```json
{"repeat":3,"require":{"unauthorizedAccess":0,"autoReplay":0,"profileMix":0,"orphansAt30s":0,"stalePrimitiveAfterFence":0},"minimumSamples":{"coldStart":30,"inputFence":30}}
```

- [ ] 2. 运行 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/linux/golden-path.spec.ts tests/browser/linux/package-lifecycle.spec.ts --repeat-each=3`，spec 必须经 M4-01 Linux native driver 操作实际安装 App；预期每支持行真实 UI/页面 oracle 三轮 100% 通过。
- [ ] 3. 在真实包执行 N-1 升级、兼容回滚、低磁盘、断电点、Secret Service 锁定、显示服务器/睡眠恢复；保存 tuple、Profile generation 与 ledger 证据。
- [ ] 4. 运行 `pnpm typecheck`、`pnpm test`、`pnpm lint`、`pnpm build:ui` 与 `python3 scripts/check-code-quality-gates.py --mode final`；Linux 执行 `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings` 和 `cargo test --manifest-path src-tauri/Cargo.toml`，预期全绿。
- [ ] 5. 运行 `node scripts/browser/check-linux-gate.mjs --evidence docs/qa/browser-rearchitecture/m4 --matrix tests/browser/linux/platform-matrix.json`；要求包 digest、三轮结果、native App PID/窗口/截图、Provider decision、加密和 sandbox，只有普通网页结果的行必须失败。
- [ ] 6. 签署 Go/No-go 与独立 Linux flag 升环记录；原生 Wayland 未过时写明“受限 beta，完整 M4 未通过”，不以 X11 成功覆盖它。

**验收/证据：** `docs/qa/browser-rearchitecture/m4/M4-06/`，目前未执行；出口为有证据的能力矩阵，跨平台支持不能由编译成功代替。
