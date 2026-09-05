# M0 Runtime 可行性实现计划

> **面向 AI 代理的工作者：** 使用 subagent-driven-development 或 executing-plans 逐任务实现。步骤使用未勾选复选框追踪；本文件记录未执行待办，当前文档交付不代表产品完成。实施期间的 commit/push 遵循总索引。

**目标：** 用固定 tuple、真实原生输入和受控故障证明三平台 Managed Runtime 的安全可行性。
**架构：** Host 保存 identity/fence，独立 ProfileGuard 持锁并监管 worker/Chromium，固定 fixture 提供外部副作用 oracle。
**技术栈：** Rust/Tauri、fs2、平台 API、固定 Node/playwright-core、Node test runner、签名包原生测试。
**规格：** [M0 Runtime](../../specs/2026-09-05-browser-rearchitecture/m0-runtime.md)、[共用协议](../../specs/2026-09-05-browser-rearchitecture/00-contracts.md)。
**估算：** 22-34 工程日；三平台设备、证书与受控 runner 为排期前提，等待时间另计。

## 文件职责与当前证据

| 路径 | 当前证据与本计划职责 |
| --- | --- |
| `src-tauri/src/side_browser_host.rs` | 当前仅系统 WebView；保持 Preview 对照组。 |
| `src-tauri/src/process_util.rs` | 当前自动补 HOME/PATH；Managed 新建独立 spawn 策略。 |
| `src-tauri/src/store_lock.rs` | 当前 atomic JSON/fs2；用于 descriptor，不把 Host 锁当 Guard 锁。 |
| `src-tauri/src/browser/protocol.rs` | M0-W01 创建的共享协议，本计划消费。 |
| `src-tauri/src/browser/runtime/` | 本计划新增 manifest、profile_guard、input_fence 与进程监管 PoC。 |
| `src-tauri/src/browser/input/` | 本计划新增 macOS/Windows Provider。 |
| `src-tauri/src/bin/browser-profile-guard.rs` | 本计划新增独立原生 Guard 可执行入口。 |
| `browser-runtime/worker/` | 本计划新增固定 worker PoC，M1 延续。 |
| `browser-runtime/fixtures/` | 本计划新增确定性网页、原生探针与进程 oracle。 |
| `scripts/browser-runtime/` | 本计划新增可复跑 manifest/签名/原生 runner。 |

未出现于当前代码的文件均标为创建。下列新增命令由所属任务创建后才可执行。
单元测试只能证明逻辑；标 `--native` 的命令需要对应 OS 原生 runner，不能在其他平台替代。
证据写入 `docs/qa/browser-rearchitecture/m0/`，当前尚无通过结果。
执行顺序为 R01 -> R04 -> R02 -> R03 -> R05 -> R06；原生输入探针必须使用已经受 Guard 监管的实验 Runtime。

### M0-R01：固定 Tuple 与确定性实验台

**依赖：** M0-W01。
**负责：** Runtime 工程师、供应链工程师。
**估算：** 3-5 工程日。
**文件：**
- 创建：`browser-runtime/runtime-lock.json`、`browser-runtime/worker/package.json`、`browser-runtime/worker/pnpm-lock.yaml`、`browser-runtime/worker/vitest.config.ts`、`browser-runtime/worker/tsconfig.json`。
- 创建：`browser-runtime/worker/src/main.ts`、`browser-runtime/fixtures/server.mjs`、`browser-runtime/fixtures/manifest.json`。
- 创建：`scripts/browser-runtime/build-tuple.mjs`、`scripts/browser-runtime/verify-tuple.mjs`、`scripts/browser-runtime/verify-tuple.test.mjs`。
- 创建：`src-tauri/src/browser/runtime/manifest.rs`；修改：`src-tauri/src/browser/mod.rs`、`src-tauri/src/browser/runtime/mod.rs`（首建时创建）。
- 测试：`scripts/browser-runtime/verify-tuple.test.mjs`。
**交付行为：** 三 target 固定依赖清单、tuple 构建/验证器及确定性 fixture；完整签名 tuple 在 R04/R05 产物齐全后由 R06 验收。

