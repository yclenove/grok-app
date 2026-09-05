# M1 Managed Runtime 实现计划

> **面向 AI 代理的工作者：** 使用 subagent-driven-development 或 executing-plans 逐任务实现。步骤使用未勾选复选框追踪；本文件记录未执行待办，当前文档交付不代表产品完成。实施期间的 commit/push 遵循总索引。

**目标：** 建立可验证启动、原生监管、可撤销执行、Profile 迁移和完整 Artifact/transfer 生命周期。
**架构：** Gateway 授权，Supervisor 选择固定 tuple/Profile，Guard 独立持锁；worker 只执行当前 generation/fence 下的受限 primitive。
**技术栈：** Rust/Tauri、atomic JSON、fs2、平台 Job/PGID、固定 Node/playwright-core、Vitest/Node tests。
**规格：** [M1 Runtime](../../specs/2026-09-05-browser-rearchitecture/m1-runtime.md)、[共用协议](../../specs/2026-09-05-browser-rearchitecture/00-contracts.md)。
**估算：** 29-41 工程日；最终包证据与 M1-D04 联合执行。

## 文件职责与当前证据

| 路径 | 当前证据与本计划职责 |
| --- | --- |
| `src-tauri/src/store_lock.rs` | 当前提供 write_bytes_atomic，复用 JSON durability；不新建 SQLite。 |
| `src-tauri/src/media_server.rs` | 当前 token 是进程级，本包增加独立 Artifact endpoint 验证。 |
| `src-tauri/src/side_browser_host.rs` | 当前 Preview pending download 按 URL，本包所有新下载按 downloadId。 |
| `src-tauri/src/lib.rs` | 只添加 Browser domain 注册/Exit 调用，不写 Runtime 大块实现。 |
| `src-tauri/src/browser/runtime/` | 延续 M0 tuple/Guard/input_fence；新增 activation、supervisor、rpc、recovery。 |
| `src-tauri/src/browser/profiles/` | 新建 UUID metadata、generation 与 migration。 |
| `src-tauri/src/browser/artifacts/` | 新建 immutable blob、token、owner reference、cleaner。 |
| `src-tauri/src/browser/transfers/` | 新建 Runtime-owned upload spool lease、download/export、purge reconciliation。 |
| `browser-runtime/worker/src/` | 延续 M0 worker，新增生产 adapter 和原语执行。 |
| `src/components/browser/runtime/` | 新建 Host 投影 adapter；不向 App.tsx/AppWorkbench 添加 state。 |

前置 M0-W01 创建 protocol.rs 与 TS wire/schema；本包不另造 RequestEnvelope。
M1-G01 创建 Gateway 对接点；权限、origin、binding/lease 决策归 Gateway，worker 不自行授权。
下面新增测试/脚本由所在任务创建，命令不是本轮已运行记录。
后续 evidence 根为 `docs/qa/browser-rearchitecture/m1/runtime/`。

### M1-R01：安全激活 Tuple、Sequence 与 LKG

**依赖：** M0-R06、M0-W01。
**负责：** Runtime 工程师、供应链工程师。
**估算：** 4-5 工程日。
**文件：**
- 修改：`src-tauri/src/browser/runtime/manifest.rs`、`src-tauri/src/browser/runtime/mod.rs`。
- 创建：`src-tauri/src/browser/runtime/activation.rs`、`src-tauri/src/browser/runtime/sequence.rs`、`src-tauri/src/browser/runtime/activation_tests.rs`。
- 创建：`browser-runtime/fixtures/activation-cases.json`、`scripts/browser-runtime/run-activation-drills.mjs`。
- 测试：`src-tauri/src/browser/runtime/activation_tests.rs`、`browser-runtime/fixtures/activation-cases.json`。
**交付行为：** 只有完整验证与 canary 成功才能原子切 current；旧签名 tuple 重放、半提取和撤销 LKG 全部拒绝。

