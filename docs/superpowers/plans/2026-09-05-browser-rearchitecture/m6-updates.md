# M6 独立 Runtime 更新、优化与 CEF 决策实现计划

> **面向 AI 代理的工作者：** 使用 subagent-driven-development 或 executing-plans 逐任务实现；本文只规划，开发、故障注入、签名发布和 CEF 实验均未执行。

**目标：** 独立安全更新 immutable Runtime，并验证兼容回滚、Profile 迁移和后续引擎决策。
**架构：** Host 验证独立签名指令，下载至隔离 staging，canary 后原子切换 pointer；活跃 Runtime 固定 tuple 直到 drain。CEF 是隔离实验，生产仍使用 Preview/Managed。
**技术栈：** Rust、签名/规范 JSON/archive 库、React、Playwright 固定 tuple、原生签名工具、GitHub Actions Draft 发布链。
**规格：** [M6 设计](../../specs/2026-09-05-browser-rearchitecture/m6-updates.md)、[共用合同](../../specs/2026-09-05-browser-rearchitecture/00-contracts.md)。

## 当前证据与文件职责

`src-tauri/src/updater.rs` 是 App updater，install 成功后才停止 agent/IM/mirror 并 relaunch；Runtime 更新不能误用它。
`src-tauri/build.rs` 只包含 `GROK_UPDATER_*`；新组件 trust root、endpoint 和签名域独立。
`scripts/assemble-updater-manifest.sh` 生成 Tauri latest.json，与本计划的 releaseSequence/revocation 无关。
`.github/workflows/release.yml` 当前公开 App 包；M1-D06 必须先提供 Draft/clean-runner 安全基础。
M1 的 runtime/profiles/policy 模块是前置任务产物，不是本次调研已存在代码。

| 路径 | 职责/来源 |
| --- | --- |
| `src-tauri/src/browser/updates/mod.rs`、`manifest.rs` | 新建，组件更新装配与签名 schema |
| `src-tauri/src/browser/updates/store.rs` | 新建，M1 安全账本的组件 metadata adapter，不持有第二份最高值 |
| `src-tauri/src/browser/updates/installer.rs` | 新建，有界下载与安全提取 |
| `src-tauri/src/browser/updates/activation.rs` | 新建，委托 M1 runtime/activation 的组件更新编排 |
| `src-tauri/src/browser/updates/policy.rs` | 新建，组件与既有 Managed policy 接入 |
| `src-tauri/src/browser/runtime/distribution.rs`、`browser-runtime/release/distribution.schema.json` | M1-D01 前置，分开官方 App/embedded 初始 tuple/component 授权 |
| `src-tauri/src/browser/runtime/manifest.rs`、`supervisor.rs` | M1-R 前置，复用 tuple 验证和 guard |
| `src-tauri/src/browser/profiles/migration.rs` | M1-R 前置，扩展新 epoch 迁移 |
| `src/components/browser/updates/RuntimeUpdatePanel.tsx` | 新建，更新状态与修复交互 |
| `.github/workflows/browser-runtime-release.yml` | 新建，独立组件 Draft/sign/verify/publish/index |
| `experiments/browser-cef/` | 新建，隔离实验，具体文件在 M6-05 列出 |
| `tests/browser/playwright.config.ts` | M1-D04 前置 runner |

所有命令从仓库根执行；签名 fixture 使用专门测试 key，生产私钥只在授权 CI secret 环境使用。
序列是规范十进制 string，使用无符号任意精度比较；不能改成 JS Number 或 semver 比较。
所有证据目前 `not_run`；发布动作只在该任务被实施且获既有发布授权后执行。
优化可以引用 M2-06 的证据，但不引入对 M2/M3/M5 的强制依赖。

## 实施约束与覆盖关系

| 合同 | 负责任务 |
| --- | --- |
| 独立信任域、序列比较、共享安全 checkpoint | M6-01 |
| 下载预算、提取安全、逐文件与平台验签 | M6-02 |
| canary/pointer 复用、active tuple、Profile epoch | M6-03 |
| 离线 policy、完整交互、Draft 与 signed index | M6-04 |
| 容量数据、CEF 隔离实验与明确决策 | M6-05 |
| 最终包矩阵、故障回滚、独立发布门 | M6-06 |