- [ ] 选择实施时仍受上游支持的 Node/Playwright 生产候选，记录支持依据和已知安全通告；固定 Node patch、playwright-core 精确版本及上游浏览器锁文件的 Chromium revision，写 schemaVersion=1、protocolVersion=1、三 target、source commit 与 license 清单；发行 sequence 使用规范十进制字符串。v1.55.0 仅为源码分析基线，候选与后续升级均须重跑 R05/R06 门。
- [ ] 创建 fixture `/ready`、`/delayed-button`、`/submit`、`/download` 和双 origin iframe；只在 runner 控制的 barrier 后允许副作用，并提供 `/counts` JSON oracle。
- [ ] 写 verifier 的 traversal、symlink cycle、Framework allowlist、hash mismatch 与未声明文件反例；先运行测试确认拒绝逻辑尚未实现而失败。
- [ ] 实现 signature-before-extract、no-follow staging、size/file-count ceiling、逐跳相对 symlink 验证及签名后文件 hash 校验，失败清 staging 且不启动 helper。
- [ ] 构建固定 worker initialize/close PoC，使用 bundled executable + private pipe；stdout 拒绝非帧数据，启动前无 Chromium child；独立 package 锁定 Vitest/TypeScript，test 脚本为 `vitest run --config vitest.config.ts`，include 为 `src/**/*.test.ts`，不引入 pnpm workspace。
- [ ] 三 target 验证 lock、license/SBOM 和签名 archive corpus，构建器缺任何 required helper 时返回 guard_missing；定义完整 build 与断网运行入口，由 R06 在 Guard/worker 完成后产生最终 tuple。

```js
import assert from 'node:assert/strict';
import { validateEntry } from './verify-tuple.mjs';
assert.throws(() => validateEntry({ path: '../escape', type: 'file' }, {}));
assert.throws(() => validateEntry({ path: 'a', type: 'symlink', target: '/tmp/x' }, {}));
assert.throws(() => validateEntry({ path: 'extra', type: 'file' }, { files: [] }));
```

运行：`node --test scripts/browser-runtime/verify-tuple.test.mjs`，预期所有恶意 entry 被拒绝。
运行：`pnpm --dir browser-runtime/worker install --frozen-lockfile`，预期仅安装锁定 worker 依赖；生产 package 从 tuple 取 Node，不依赖此开发命令。
运行：`node scripts/browser-runtime/build-tuple.mjs --all-targets --validate-lock --verify-fixtures --out .artifacts/browser-runtime`，预期三 target 锁定清单和 corpus 验证通过；缺 Guard 时完整 build 必须明确失败。
**失败处理：** 固定版本与许可缺失时停止 tuple build，不下载“最近可用”替代物。
**验收：** valid archive 可重复验证，所有 negative corpus 拒绝，PoC 打包不开放 Managed 产品 flag。

### M0-R02：macOS 原生输入归属与失效探针

**依赖：** M0-R01、M0-R04。
**负责：** macOS 原生工程师、测试工程师。
**估算：** 3-5 工程日。
**文件：**
- 创建：`src-tauri/src/browser/runtime/input_fence.rs`、`src-tauri/src/browser/input/mod.rs`、`src-tauri/src/browser/input/macos.rs`。
- 创建：`src-tauri/src/browser/input/tests.rs`、`browser-runtime/fixtures/native-input-macos.json`。
- 创建：`scripts/browser-runtime/run-native-input.mjs`；修改：`src-tauri/Cargo.toml`、`src-tauri/src/browser/runtime/mod.rs`。
- 测试：`src-tauri/src/browser/input/tests.rs`、`browser-runtime/fixtures/native-input-macos.json`。
**交付行为：** 已登记 Runtime 的 macOS 窗口输入产生原子 fence，权限/监听失效产生 blocked 状态。