- [ ] 定义 signed instruction、highestAcceptedSequence、revocationFloor、acceptedDigest 与 current/LKG descriptor；sequence 用规范十进制 string 作无符号比较，u32 generation overflow 关闭 admission。
- [ ] 写低 sequence、同 sequence 不同 digest、higher-sequence rollback、revoked LKG、坏 pointer、低磁盘和 signature failure 的失败测试。
- [ ] 从 M0 verifier 接安全提取与逐文件平台签名复验；精确 allowlisted Framework symlink，拒绝 hardlink、reparse escape、未声明文件与超限 payload。
- [ ] 实现 immutable 目录发布和 canary initialize/blank screenshot/close probe；descriptor/sequence fsync 成功后才切 current，所有 phase 支持 fault injection。
- [ ] 实现最多一次本地 LKG，要求有效签名策略允许且 App/Profile epoch 兼容；正式旧版本回滚只接受更高 sequence 精确授权，不降低 highest sequence。
- [ ] 运行 pointer 前后崩溃与同目录 active ref 测试，确保在用 tuple 不修改；quarantine 失败候选，向 UI 返回 repair/Preview reasonCode。

```rust
#[test]
fn same_sequence_cannot_change_payload() {
    let state = SequenceState::test(8, "digest-a", 4);
    assert!(state.accepts(8, "digest-a"));
    assert!(!state.accepts(8, "digest-b"));
    assert!(!state.accepts(7, "digest-a"));
}
```

运行：`cargo test --manifest-path src-tauri/Cargo.toml browser::runtime::activation_tests`。
运行：`node scripts/browser-runtime/run-activation-drills.mjs --all-faults --evidence docs/qa/browser-rearchitecture/m1/runtime/activation`。
预期：故障不改变当前健康 tuple；无授权 LKG 时 Managed unavailable，Preview 仍可用。
**失败处理：** durability 或签名验证失败即 fail closed，不尝试 PATH executable，不反复自动回滚。
**验收：** sequence 永不回退，旧 tuple 必须有当前策略授权，active generation 字节不变。

### M1-R02：生产 Supervisor 与 Guard 生命周期

**依赖：** M1-R01、M0-R04。
**负责：** Runtime 原生工程师。
**估算：** 4-6 工程日。
**文件：**
- 创建：`src-tauri/src/browser/runtime/supervisor.rs`、`src-tauri/src/browser/runtime/spawn.rs`、`src-tauri/src/browser/runtime/supervisor_tests.rs`。
- 修改：`src-tauri/src/browser/runtime/profile_guard.rs`、`src-tauri/src/browser/runtime/ledger.rs`、`src-tauri/src/bin/browser-profile-guard.rs`。
- 修改：`src-tauri/src/browser/runtime/containment_macos.rs`、`src-tauri/src/browser/runtime/containment_windows.rs`、`src-tauri/src/lib.rs`。
- 测试：`src-tauri/src/browser/runtime/supervisor_tests.rs`、`scripts/browser-runtime/run-guard-drills.mjs`。
**交付行为：** 同 Profile 单 Runtime、跨 Session 引用、两 Runtime 容量与异常退出全部由 Host/Guard 管理。

- [ ] 写相同 Profile 复用、第三个 Profile runtime_capacity、最后 ref 才 drain、第二 App claimant 拒绝的失败测试。
- [ ] 实现 Supervisor registry 与 generation counter，descriptor 固定 tuple/profile generation；target registry 不接受未分配 ownership 的 target。
- [ ] 将 M0 Guard 生产化：Windows suspended/Job/resume 原子顺序，macOS PGID/ledger 身份确认，Host EOF 后独立 TERM/KILL/reap；Host 监听 Guard 意外退出并持 reconciliation lock 清理，不使用 TTL 抢锁。
- [ ] 新建 spawn helper，以验证后绝对路径、env_clear、私有 cwd、必需 handle/env allowlist 启动；排除 API key/agent home/proxy secret，保持 Chromium sandbox。
- [ ] 接 Host Exit 与 Session ref close，先关闭 admission/fence 再 drain；停机失败保留 reconciliation_required，不能先报告 Runtime closed 再遗留进程；Session closed 与 Runtime-owned spool cleanupPending 按 R05/R06 分别记录。
- [ ] 运行双 App、用户 Chrome 同开、Guard/Host crash 与容量压力，断言 30 秒 descendants=0 且其他 Chrome 不变。

```rust
#[test]
fn shared_runtime_stops_only_after_last_ref() {
    let mut refs = RuntimeReferences::default();
    refs.attach("session-a"); refs.attach("session-b");
    assert!(!refs.detach("session-a").should_drain());
    assert!(refs.detach("session-b").should_drain());
}
```

运行：`cargo test --manifest-path src-tauri/Cargo.toml browser::runtime::supervisor_tests`。
运行：`node scripts/browser-runtime/run-guard-drills.mjs --native --repeat 3 --evidence docs/qa/browser-rearchitecture/m1/runtime/guard`。
**失败处理：** containment 建立失败时 worker 不 initialize；身份不可证明时保留 Profile 忙状态。
**验收：** 隐藏 pane 不结束 Runtime，关闭最后 Session 完整停机；环境过滤不被宣称为 OS 文件隔离。