M1 `runtime/activation.rs`、`runtime/sequence.rs`、`runtime/policy.rs` 是唯一激活/反重放/运行 policy 实现。
本包新增 updates 层只负责组件分发编排和 metadata adapter，不能复制一套 canary 或 lease 逻辑。
签名 fixture 在测试内用固定测试 seed 生成有效字节，不能拿描述性字符串冒充签名结果。

### M6-01：定义组件 manifest、独立 key 与安全账本

**依赖：** M1-R06、M1-D06。
**负责：** Runtime 安全工程师、发布工程师。
**估算：** 4-6 工程日，包含官方 distribution 授权扩展和同 App 新 tuple 接纳验证。
**文件：** 新建 `src-tauri/src/browser/updates/mod.rs`、`src-tauri/src/browser/updates/manifest.rs`、`src-tauri/src/browser/updates/store.rs`、`src-tauri/src/browser/updates/manifest_tests.rs`、`src-tauri/src/browser/updates/distribution_tests.rs`；新建 `browser-runtime/schemas/component-release.schema.json`、`tests/browser/updates/manifest-cases.json`；修改 `src-tauri/build.rs`、前置 `src-tauri/src/browser/mod.rs`、`src-tauri/src/browser/runtime/sequence.rs`、`src-tauri/src/browser/runtime/distribution.rs`、`browser-runtime/release/distribution.schema.json`、`scripts/browser-runtime/build-attestation.mjs`。

- [ ] 1. 升级 distribution schema，拆分 officialApp、embeddedTupleDigest 与 componentAuthorization（trust domain/key role、target、protocol/App 范围）；写同 App 合法新 tuple 及错误 key/target/protocol/unsigned App fixture，并保留重放/大整数测试。

```json
{"acceptedSequence":"9007199254740993","candidateSequence":"9007199254740992","signatureCase":"validTestKey","expected":"release_sequence_replay"}
```

```json
{"attestation":{"officialApp":{"distribution":"official","sourceCommit":"fixture-commit","appVersion":"1.2.3","target":"aarch64-apple-darwin","publisher":"fixture-publisher"},"embeddedTupleDigest":"tuple-a","componentAuthorization":{"trustDomain":"grok-browser-runtime-v1","keyRole":"runtime-release","targets":["aarch64-apple-darwin"],"protocolVersions":[1],"appVersionRange":">=1.2.0 <2.0.0"}},"candidate":{"tupleDigest":"tuple-b","signatureCase":"authorizedComponentKey","target":"aarch64-apple-darwin","protocolVersion":1},"expected":"accept_after_sequence_policy_and_compatibility_checks"}
```

- [ ] 2. 用成熟签名/规范 JSON 库验证 raw manifest；distribution 先证明官方 App 与组件授权，再精确验证候选 target/protocol/App/Profile 范围；sequence 拒绝负值、小数、前导零和超长输入。

```ts
function compareSequence(a: string, b: string): number {
  if (!/^(0|[1-9][0-9]{0,127})$/.test(a) || !/^(0|[1-9][0-9]{0,127})$/.test(b)) throw new Error("invalid_sequence");
  return a.length === b.length ? (a < b ? -1 : a > b ? 1 : 0) : Math.sign(a.length - b.length);
}
```