- [ ] 定义 Provider health、ProcessIdentity、WindowIdentity 与归属输出，未知窗口或不匹配 generation 一律不能授权；记录最小事件类别，不存键值。
- [ ] 写权限拒绝、tap disabled、PID start time 变化和旧 generation 的 Rust 失败测试，确保判定先于 writer admission。
- [ ] 实现 listen-only CGEventTap 与 Input Monitoring 探测，独立消息循环监测 disabled 信号；Preview 分发路径只做对照，不增加 Agent DOM 能力。
- [ ] 用受控 Runtime 窗口建立 PID/start time/window registry，窗口增加/销毁或焦点变化时验证归属；未知归属直接阻断而不猜当前 tab。
- [ ] 在已签名安装形态跑 pointer/wheel/key/touch/IME、远程桌面、权限撤销、sleep/wake 和 Playwright 不自触发用例，逐项记录未覆盖范围。
- [ ] 在 Apple Silicon 与真实 Intel/x64 VM 各采集至少 30 个 fencing 样本，记录 Host 接收至 worker ACK 的 P50/P95/max 与 Provider 恢复不自动交还断言。

```rust
#[test]
fn missing_attribution_never_grants_writer() {
    let state = ProviderState::blocked(InputBlockReason::UnknownWindow);
    assert!(!state.can_admit_writer());
    assert_eq!(state.reason_code(), "blocked_input_attribution");
}
```

运行：`cargo test --manifest-path src-tauri/Cargo.toml browser::input::tests`，预期拒绝所有含糊身份。
运行：`node scripts/browser-runtime/run-native-input.mjs --platform macos --native --samples 30 --evidence docs/qa/browser-rearchitecture/m0/macos-input`。
预期：权限拒绝/失效均 blocked；人工输入撤销正确 Runtime；自动化输入不自触发；p95 <=250 ms。
**失败处理：** 原生来源或真实包不通过时 macOS 对应架构 no-go，不增加绕过开关。
**验收：** report 明确硬件/OS/签名身份与 unsupported 输入类别，不能使用 DOM isTrusted 代替来源证明。

### M0-R03：Windows 普通用户输入归属

**依赖：** M0-R01、M0-R02（复用 Provider 合同，不依赖 macOS 通过结论）。
**负责：** Windows 原生工程师、测试工程师。
**估算：** 3-5 工程日。
**文件：**
- 创建：`src-tauri/src/browser/input/windows.rs`、`browser-runtime/fixtures/native-input-windows.json`。
- 修改：`src-tauri/src/browser/input/mod.rs`、`src-tauri/src/browser/input/tests.rs`、`src-tauri/Cargo.toml`。
- 修改：`scripts/browser-runtime/run-native-input.mjs`。
- 测试：`src-tauri/src/browser/input/tests.rs`、`browser-runtime/fixtures/native-input-windows.json`。
**交付行为：** 普通用户 Raw Input/hook 线程将 HWND/PID 输入归到 Runtime，线程故障立即关闭 admission。

- [ ] 添加 HWND 重用、PID start time 变化、销毁窗口后迟到 input、message-loop stopped 的失败测试。
- [ ] 配置 Windows feature flags，创建专用消息线程与 Raw Input 注册，必要低级 hook 只做观测，不申请 administrator/uiAccess。
- [ ] 维护 HWND 与完整 ProcessIdentity 映射，重新分配或未知窗口不能复用旧 runtimeGeneration，线程退出向 Host 发布 unhealthy。
- [ ] 将健康故障接到相同 fence/observer-stop 通道；重新注册后保留用户 latch，直到可信 App UI 显式交还。
- [ ] 在最终安装路径运行 keyboard/wheel/touch/IME、多个窗口、远程桌面、休眠恢复和 Playwright 输入，oracle 验证不会撤销其他 Runtime 或用户 Chrome。
- [ ] 采集最少 30 个真机样本及低速系统压力样本，记录不依赖管理员的执行身份与阻断结果。

```rust
#[test]
fn reused_window_invalidates_runtime_owner() {
    let first = WindowIdentity::test(7, 10, 100, 1);
    let reused = WindowIdentity::test(7, 10, 200, 1);
    assert!(!first.same_process_and_generation(&reused));
}
```