### M1-R03：Framed Worker 与逐 Primitive Fence

**依赖：** M1-R02、M1-G01、M0-R05。
**负责：** Playwright Runtime 工程师、Gateway 工程师。
**估算：** 5-7 工程日。
**文件：**
- 创建：`src-tauri/src/browser/runtime/rpc.rs`、`src-tauri/src/browser/runtime/managed_adapter.rs`、`src-tauri/src/browser/runtime/rpc_tests.rs`。
- 创建：`browser-runtime/worker/src/transport.ts`、`browser-runtime/worker/src/managed.ts`、`browser-runtime/worker/src/primitives.ts`、`browser-runtime/worker/src/primitives.test.ts`。
- 修改：`browser-runtime/worker/src/main.ts`、`browser-runtime/worker/src/fence.ts`、`browser-runtime/worker/vitest.config.ts`、`browser-runtime/worker/tsconfig.json`、`src-tauri/src/browser/runtime/input_fence.rs`。
- 测试：`src-tauri/src/browser/runtime/rpc_tests.rs`、`browser-runtime/worker/src/primitives.test.ts`。
**交付行为：** Gateway 请求映射声明 primitive，旧身份拒绝，接管封住后续动作并停止观察。

- [ ] 写分帧/超长帧/背压/初始化前请求、旧 boot/runtime/binding generation、过期 fence/lease 与乱序结果的失败测试。
- [ ] 实现 4 字节大端长度帧、1 MiB 上限、requestId 去重、32 条 tab 队列和 256 条 event 背压，stdout-only protocol 与脱敏 stderr ring；超限拒绝，大结果只能走 Artifact ingest。
- [ ] 实现 navigation、受限 locator/safe input、截图及 metadata primitive，Host 提供 frame/origin verdict，popup ownership 未登记前不观察。
- [ ] 对每 primitive 前、await 后、retry 前重新校验；采用 M0 证明的取消路径，必要 page/context/connection teardown，Promise.race 只做 deadline 不做取消证明。
- [ ] 绑定 Provider unhealthy/native input 到整个 Runtime fence，pause observer；旧队列不得在新 turn、重连或 explicit return 后复活，新 snapshot/new lease 重建控制。
- [ ] 跑 actionability barrier 与服务器计数，当前竞态 primitive 标 unknown_outcome，后续写入为零；未经 sanitizer 的原始 Playwright Trace与Full CDP保持unsupported。

```ts
export async function guardedPrimitive<T>(fence: Fence, action: () => Promise<T>) {
  fence.assertCurrent();
  const value = await action();
  fence.assertCurrent();
  return value;
}
// 取消在 dispatcher 的已验证 teardown 路径完成，不能由这两个检查代替。
```

运行：`pnpm --dir browser-runtime/worker test`、`cargo test --manifest-path src-tauri/Cargo.toml browser::runtime::rpc_tests`。
定向运行：`pnpm --dir browser-runtime/worker exec vitest run --config vitest.config.ts src/primitives.test.ts`；独立配置必须扫描 worker，不能依赖根 Vitest 的 `src/**` include。
运行：`node scripts/browser-runtime/run-fence-probes.mjs --native --all-primitives --evidence docs/qa/browser-rearchitecture/m1/runtime/fence`。
**失败处理：** 无法证明某 primitive 取消语义时从 capability 表移除，返回 unsupported，不扩大审批豁免。
**验收：** 旧 lease 零通过、observer fence 后零新增内容、真实窗口状态同步，runtimeGeneration 非 Managed 为 null。

### M1-R04：Profile Generation、迁移与保守恢复

**依赖：** M1-R02、M0-R05。
**负责：** 存储工程师、Runtime 工程师。
**估算：** 4-6 工程日。
**文件：**
- 创建：`src-tauri/src/browser/profiles/mod.rs`、`src-tauri/src/browser/profiles/store.rs`、`src-tauri/src/browser/profiles/migration.rs`、`src-tauri/src/browser/profiles/tests.rs`。
- 创建：`browser-runtime/fixtures/profile-migrations.json`、`scripts/browser-runtime/run-profile-drills.mjs`。
- 修改：`src-tauri/src/paths.rs`、`src-tauri/src/browser/runtime/supervisor.rs`、`src-tauri/src/browser/mod.rs`。
- 测试：`src-tauri/src/browser/profiles/tests.rs`、`browser-runtime/fixtures/profile-migrations.json`。
**交付行为：** UUID Profile 与新 generation migration，兼容重启保留状态，不兼容 downgrade 保留原始数据。