- [ ] 3. 扩展 M1 同一原子 security checkpoint，记录最高 release/policy/root sequence、digest、revocation floor；embedded/component 共用 Runtime 最高值，损坏或旧 schema 不重置为 embedded 默认值。
- [ ] 4. 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::updates::manifest_tests` 与 `cargo test --manifest-path src-tauri/Cargo.toml browser::updates::distribution_tests`；预期同 App 接纳合法不同 digest，拒绝错误信任域/target/协议/兼容区间/社区伪装，重放拒绝且重复幂等。
- [ ] 5. 加更高 sequence 授权 rollback fixture，证明 target 不低于 floor 且最高值不下降；对账本每个写点注入失败再重启。
- [ ] 6. 保存 schema/test vector/恢复矩阵与测试 key 指纹，公开证据不得含生产 secret。

**验收/证据：** `docs/qa/browser-rearchitecture/m6/M6-01/`，目前未执行；数值超过 JS 安全整数仍准确比较。

### M6-02：实现有界下载、安全提取与最终文件验证

**依赖：** M6-01。
**负责：** Runtime 安装器工程师、安全 QA。
**估算：** 3-5 工程日。
**文件：** 新建 `src-tauri/src/browser/updates/installer.rs`、`src-tauri/src/browser/updates/installer_tests.rs`、`tests/browser/updates/archive-fixtures.json`、`scripts/browser/make-update-fixtures.mjs`；修改前置 `src-tauri/src/browser/runtime/manifest.rs`。

- [ ] 1. 创建合成 archive generator，覆盖 traversal、absolute path、hardlink/device、Framework 合法/非法 symlink、Unicode/case 冲突、缺文件与超量展开。

```json
{"entries":[{"path":"Frameworks/Current","kind":"symlink","target":"../../../../outside"}],"expected":"archive_link_escape"}
```

- [ ] 2. 实现 streaming 下载到 transaction staging，先验证 archive size/hash/signature，再使用 no-follow structured archive reader 提取；HTTPS 重定向必须匹配签名允许域。
- [ ] 3. 在任何 helper 执行前核对完整 file manifest、hash、mode、架构和平台签名；本地硬上限 archive 2 GiB、expanded 8 GiB、100,000 entries。

```json
{"reserveBytesFormula":"expandedBytes * 2 + 536870912","preserve":["currentTuple","activeTuples","profileGenerations"],"runHelperBeforeVerification":false}
```

- [ ] 4. 运行 `node scripts/browser/make-update-fixtures.mjs --out tests/browser/updates/generated` 与 `cargo test --manifest-path src-tauri/Cargo.toml browser::updates::installer_tests`；预期恶意输入全拒绝，合法 Framework symlink 通过。
- [ ] 5. 注入低磁盘、断网、取消和 checksum mismatch，检查只删除本 transaction partial；当前 pointer/Profile 校验值不变。
- [ ] 6. 记录每个平台提取后 verifier 报告，确认错误发生前未执行 Node、guard 或 Chromium，缓存命中也重新核对 manifest。

**验收/证据：** `docs/qa/browser-rearchitecture/m6/M6-02/`，目前未执行；未验证 helper 执行次数必须为零。

### M6-03：实现 canary、活跃 tuple 固定与 Profile 迁移

**依赖：** M6-01、M6-02。
**负责：** Runtime/Profile 工程师。
**估算：** 4-6 工程日。
**文件：** 新建 `src-tauri/src/browser/updates/activation.rs`、`src-tauri/src/browser/updates/activation_tests.rs`、`tests/browser/updates/activation.spec.ts`、`tests/browser/updates/profile-epoch.spec.ts`；修改前置 `src-tauri/src/browser/profiles/migration.rs`、`src-tauri/src/browser/runtime/supervisor.rs`、`src-tauri/src/browser/runtime/activation.rs`。

- [ ] 1. 写 App N-1/N、Runtime N-1/N、Profile epoch 1/2 与 active refcount fixture，旧版本不能原地打开新 epoch。

```json
{"active":{"tuple":"tuple-a","profileGeneration":4,"profileEpoch":1},"candidate":{"tuple":"tuple-b","profileEpoch":2},"expected":{"activeTuple":"tuple-a","newProfileGeneration":5,"originalPreserved":true}}
```

- [ ] 2. 委托 M1 activation 的 disposable Profile canary：initialize、空白页截图、close、descendants=0 后才能 staged；失败 quarantine，最多一次合格 LKG 恢复。
- [ ] 3. 用 pointer journal/security checkpoint 切换 current；活跃 tuple 保持 immutable。回收严格 refcount=0 -> close admission/drain -> descendants=0 -> unlock/GC，drain 失败保留 lock/quarantine。
- [ ] 4. 在 OS Profile lock 可取得且用户显式同意时迁移到新 generation；保留原始 checksum 与登录态恢复证据，不 wipe/merge，失败返回可选兼容 Profile。
- [ ] 5. 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::updates::activation_tests` 和 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/updates/activation.spec.ts tests/browser/updates/profile-epoch.spec.ts --repeat-each=3`；预期 active tuple 不变、并发 writer profile_busy。
- [ ] 6. 在 pointer 前后、migration copy/commit、refcount 归零、drain/guard close 注入至少 50 个断点；断言 descendants>0 时 lock/tuple 仍保留，恢复完整 generation 且无旧请求 replay。

**验收/证据：** `docs/qa/browser-rearchitecture/m6/M6-03/`，目前未执行；LKG 不降低最高 sequence，也不打开不兼容 Profile。

### M6-04：接入 policy、完整更新视图与独立发布流程

**依赖：** M6-01、M6-02、M6-03。
**负责：** Host/前端工程师、发布负责人、i18n 维护者。
**估算：** 4-6 工程日。
**文件：** 新建 `src-tauri/src/browser/updates/policy.rs`、`src-tauri/src/browser/updates/policy_tests.rs`、`src/components/browser/updates/RuntimeUpdatePanel.tsx`、`src/components/browser/updates/RuntimeUpdatePanel.test.tsx`、`.github/workflows/browser-runtime-release.yml`、`scripts/browser/publish-runtime-index.mjs`、`tests/browser/updates/offline-policy.spec.ts`；修改 `src/lib/settingsCatalog/entries/runtime.ts`、`docs/llm-wiki/release.md` 和下列语言文件。

语言文件：`src/i18n/messages/en/workspace.ts`、`src/i18n/messages/zh/workspace.ts`、`src/i18n/messages/zh-TW/workspace.ts`、`src/i18n/messages/de/workspace.ts`、`src/i18n/messages/es/workspace.ts`、`src/i18n/messages/fil/workspace.ts`、`src/i18n/messages/fr/workspace.ts`、`src/i18n/messages/id/workspace.ts`、`src/i18n/messages/it/workspace.ts`、`src/i18n/messages/ja/workspace.ts`、`src/i18n/messages/ko/workspace.ts`、`src/i18n/messages/pt-BR/workspace.ts`、`src/i18n/messages/ru/workspace.ts`、`src/i18n/messages/ta/workspace.ts`、`src/i18n/messages/uk/workspace.ts`。

- [ ] 1. 写有效/过期 bootstrap/cache、clock rollback、block_new、stop_all、revoked tuple/key fixture，复用 M1 admission 与 worker fence。

```json
{"network":"offline","cache":"expired","bootstrap":"expired","expected":{"managedAdmission":"block_new","preview":"available","forceKillActive":false}}
```

- [ ] 2. 实现检查/下载/取消/校验/可用于新会话/等待活跃任务/失败修复视图；新设置注册 `runtime.browser.runtimeUpdates`，没有任意旧 tuple/绕过 policy 控件。
- [ ] 3. 建独立 workflow：签名不可变资产 -> Draft 上传 -> clean runner 重下载校验 -> publish -> signed index；403/404/缺包/错 hash 不推进 index。
- [ ] 4. 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::updates::policy_tests`、`pnpm test -- src/components/browser/updates/RuntimeUpdatePanel.test.tsx src/i18n/messages.test.ts src/lib/settingsCatalog.test.ts`；预期所有离线状态明确且不泄漏内部 secret。
- [ ] 5. 运行 `node scripts/browser/publish-runtime-index.mjs --self-test` 与 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/updates/offline-policy.spec.ts --repeat-each=3`；预期半发布保持旧 pointer，stop_all fence 并标记未知副作用。
- [ ] 6. 在 staging release 演练 key rotation、撤销与更高 sequence rollback；更新发布 wiki，证明组件 activation 不调用 `prepare_for_app_update` 或重启 ACP。

**验收/证据：** `docs/qa/browser-rearchitecture/m6/M6-04/`，目前未执行；现有 App 更新与 Runtime 更新具备独立状态/签名/协议。

### M6-05：完成容量优化与独立 CEF 实验决策

**依赖：** M6-03。
**负责：** Runtime 性能工程师、原生平台工程师、无障碍 QA。
**估算：** 4-7 工程日；CEF 未通过安全/平台证明可明确 No-go。
**文件：** 新建 `src-tauri/src/browser/runtime/capacity.rs`、`src-tauri/src/browser/runtime/capacity_tests.rs`、`tests/browser/updates/capacity.spec.ts`、`scripts/browser/benchmark-runtime.mjs`；新建 `experiments/browser-cef/README.md`、`experiments/browser-cef/CMakeLists.txt`、`experiments/browser-cef/src/main.cc`、`experiments/browser-cef/scripts/run-comparison.mjs`。

- [ ] 1. 固定 M1 fixture/hardware/tuple 与计时边界，采集至少 30 次 cold/warm/RSS/CPU/恢复；M2 数据可引用但不是依赖。
- [ ] 2. 实现 idle drain/精确同 Profile 复用，保留默认最多两个 Runtime；refcount=0 后 drain，descendants=0 才 unlock/GC，lease/Artifact 写入未清零不回收，活跃任务不驱逐。

```json
{"runtimeLimit":2,"activeProfiles":["profile-a","profile-b"],"newProfile":"profile-c","expected":"runtime_capacity","evictActive":false}
```

- [ ] 3. 在隔离 experiments 构建最小 CEF 任务窗口，重用确定性 fixture 比较输入接管、sandbox、可访问性、签名、升级与维护工时；不接生产路由。
- [ ] 4. 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::runtime::capacity_tests`、`pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/updates/capacity.spec.ts --repeat-each=3`；预期无串 Profile/grant、无静默驱逐。
- [ ] 5. 运行 `node scripts/browser/benchmark-runtime.mjs --samples 30 --out docs/qa/browser-rearchitecture/m6/M6-05/managed.json` 与 `node experiments/browser-cef/scripts/run-comparison.mjs --samples 30 --baseline docs/qa/browser-rearchitecture/m6/M6-05/managed.json --out docs/qa/browser-rearchitecture/m6/M6-05/cef.json`；报告原始样本和排除项。
- [ ] 6. 写 `docs/qa/browser-rearchitecture/m6/M6-05/cef-decision.md`；Go 要求至少一项性能改善 20%、其他指标回退不超 10%、全部安全/可访问性/发布门通过，否则 No-go；Go 只提出后续独立规格。