运行：`cargo test --manifest-path src-tauri/Cargo.toml browser::input`，预期 stale HWND/PID 全部拒绝。
运行：`node scripts/browser-runtime/run-native-input.mjs --platform windows --native --samples 30 --evidence docs/qa/browser-rearchitecture/m0/windows-input`。
预期：三种健康故障都先阻断，p95 <=250 ms，零错误 domain 撤销。
**失败处理：** hook/Raw Input 无法证明覆盖的输入类型保留显式接管，影响安全承诺则平台 no-go。
**验收：** Windows x64 普通用户与签名包结果独立记录，交叉编译不计入样本。

### M0-R04：独立 Guard、Job/PGID 与 Profile 锁

**依赖：** M0-R01。
**负责：** 原生 Runtime 工程师、安全测试工程师。
**估算：** 4-6 工程日。
**文件：**
- 创建：`src-tauri/src/bin/browser-profile-guard.rs`、`src-tauri/src/browser/runtime/profile_guard.rs`。
- 创建：`src-tauri/src/browser/runtime/containment_macos.rs`、`src-tauri/src/browser/runtime/containment_windows.rs`、`src-tauri/src/browser/runtime/ledger.rs`。
- 创建：`src-tauri/src/browser/runtime/guard_tests.rs`、`browser-runtime/fixtures/process-tree.mjs`、`scripts/browser-runtime/run-guard-drills.mjs`。
- 修改：`src-tauri/Cargo.toml`、`src-tauri/src/browser/runtime/mod.rs`。
- 测试：`src-tauri/src/browser/runtime/guard_tests.rs`、`browser-runtime/fixtures/process-tree.mjs`。
**交付行为：** Host 硬退出后 Guard 仍持锁清理，本 run descendants 退出得到证明后才释放 Profile。

- [ ] 写双 claimant、Host EOF、PID 复用、Job assign 失败的失败测试，并定义 ledger 必含 bootId/run nonce/profileId/PID/start time/executable identity。
- [ ] 实现 Guard 私有 liveness pipe、reconciliation lock 和 Profile lock，锁只归 Guard；旧 ledger 无法证明退出时返回 reconciliation_required；Cargo 新增 Guard binary 后设置 `default-run = "grok-app"`，保留原 App 启动目标。
- [ ] 实现 macOS Guard 位于独立 PGID 外，worker 开始执行前入组；TERM 后有界 KILL、reap、逐 descendant 身份确认；Host 监测 Guard 意外退出并持 reconciliation lock 承接相同清理。
- [ ] 实现 Windows CREATE_SUSPENDED、AssignProcessToJobObject、禁止 breakaway、KILL_ON_JOB_CLOSE 后 ResumeThread；Job handle 不可继承且仅 Guard 持有。
- [ ] 用 process fixture 注入 Host/Guard/worker crash、spawn 中断、nested Job、detached child 和 PID 重用；同时保持用户 Chrome 存活作为误杀 oracle。
- [ ] 在三原生平台重复 drill，核对 30 秒 descendants=0、旧 run 未清完前第二 claimant 不能写 Profile，并留脱敏 evidence。

```text
spawn suspended -> assign containment -> verify containment -> resume
EOF -> close admission -> fence -> bounded drain -> terminate descendants
    -> verify each recorded identity exited -> persist clean ledger -> unlock
```

运行：`cargo test --manifest-path src-tauri/Cargo.toml browser::runtime::guard_tests`。
运行：`node scripts/browser-runtime/run-guard-drills.mjs --native --repeat 3 --evidence docs/qa/browser-rearchitecture/m0/guard`。
预期：所有 crash point 在 30 秒后本 run descendants=0，其他 Chrome PID identity 未受影响。
**失败处理：** 无法确认退出时保持锁/隔离状态，禁止 TTL 偷锁、按进程名杀进程或提前报告 clean。
**验收：** suspended 阶段 worker fixture 的 first-instruction counter=0，Job 绑定前没有 worker 代码执行。

### M0-R05：存储保护、原语取消与上传 Reader 生命周期