- [ ] 定义 project/named/ephemeral manifest、generation/epoch/runtime compatibility 与 owner refs，写 path 变化不改 ID、孤立 project 不删除和跨 Profile 不串用失败测试。
- [ ] 实现 app-data/browser/v1 路径与 user-only权限/no-follow metadata，persistent eligibility 由 M0证据+distribution gate决定，未证明平台仅ephemeral。
- [ ] 实现 migration journal：独占 lock、停止 refs、复制新 generation staging、完整性校验、canary、发布新 manifest；每阶段增加 crash fault。
- [ ] 实现 rollback 选择兼容 generation，拒绝旧 Runtime 原地打开新 epoch；失败保留 source generation 与用户恢复入口。
- [ ] 实现 orphan 与 ephemeral cleanup，Session 删除只移除 refs；project/named 删除要求已确认 command，禁止 wipe/merge 或按项目路径递归删除。
- [ ] 验证低磁盘、备份 restore、两 App 争锁、N-1/N runtime x epoch 组合，检查所有失败后原数据 hash 与登录 fixture仍可用。

```rust
#[test]
fn downgrade_needs_compatible_generation() {
    let profile = ProfileGeneration::test(2, 8);
    let runtime = RuntimeProfileRange::test(6, 7);
    assert!(!runtime.can_open(&profile));
    assert_eq!(profile.epoch(), 8);
}
```

运行：`cargo test --manifest-path src-tauri/Cargo.toml browser::profiles::tests`。
运行：`node scripts/browser-runtime/run-profile-drills.mjs --native --all-faults --evidence docs/qa/browser-rearchitecture/m1/runtime/profiles`。
**失败处理：** manifest 写失败标 degraded；不删除原 generation，不把 crash 当 Profile corruption。
**验收：** migration 失败 source 保留，旧 App 不能绕过 sequence/epoch gate，ephemeral 仅在无 ref/lock后清除。

### M1-R05：Artifact、Runtime-owned Upload Spool 与 Export

**依赖：** M1-R03、M1-R04、M1-G01。
**负责：** 存储/传输工程师、Gateway 工程师。
**估算：** 7-10 工程日。
**文件：**
- 创建：`src-tauri/src/browser/artifacts/mod.rs`、`src-tauri/src/browser/artifacts/store.rs`、`src-tauri/src/browser/artifacts/access.rs`、`src-tauri/src/browser/artifacts/cleaner.rs`、`src-tauri/src/browser/artifacts/tests.rs`。
- 创建：`src-tauri/src/browser/transfers/mod.rs`、`src-tauri/src/browser/transfers/upload.rs`、`src-tauri/src/browser/transfers/upload_spool.rs`、`src-tauri/src/browser/transfers/download.rs`、`src-tauri/src/browser/transfers/export.rs`、`src-tauri/src/browser/transfers/tests.rs`。
- 修改：`src-tauri/src/media_server.rs`、`src-tauri/src/browser/mod.rs`、`src-tauri/src/browser/runtime/ledger.rs`、`src-tauri/src/browser/runtime/profile_guard.rs`、`src-tauri/src/browser/runtime/supervisor.rs`、`browser-runtime/worker/src/primitives.ts`；创建：`scripts/browser-runtime/run-transfer-drills.mjs`。
- 测试：`src-tauri/src/browser/artifacts/tests.rs`、`src-tauri/src/browser/transfers/tests.rs`。
**交付行为：** opaque Artifact/durable owner，200 MiB path upload 在 Runtime exit proof 后清理，延迟读取可用且保留占额；可恢复 download 与 export 人工核对。