**验收/证据：** `docs/qa/browser-rearchitecture/m6/M6-05/`，目前未执行；CEF 结果不改变 Preview/Managed 的正式产品合同。

### M6-06：执行更新生命周期、回滚与发布最终验收

**依赖：** M6-01、M6-02、M6-03、M6-04、M6-05。
**负责：** 独立安全 QA、平台 QA、发布负责人。
**估算：** 3-5 工程日，不含样本不足的观察延长时间。
**文件：** 新建 `tests/browser/updates/package-lifecycle.spec.ts`、`tests/browser/updates/security-regression.spec.ts`、`scripts/browser/check-runtime-update-gate.mjs`、`docs/qa/browser-rearchitecture/m6/M6-06/acceptance.md`；修改 `.github/workflows/ci.yml`。

- [ ] 1. 构建最终 DMG arm64/x64、NSIS/portable 组合矩阵；Linux 只加入 M4 已放行行，记录 App/Runtime N-1/N 与 Profile epoch 前后。

```json
{"repeats":3,"minimumFaults":{"transactionPoints":50,"activeUpdates":30,"migrationRollback":30},"zeroTolerance":["unverifiedExecution","activeTupleMutation","profileLoss","replay","orphanAt30s"]}
```

- [ ] 2. 运行 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/updates/package-lifecycle.spec.ts tests/browser/updates/security-regression.spec.ts --repeat-each=3`；预期声明矩阵每行 100% 通过。
- [ ] 3. 实际包覆盖 offline policy、低磁盘、中断、坏 manifest/pointer、epoch migration、手工 App downgrade、key rotation/revocation、LKG 缺失与被撤销；Windows portable 路径搬动单独留证。
- [ ] 4. 运行 `pnpm typecheck`、`pnpm test`、`pnpm lint`、`pnpm build:ui`、`python3 scripts/check-code-quality-gates.py --mode final`；各平台运行 `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`、`cargo test --manifest-path src-tauri/Cargo.toml`，预期全绿。
- [ ] 5. 运行 `node scripts/browser/check-runtime-update-gate.mjs --evidence docs/qa/browser-rearchitecture/m6`；要求序列/签名/提取/active tuple/Profile/离线/CEF 决策完整，缺证据返回非零。
- [ ] 6. 签署独立 updater flag 的 Go/No-go 与 rollout/rollback 记录；持续满足总体设计样本门后升环，发现安全越权、数据丢失或 orphan 立即停止。

**验收/证据：** `docs/qa/browser-rearchitecture/m6/M6-06/`，目前未执行；M6 完成意味着组件更新可验证和 CEF 决策有证据，不意味着替换 WebView。