**依赖：** M0-R02、M0-R03、M0-R04。
**负责：** Browser Runtime 工程师、安全测试工程师。
**估算：** 6-9 工程日。
**文件：**
- 创建：`browser-runtime/worker/src/fence.ts`、`browser-runtime/worker/src/primitive-probe.ts`、`browser-runtime/worker/src/primitive-probe.test.ts`。
- 创建：`src-tauri/src/browser/runtime/storage_probe.rs`、`src-tauri/src/browser/runtime/upload_spool_probe.rs`、`scripts/browser-runtime/run-storage-probes.mjs`、`scripts/browser-runtime/run-fence-probes.mjs`、`scripts/browser-runtime/run-upload-spool-probes.mjs`。
- 修改：`browser-runtime/fixtures/server.mjs`、`browser-runtime/worker/package.json`、`src-tauri/src/browser/runtime/mod.rs`。
- 测试：`browser-runtime/worker/src/primitive-probe.test.ts`、`src-tauri/src/browser/runtime/storage_probe.rs` 内的 `#[cfg(test)]` 模块。
**交付行为：** 明确 primitive 取消、persistent eligibility 与 200 MiB path upload 的 Runtime-owned spool 保留/退出后清理策略；无证据的能力保持关闭。

- [ ] 扩展 fixture 为等待/派发/ACK 后 barrier，统计提交、下载、FileReader/slice/stream 的 hash、读取错误和字节数；增加 1 MiB、50 MiB 边界、200 MiB、超限与 2 GiB 预算 fixture，并写 fence 后 Agent 后续 primitive=0 的失败测试。
- [ ] 对照 Playwright v1.55.0 分析基线，在 R01 锁定候选上核对 `server/dom.ts` -> `chromium/crPage.ts` -> `DOM.setFileInputFiles` 的路径语义和 buffer 限制并记录差异；实现 path-only spool probe，路径派发前 durable Runtime lease，ACK/worker 句柄释放不触发 purge。
- [ ] 实现 worker fence/取消探针，覆盖 navigation/click/fill/screenshot/observer/upload/download 的原生 takeover；ACK 后和撤权后延迟 FileReader/人工提交仍读完整字节，Agent 竞态返回 unknown_outcome，不用 Promise.race 或页面晚读替代 Agent 取消 oracle。
- [ ] 实现 ACL/mode、no-follow、私有 cwd/env allowlist 与合成凭据 Keychain/DPAPI/备份探针；明确范围外文件可见性，未证明静态保护则 ephemeral-only，未证明的 OS 隔离不作承诺。
- [ ] 注入未派发撤销、已派发 takeover/turn/Session close、共享 Runtime、Host/Guard/worker crash、删除失败；未派发可删，已派发保留配额直至完整 Runtime exit proof，验证 Runtime stop 后清理且用户 Chrome/源文件不变。
- [ ] 三平台汇总 primitive 支持表、晚读完整性、spool ledger/cleanupPending、空间释放和 credential-at-rest 证据；无法证明 reader lifetime 或 exit proof 的上传能力 no-go，不用跳过用例增加通过率。

```ts
import assert from 'node:assert/strict';
type ProbeEvidence = {
  beforeWrites: number; afterWrites: number;
  outcome: 'interrupted' | 'unknown_outcome';
  currentEpoch: number; acknowledgedEpoch: number;
};
export function assertFenced(probe: ProbeEvidence) {
  assert.equal(probe.afterWrites - probe.beforeWrites, 0);
  assert.equal(probe.acknowledgedEpoch, probe.currentEpoch);
 }
```

运行：`pnpm --dir browser-runtime/worker test`，预期 checkpoint 与取消状态测试通过。
定向运行：`pnpm --dir browser-runtime/worker exec vitest run --config vitest.config.ts src/primitive-probe.test.ts`，预期真实扫描 worker 测试，禁止零测试成功。
运行：`node scripts/browser-runtime/run-fence-probes.mjs --native --all-primitives --evidence docs/qa/browser-rearchitecture/m0/fence`。
运行：`node scripts/browser-runtime/run-storage-probes.mjs --native --evidence docs/qa/browser-rearchitecture/m0/storage`。
运行：`node scripts/browser-runtime/run-upload-spool-probes.mjs --native --sizes 1MiB,50MiB,200MiB --all-faults --evidence docs/qa/browser-rearchitecture/m0/upload-spool`。
**失败处理：** 取消或 spool 生命周期不可靠的 primitive unsupported；凭据静态保护未证实的 persistent Profile 不启用。
**验收：** 每项结论有页面/服务器/进程与磁盘 oracle，上传 ACK 后不可删除副本；完整 Runtime 退出并成功 purge 后才释放配额。