- [ ] 定义 Artifact/UploadGrant/UploadSpoolLease/Download 与 owner record，写 token 越权、promotion/export crash、200 MiB 晚读、撤权后错误删除和共享 Session close 反例；覆盖截图 32 MiB/32 million pixels、下载 512 MiB、Trace 64 MiB、Session 临时 1 GiB 与全局 staging 2 GiB，保留 spool 也计入上限。
- [ ] 实现 partial -> hash/size -> rename -> ready、写前预留/逐块计数/重启重建预算，promotion 先 owner 再 journal；upload 以 no-follow 源 handle 复制为不可变 user-only spool，在派发路径前 durable 绑定 runtimeId/runtimeGeneration/hostBootId/runNonce、hash/size 与 dispatchState，MCP 只见 opaque handle。
- [ ] 所有上传使用 path-based setInputFiles；将单次授权撤销与 reader lifetime 分开，未派发可 purge，已派发或派发不确定则 cleanupPending；ACK/input clear/navigation/tab close/worker handle close 均不删除，完整 Runtime exit proof 后 no-follow purge，失败 tombstone 与额度保留，不采用 24 小时 Artifact retention。
- [ ] 实现 Runtime-owned spool ledger 与 Guard/reconciliation 接口，Session 关闭释放 tab/ref 但留清理义务，最后普通 ref 才 drain；暴露 pendingBytes/count、purgeFailed 和 scoped stop-and-cleanup，接管不关闭手动浏览器，不恢复 grant、不自动重传，任何 stale run 身份都先阻断清理。
- [ ] 实现 downloadId 独立 staging、receiving crash 清 partial、staged 只等待保存；export 固定 no-follow 父目录/同目录临时目标，复核覆盖许可及 quarantine/MOTW 后原子发布，失败保留源/原目标；exporting crash -> needs_review/unknown_outcome，不自动打开或删除 exported 文件。
- [ ] 跑 M0 延迟 FileReader/stream/人工提交的 1/50/200 MiB oracle，加 takeover/expiry/shared close/满预算/显式停止/Host-Guard-worker crash/删除失败，以及 download/export/promotion 故障；证明已派发副本持续可读、Runtime 全退出后才删、未授权新 attach=0，保留状态交 R06 实际 UI 接入。

```rust
#[test]
fn dispatched_upload_survives_revoke_until_runtime_exit() {
    let mut upload = UploadSpoolLease::test_dispatched("runtime-a", 7);
    upload.revoke(UploadRevokeReason::Takeover);
    assert!(!upload.accepts_new_attach());
    assert_eq!(upload.cleanup_state(), CleanupState::CleanupPending);
    assert!(!upload.can_purge_without_exit_proof());
    assert!(upload.reserved_bytes() > 0);
}
```

运行：`cargo test --manifest-path src-tauri/Cargo.toml browser::artifacts`、`cargo test --manifest-path src-tauri/Cargo.toml browser::transfers`。
运行：`node scripts/browser-runtime/run-transfer-drills.mjs --native --all-faults --evidence docs/qa/browser-rearchitecture/m1/runtime/transfers`。
**失败处理：** transfer 失败保留脱敏 metadata；已披露 File 可继续页面晚读，竞态不自动重传；无 exit proof 不删，purge 失败保留配额并阻断新 attach。
**验收：** Browser token 不复用 media token，未授权新披露=0，spool 永不作为 Artifact/MCP/support bundle；源文件、导出和共享 Runtime 未被误清理。

### M1-R06：恢复、关闭与 Runtime 联合验收

**依赖：** M1-R01、M1-R02、M1-R03、M1-R04、M1-R05、M0-W06、M1-G05。
**负责：** Runtime 负责人、QA 工程师。
**估算：** 5-7 工程日。
**文件：**
- 创建：`src-tauri/src/browser/runtime/recovery.rs`、`src-tauri/src/browser/runtime/recovery_tests.rs`、`src-tauri/src/browser/runtime/update_quiesce.rs`、`src-tauri/src/browser/runtime/update_quiesce_tests.rs`。
- 创建：`src/components/browser/runtime/projection.ts`、`src/components/browser/runtime/projection.test.ts`、`src/components/browser/runtime/RuntimeCleanupStatus.tsx`、`src/components/browser/runtime/RuntimeCleanupStatus.test.tsx`、`scripts/browser-runtime/run-runtime-matrix.mjs`。
- 修改：`src-tauri/src/browser/runtime/mod.rs`、`src-tauri/src/browser/runtime/supervisor.rs`、`src-tauri/src/browser/lifecycle.rs`（M1-G01 接口落地后）、`src-tauri/src/commands/browser.rs`、`src/lib/api/browser.ts`、`src/components/browser/BrowserWorkbench.tsx`、`src/components/browser/BrowserDiagnostics.tsx`（M1-G05 创建）与十五语言 Browser 领域目录。
- 创建：`browser-runtime/fixtures/runtime-matrix.json`、`docs/qa/browser-rearchitecture/m1/runtime/README.md`（实施阶段填入真实结果）。
- 测试：`src-tauri/src/browser/runtime/recovery_tests.rs`、`src-tauri/src/browser/runtime/update_quiesce_tests.rs`、`src/components/browser/runtime/projection.test.ts`、`src/components/browser/runtime/RuntimeCleanupStatus.test.tsx`。
**交付行为：** Host 重启恢复上下文但不恢复控制，Session close 与 Runtime 清理状态分开可见，Browser 独立 quiesce 可失败恢复且不停止 ACP/IM。