### M0-R06：三平台可行性结论与版本化基线

**依赖：** M0-R01、M0-R02、M0-R03、M0-R04、M0-R05。
**负责：** QA 负责人、Runtime 负责人。
**估算：** 3-4 工程日。
**文件：**
- 创建：`scripts/browser-runtime/run-m0-matrix.mjs`、`scripts/browser-runtime/validate-evidence.mjs`、`scripts/browser-runtime/validate-evidence.test.mjs`。
- 创建：`browser-runtime/fixtures/m0-matrix.json`、`browser-runtime/fixtures/measurement-schema.json`。
- 修改：`.github/workflows/ci.yml`；创建：`docs/qa/browser-rearchitecture/m0/README.md`（实施阶段填写实际证据索引）。
- 测试：`scripts/browser-runtime/validate-evidence.test.mjs`、`browser-runtime/fixtures/m0-matrix.json`。
**交付行为：** 三平台逐项 go/no-go、CPU/RSS/包体基线和 M1 能力 allowlist，没有隐含全平台通过。

- [ ] 固定 matrix 的 OS/arch/package/fixture 行与 required 字段，缺架构、三轮不足或 mixed tuple digest 使 evidence validator 失败。
- [ ] 创建 metadata-only evidence 验证测试，拒绝 raw URL query、private path、secret-like payload、空样本与缺 denominator。
- [ ] 编译签名 R04 Guard 与 R05 worker，调用 R01 构建器形成完整 tuple，再编排 native input/Guard/storage/fence/upload-spool probe，每平台连续三轮；失败保留原 report，不用重跑覆盖失败事实。
- [ ] 最少 30 样本测 cold start/new tab/fence/crash，记录起止事件；建立 CPU/RSS/disk/package 基线并评估默认两个 Runtime ceiling。
- [ ] 将纯 schema/unit 检查接入 CI，把真机行明确标 native runner required；x64 交叉编译产物不得自动标 execution passed。
- [ ] 产出逐平台可用 capability、persistent eligibility、未覆盖输入与 no-go 原因；与 M0-W06 Preview 门共同提交 M0 验收记录。

```js
assert.equal(report.repetitions, 3);
assert.equal(report.results.every(row => row.passed && row.nativeExecution), true);
assert.equal(report.orphanDescendants, 0);
assert.equal(report.unauthorizedActions, 0);
assert.equal(report.automaticReplays, 0);
```

运行：`node --test scripts/browser-runtime/validate-evidence.test.mjs`。
运行：`node scripts/browser-runtime/build-tuple.mjs --all-targets --poc --out .artifacts/browser-runtime`，预期所有 required helper、签名后 hash 和完整三 target tuple 具备。
运行：`node scripts/browser-runtime/run-m0-matrix.mjs --native --repeat 3 --evidence docs/qa/browser-rearchitecture/m0`。
原生 runner 只产出本平台结果；聚合运行 `node scripts/browser-runtime/validate-evidence.mjs --milestone m0 --require-all-targets --evidence docs/qa/browser-rearchitecture/m0` 才能判定全部平台。
预期：每 required 行明确通过或阻断，证据缺失退出非零，不生成伪通过证书。
**失败处理：** 未通过平台保持 Managed disabled；性能可重新设版本化 ceiling，但安全零容忍项不可放宽。
**验收：** report 指向真实包 digest 与原生设备，M1-R01 只接受满足门的 target。

## 交接检查

- [ ] 六项任务 evidence 均包含版本/平台/时间/命令/结果，尚未执行项保持未勾选。
- [ ] 与 M0-W01 的字段名、u32 overflow 处理和 reasonCode 一致，未新增 caller-origin 信任。
- [ ] 与 M0-W06 分别保留 Preview 与 Runtime 门，不把一方通过算作另一方完成。
- [ ] 无 secret、机器私有路径或原始输入进入仓库；完成文档审查不代表完成运行验证。