- [ ] 写旧 boot/binding/lease/token 拒绝、未知副作用、安全 restorePolicy，以及 retained spool 误删/漏计额、共享 Session close、Runtime drain 失败与 update quiesce 部分失败的测试，验证 ACP/IM teardown 调用数为零。
- [ ] 实现启动 reconciliation：ready owner 重建、普通 partial 清理；upload 根据独立 durable dispatch/run ledger 还原预算，旧 descendants exit proof 后才 purge，身份不明则 reconciliation_required；在副作用前写无 payload inflight，恢复只报 unknown_outcome，会话锁存 user 且不恢复旧队列。
- [ ] 接 Session close/delete：admission/fence/revoke/cancel -> durable closing -> tab/ref 释放 -> 未派发 spool 清除或义务 durable 移交后 Session closed；共享 Runtime 显示 cleanupPending，最后普通 ref 触发 drain，Runtime closed 必须 descendants=0 且 spool purge，失败 degraded 不伪报已清理。
- [ ] 实现幂等 beginAppUpdateQuiesce(updateId)/abortAppUpdateQuiesce(token)/commitAppUpdateQuiesce(token)：仅 Browser fence/revoke、上下文持久化、drain/exit proof/spool purge；begin 失败不交付成功 token，abort 保留 user latch、允许人工重开/Preview，绝不恢复 Agent 或调用 prepare_for_app_update，供 D05 在 install 前调用。
- [ ] 在现有 Workbench 与 M1-G05 BrowserDiagnostics 接同一 RuntimeCleanupStatus/Host 投影，展示 pendingBytes/count、cleanupPending/purgeFailed、空间不足和 scoped stop-and-cleanup；App dialog 确认受影响 Session，执行 busy/error/retry/键盘路径，所有新文案同步十五语言，不向 App shell 加 state。
- [ ] 联合 Gateway 跑三平台 golden path、200 MiB ACK 后延迟读与人工提交、takeover、Session close、显式清理、满预算、所有 crash ledger 和 quiesce/abort 并发；交付 M1-G06、D04、D05 所需接口及可复验的服务器/磁盘/进程 evidence。

```ts
const projection = projectRuntimeFailure({ reasonCode: 'runtime_unavailable', canPreview: true });
expect(projection.availableActions).toContain('openPreview');
expect(projection.agentActionCompleted).toBe(false);
expect(projection.controlMode).toBe('user');
```

运行：`pnpm exec vitest run src/components/browser/runtime/projection.test.ts`。
运行：`pnpm exec vitest run src/components/browser/runtime/RuntimeCleanupStatus.test.tsx src/i18n/messages.test.ts`。
运行：`cargo test --manifest-path src-tauri/Cargo.toml browser::runtime::recovery_tests`。
运行：`cargo test --manifest-path src-tauri/Cargo.toml browser::runtime::update_quiesce_tests`。
运行：`node scripts/browser-runtime/run-runtime-matrix.mjs --native --repeat 3 --evidence docs/qa/browser-rearchitecture/m1/runtime`。
**失败处理：** 缺平台/签名包证据不标完成；metadata 失败 degraded；Session 可 closed 且 Runtime cleanupPending，Runtime 未证实清理不得报告 closed/purged。
**验收：** 自动 replay、越权、Profile 串用、停止后 30 秒 orphan 全零；更新失败后 ACP/IM 仍可用且 Browser 只人工恢复，最终安装调用由 D05 验证。

## 合同与交接检查

- [ ] 所有RequestEnvelope字段与00-contracts一致，origin仅Host计算，u32计数不回绕。
- [ ] 原生Provider健康、policy有效、当前generation同时满足才允许writer；不是仅在启动时检查。
- [ ] M1-G06收到Runtime/transfer验收结果，M1-D04可调用已定义脚本，未形成互相依赖循环。
- [ ] 未执行的真机验证明确保留未勾选，不用本计划中的代码片段当作通过证据。
