# 2026-10-22 STATUS.md 抽查审计报告

本次审计对 STATUS.md 中所有标记为 done 的卡片及近两日新增记录（重点核对 WL1–WL3、C04、click-approach、指针闸、重试、C08/C22/W-HOTKEY 与 P9-04、P6-07 两行）进行了源码与历史证据的一致性排查。
证据真实性总体优良：所有声明的 Rust 测试函数名（40+ 项）、ADR 编号（0001–0015）以及提交 SHA 均在代码库与 git 提交记录中真实存在且语义吻合。
真机测试结果（如 13/8/1/8、18/8/1/3、20/9/1/0、400 次 0 锁存）在 `computer-use/crates/gcu-driver-windows/diagnostics.md` 与 ADR 0013 增补文件中均有完整可核查的原始记录与时间线。
主要不一致 1（P9-04 行内矛盾）：状态列已由项目所有者豁免外部审阅签字（PR #158），但备注末尾仍残留“待用户指定审阅者；签字栏保持空”的旧表述；且引用的数据清单路径缺少 `computer-use/` 前缀。
主要不一致 2（文档路径前缀不一）：部分卡片引用的 `docs/*.md` 路径缺少 `computer-use/` 前缀；P6-07 评测代码已合入但行仍保持 `todo`（符合未完成真实模型 DoD 暂不标记 done 的规程要求）。

---

## 1. 计划与一致性自检脚本执行结果

执行命令：`python3 -I docs/plans/computer-use-v2/reviews/check_plan.py`
输出结果：
```text
PASS: 14份Markdown，261个相对链接/锚点；83张卡与STATUS一一对应，依赖纯卡号/无悬空/无环，文件/测试/DoD/验收命令齐全。
PASS: 阶段工时 P0=21.5d、P1=19.5d、P2=18.5d、P3=10.0d、P4=18.5d、P5=26.0d、P6=15.0d、P7=17d、P8=12d、P9=19.0d、S=3.0d；P0–P9=177.0d，spike=3.0d。
PASS: 处置 25+18+24=67 条，无遗漏或重复。
```

---

## 2. 抽查审计结果表格

| 行 / 卡 / 条目 | 声称内容 | 核对方法 | 结果 |
|---|---|---|---|
| STATUS 行 141（WL1） | 动作期证明丢弃旧租约、常量与 fail-closed 不变；测试 `late_initial_probe_pair_overlapping_first_foreground_does_not_latch_stop`、`stale_action_proof_during_injection_latches_stop`、`removed_hooks_during_injection_latches_stop`、`missing_injection_renewal_latches_at_the_heartbeat_timeout` | `grep` 验证 `lease_tests.rs` 内 4 个 `fn`；核对 `computer-use/docs/adr/0013-addendum-initial-probe-overlap.md` | 一致 |
| STATUS 行 142（WL1 复测） | win-test 标准用户提交 `879bb8a5`；真机契约 6 通过/7 跳过/1 not_run/16 失败；时间线分析与工作假设 | `git cat-file -t 879bb8a5` 验证提交；核对 `diagnostics.md` 行 530–557 小节数据 | 一致 |
| STATUS 行 143（WL2） | 提前续租、鼠标与 F24 合并 SendInput、MsgWait 去掉盲 20ms 睡眠；6 项测试；元数据 `hook_probe_trace`/`lease_age_ms` | `grep` 验证 6 个 `fn`（如 `serial_20ms_probe_pair_survives_a_70ms_injection`）；检查 `probe.rs` 与 `hotkey.rs` 元数据字段 | 一致 |
| STATUS 行 144（WL2 复测） | win-test 标准用户提交 `55d0528c`；真机契约 13 通过/8 跳过/1 not_run/8 失败；`lease_age_ms` 约 11–31；无 Stop 锁存 | `git cat-file -t 55d0528c` 验证提交；核对 `diagnostics.md` 行 558–590 与 ADR 0013 增补文本 | 一致 |
| STATUS 行 145（WL3） | 续租探针与动作输入缺少事务分析；C04/C08 根因；`injection` 互斥闸；6 项测试名；换行改 Enter | `grep` 验证 `injection.rs:81`、`injection.rs:95`、`lease_tests.rs` 与 `messages.rs:236` 中的测试声明；检查 ADR 0013 增补文本 | 一致 |
| STATUS 行 146（C04 夹具修复） | 网格绘制 320×240 与无障碍边界 402×240 不一致；测试 `wide_column_grid_accessible_bounds_match_painted_cells`；分支 `cu2/C04-fixture-grid` | 检查 `computer-use/apps/fixture/tests/accessibility.rs:283` 存在对应测试函数；检查 PR #152 提交记录 | 一致 |
| STATUS 行 147（WL3 复测与回退） | win-test 标准用户提交 `1205115a`；契约 11 通过/7 跳过/1 not_run/11 失败；出现 `stop_listener failed`；回退代码提交（PR #153），保留分析文档与 C04 修复 | `git cat-file -t 1205115a`；`git log` 确认 PR #153（commit `32be667f` / `e88708a3`）回退记录与原因 | 一致 |
| STATUS 行 148（click-approach） | 前台 PointerClick/PointerDrag 逼近移动 1px + 30ms 停顿；分支 `cu2/click-approach`（PR #154）；复测 `contract-out-20261021-191155.txt` 14/8/1/7 无改善 | `git log` 确认 PR #154；核对 `diagnostics.md` 行 614 实测输出文件名与数据 | 一致 |
| STATUS 行 149（指针闸） | 命名互斥体 `Local\gcu-pointer-gate-<host pid>`，env `GCU_HOST_PID`，40ms 超时 fail-closed；ADR 0013 增补；契约 18/8/1/3（PR #155） | 检查 `hook_probe.rs:26`、`hook_probe.rs:58`、`process.rs:54`；核对 `0013-addendum-cross-process-pointer-gate.md` 与 `diagnostics.md` 行 638 | 一致 |
| STATUS 行 150（C08/C22/W-HOTKEY 与拖拽锁存） | 前台换行 Enter、BMP+ZWJ、Tab 焦点、第二驱动窗口解析、有界等待光标到位；契约 20/9/1/0；拖拽压测 1080 次 24 次锁存（~2%），测试 `live_drag_stress` | 检查 `live.rs:699` 存在 `live_drag_stress`；`git log` 确认 PR #156（commit `2bab89b8`）；核对 `diagnostics.md` 行 656–693 | 一致 |
| STATUS 行 151（拖拽锁存缓解/重试） | `MouseProof::next` 缺失回调一次有界重试；400 次拖拽 0 锁存；契约 20/9/1/0；4 个重试用例 | 检查 `hook_probe.rs:326` 及 `tests/hook_probe.rs` 中 4 项重试测试；`git log` 确认 PR #157（commit `591a0ebd`）；核对 `diagnostics.md` 行 694–709 | 一致 |
| STATUS 行 152（P6-07 真实模型运行路径） | `gcu-eval` 新增 `--driver real`、`--fixture`、`--auth-seed`、`--only`；JSON 评分转换；默认 fake 不变；尚未跑真实模型保持 todo | 检查 `gcu-eval/src/main.rs:84-132`、`runner.rs`、`tests/runner.rs:839-883`；`git log` 确认 PR #159 / #160 | 一致 |
| STATUS 行 153（P2-09 物理控制台契约运行） | 提权/降权运行受限；标准用户 6/7/1/16；`confirm_hook_proofs` 守卫超时；`D:\cu2\hw\run-live-std.ps1` 不在仓库 | 检查 `hotkey.rs:123` 存在 `confirm_hook_proofs`；核对 `diagnostics.md` 行 478–529；脚本按声明无法核对（外部机器） | 一致 / 无法核对（机器本地脚本） |
| STATUS 行 154（P9-04 缺口 G1） | 持久授权收敛到 `persistent_eq` / `persistent_grant_matches`；ADR 0015；Identity.proof 字段 | 检查 `convert.rs:15`；核对 `computer-use/docs/adr/0015-persistent-grant-identity.md` 与协议字段 | 一致 |
| STATUS 行 87（P9-04 状态与备注） | 状态列声明所有者豁免审阅者签字（PR #158）；备注列声明引用 `computer-use/docs/security.md` 与 `docs/data-inventory.md`、缺口 G1–G7 处理；备注末尾声明“仍未完成：外部审阅者签字（DoD，待用户指定审阅者；签字栏保持空）” | 检查 `security.md` 与 `data-inventory.md`；核对 PR #158（commit `bd14c5d4`）；对比状态列与备注列表述 | **不一致**（备注列残留未豁免旧表述；`data-inventory.md` 漏前缀） |
| STATUS 行 72（P6-07 状态与卡行） | 状态 `todo`，分支、PR、日期、备注留空；阶段记录中记录 A/B 框架与 real 模型路径（行 140、行 152） | 检查已合入的 PR #145、#159、#160；对比 08 规程关于拆卡与未完成 DoD 状态约定 | 一致（未完成真机评测前状态保持 todo 符合规程；建议按规程记录已合入子分支） |
| STATUS 行 8（S-01） | Grok MCP 探测；ADR 0001 已接受；分支 `cu2/S-01-grok-mcp-probe`；PR #6 | 检查 `0001-grok-build-mcp-capabilities.md`；`git log` 确认 commit `5b1ad149` | 一致 |
| STATUS 行 9（S-02） | macOS TCC 与 helper；ADR 0006；分支 `cu2/S-02-tcc-adr` | 检查 `0006-macos-tcc-helper.md`；`git log` 确认 PR #45（commit `54753fb3`） | 一致 |
| STATUS 行 10（P0-01） | computer-use workspace；分支 `cu2/P0-01-workspace`；PR #2 | 检查 `computer-use/Cargo.toml`；`git log` 确认 commit `6ef56daf` | 一致 |
| STATUS 行 12（P0-03） | gcu-protocol；预算 13,821/14,000 字节；window_minimized 错误码；PR #5 | 检查 `computer-use/crates/gcu-protocol/golden/errors.json:342`；`git log` 确认 commit `db1e7feb` | 一致 |
| STATUS 行 13（P0-04） | gcu-host-api；24 测试；Stop 锁存无引擎清除；PolicyStore 只读 | 检查 `computer-use/crates/gcu-host-api/` 测试与接口 | 一致 |
| STATUS 行 15（P0-06） | 树管线与渲染；13步管线；55快照；文本20KB上限 | 检查 `gcu-core/tests/snapshots/*.snap` 与渲染管线实现 | 一致 |
| STATUS 行 16（P0-07） | SnapshotStore/Index/Diff；3快照；105测试 | 检查 `gcu-core/tests/snapshots_cases.rs` 与脱敏类型 | 一致 |
| STATUS 行 18（P0-08a） | 策略数据；拒绝名单138条+15语言敏感词；PR #4 | 统计 `computer-use/crates/gcu-core/data/denylist.toml` 确为 138 条条目；检查 15 语言敏感词列表 | 一致 |
| STATUS 行 20（P0-10） | gcu-cli（fake）；ADR 0003 resume 信任边界；行覆盖84.75% | 检查 `computer-use/docs/adr/0003-resume-trust.md`；检查 CLI 命令实现 | 一致 |
| STATUS 行 28（P1-06） | macOS 定向事件；ADR 0006；字段 51；CGEventSetWindowLocation；InputLedger | 检查 `gcu-driver-macos/src/input.rs` 与 `InputLedger` 实现 | 一致 |
| STATUS 行 31（P1-09） | macOS 契约测试真机 26 通过/4 跳过/0 失败；夹具 7->54 节点 | 检查 `gcu-driver-macos` 契约用例与 AccessKit 懒加载规避逻辑 | 一致 |
| STATUS 行 38（P2-07） | Windows 守卫/热键；LL 钩子；RegisterHotKey Stop；watchdog | 检查 `gcu-driver-windows/src/input/hotkey.rs` 与 watchdog | 一致 |
| STATUS 行 40（P2-09） | Windows 契约真机 15 通过/15 跳过/0 失败；第 4 探针；diagnostics.md 引用 | 检查 `diagnostics.md` 对应节与契约用例跳过理由 | 一致 |
| STATUS 行 45（P3-05） | Linux 契约 23 通过/6 跳过/0 失败；smoke-linux.sh | 检查 `computer-use/scripts/smoke-linux.sh` 脚本存在 | 一致 |
| STATUS 行 46（P4-01） | macOS helper 与代理；LSUIElement .app；30 helper 测试 + 38 xtask 测试 | 检查 `gcu-cli/src/helper/` 模块与 xtask bundle 实现 | 一致 |
| STATUS 行 47（P4-02） | Onboarding 窗口；ADR 0009；15 语言文案单一来源 `computer-use/ui/locales/*.json` | 检查 `0009-wry-onboarding-webview-deny.md` 与 locales 文件 | 一致 |
| STATUS 行 53（P4-07） | release-computer-use.yml；xtask release；sbom.rs；packages/；scripts/install-smoke/ | 检查 release 工作流文件、`sbom.rs`、packages 目录与测试脚本 | 一致 |
| STATUS 行 54（P4-08） | computer-use/README.md 与 README_ZH.md 中英一致；各平台说明 | 检查两个 README 文件存在且命令一致 | 一致 |
| STATUS 行 55（P5-01） | ComputerUseHost 骨架；gcu-host-protocol::supervisor；src-tauri 薄适配 | 检查 `gcu-host-protocol::supervisor` 与 `src-tauri/src/computer_use/` | 一致 |
| STATUS 行 56（P5-02） | Gateway + 注入；32 字节 token；acp_client scrub；Host/Origin 校验 | 检查 `gcu-mcp/src/http.rs`、`gcu-host-protocol/src/inject.rs`、`src-tauri/src/acp_client.rs` | 一致 |
| STATUS 行 57（P5-03） | 审批卡；gcu-host-protocol 状态机；ComputerUseApprovalCard；15 语导出 | 检查组件实现与 vitest 56 项测试通过情况 | 一致 |
| STATUS 行 60（P5-06） | Computer 侧栏面板；sideWorkbench.ts 原先超 600 行未拆分 | 检查 `src/lib/sideWorkbench.ts` 行数（758 行，确超 600 行） | 一致 |
| STATUS 行 61（P5-07） | 覆盖窗；cu-overlay.json 独立 capability；engine_stop_tests；引用 `overlay/README.md` | 检查 `cu-overlay.json`、`engine_stop_tests.rs`；`overlay/README.md` 实际位于 `computer-use/crates/gcu-overlay/README.md` | **不一致**（路径漏前缀） |
| STATUS 行 62（P5-08） | 设置页；ComputerUseSection；settingsCatalog 登记；命令注册 | 检查设置组件、`src/lib/settingsCatalog.ts` 与 `command_registry.rs` | 一致 |
| STATUS 行 63（P5-09） | 生命周期接线；docs/llm-wiki/computer-use.md 登记 AGENTS.md 与 settings-ia.md | 检查 `AGENTS.md` Read first 列表与 `settings-ia.md` 引用 | 一致 |
| STATUS 行 66（P6-01） | settle 调优；gcu-core::settle；tests/settle.rs；bench/settle.rs；bench/README.md | 检查 settle 模块、单测与 benchmark 文件 | 一致 |
| STATUS 行 67（P6-02） | auto 截图策略；ADR 0011；ScreenshotMode；golden/tools.json 预算 | 检查 `0011-auto-screenshot-policy.md` 与 tools.json 字节数 | 一致 |
| STATUS 行 68（P6-03） | wait_for 工具；tests/wait_for.rs；150ms 轮询 | 检查 wait 模块与 `tests/wait_for.rs` | 一致 |
| STATUS 行 69（P6-04） | computer_batch 工具；tests/batch.rs；≤32 步 | 检查 batch 模块与 `tests/batch.rs` | 一致 |
| STATUS 行 70（P6-05） | computer_script；rquickjs 0.14；引用 `docs/script-sandbox-review.md` | 检查 gcu-script 实现；`docs/script-sandbox-review.md` 实际位于 `computer-use/docs/script-sandbox-review.md` | **不一致**（路径漏前缀） |
| STATUS 行 71（P6-06） | 工具目录 profile；`golden/profiles/*.json`；引用 `docs/tool-profiles.md` | 检查 profiles golden；`docs/tool-profiles.md` 实际位于 `computer-use/docs/tool-profiles.md` | **不一致**（路径漏前缀） |
| STATUS 行 74（P7-02） | portal ScreenCast；ADR 0007（PipeWire PPA） | 检查 `0007-ashpd-pipewire-deny.md` 与 restore token 实现 | 一致 |
| STATUS 行 77（P7-05） | Wayland 契约测试；17 通过/1 ignored；wayland-ci.sh | 检查 `computer-use/scripts/wayland-ci.sh` 脚本 | 一致 |
| STATUS 行 78（P7-06） | Wayland 用户文档；computer-use/docs/wayland.md（554+ 行） | 检查 `computer-use/docs/wayland.md` 存在（564 行） | 一致 |
| STATUS 行 79（P8-01） | 精简 MV3 DOM 扩展；computer-use/tools/browser-extension/；tests/dom.test.ts | 检查扩展源码目录、manifest 与单测文件 | 一致 |
| STATUS 行 80（P8-02） | Native messaging host；tests/native_host.rs；引用 `docs/cli.md` | 检查 native_host 实现与测试；`docs/cli.md` 实际位于 `computer-use/docs/cli.md` | **不一致**（路径漏前缀） |
| STATUS 行 84（P9-01） | 评测 runner；gcu-eval | 检查 gcu-eval 各模块与 fake 驱动测试 | 一致 |
| STATUS 行 85（P9-02） | 任务集 ≥60；task_catalog 校验覆盖下限 | 检查 tasks 目录与 task_catalog 校验逻辑 | 一致 |
| STATUS 行 88（P9-05） | 兼容性矩阵；引用 `docs/compatibility.md`；全部 not_run | 检查兼容性矩阵文件；`docs/compatibility.md` 实际位于 `computer-use/docs/compatibility.md` | **不一致**（路径漏前缀） |

---

## 3. 不一致项与建议修改方案

### 不一致项 1：P9-04 备注中外部审阅者签字残留表述与状态列冲突
- **文件与行号**：`docs/plans/computer-use-v2/STATUS.md:87`
- **现状分析**：
  在 PR #158（commit `bd14c5d4`）中，项目所有者于 2026-10-22 正式豁免了外部审阅者签字程序。行 87 的状态列已由 `needs-decision: 外部审阅者签字...` 更新为：
  `external-review-waived: 外部审阅者签字已由项目所有者于 2026-10-22 豁免（缺口 G1–G7 已处理或有明确 not_run，见下；真机证据缺口与 07 §8 发布门槛不豁免）`
  并且 `computer-use/docs/security.md` 表格已更新为 `waived-by-owner`，`computer-use/crates/gcu-eval/tests/security.rs` 也增加了对 `waived-by-owner` 的断言测试。
  然而，行 87 的备注列（第 9 列）末尾仍保留有旧文字：
  `**仍未完成**：外部审阅者签字（DoD，待用户指定审阅者；签字栏保持空）；各缺口的真机证据...`
  导致状态列“已豁免”与备注列“仍未完成：待用户指定审阅者”产生直接行内事实冲突。
- **建议改法**：
  将行 87 备注列末尾的：
  `**仍未完成**：外部审阅者签字（DoD，待用户指定审阅者；签字栏保持空）；各缺口的真机证据（G1 三平台更新场景、G3 逐路径 Windows DACL 读回、G6 Developer ID 证书链正向、G7 真实 release 运行与签名公证）为 not_run；S5 等 partial 项未因本轮改为 signed`
  改为：
  `**仍未完成**：外部审阅者签字已由所有者豁免（见状态列与 security.md，非第三方签字）；各缺口的真机证据（G1 三平台更新场景、G3 逐路径 Windows DACL 读回、G6 Developer ID 证书链正向、G7 真实 release 运行与签名公证）为 not_run；S5 等 partial 项未因本轮改为 signed`

---

### 不一致项 2：P9-04 引用数据清单文件路径漏写前缀
- **文件与行号**：`docs/plans/computer-use-v2/STATUS.md:87`
- **现状分析**：
  行 87 备注中写道：
  `computer-use/docs/security.md（...）、docs/data-inventory.md（17 项落盘/持有数据的所有者/权限/保留期/清理路径）`
  其中 `security.md` 正确带有 `computer-use/` 前缀，但同一句中的 `docs/data-inventory.md` 缺少该前缀。仓库根目录下并不存在 `docs/data-inventory.md`（实际位于 `computer-use/docs/data-inventory.md`），易引发路径解析歧义或链接断链。
- **建议改法**：
  将 `docs/data-inventory.md` 改为 `computer-use/docs/data-inventory.md`。

---

### 不一致项 3：多处卡片引用文档路径前缀缺少 `computer-use/`
- **文件与行号**：
  - `docs/plans/computer-use-v2/STATUS.md:61`（P5-07）：`overlay/README.md`
  - `docs/plans/computer-use-v2/STATUS.md:70`（P6-05）：`docs/script-sandbox-review.md`
  - `docs/plans/computer-use-v2/STATUS.md:71`（P6-06）：`docs/tool-profiles.md`
  - `docs/plans/computer-use-v2/STATUS.md:80`（P8-02）：`docs/cli.md`
  - `docs/plans/computer-use-v2/STATUS.md:82`（P8-04）：`docs/browser-extension.md`
  - `docs/plans/computer-use-v2/STATUS.md:88`（P9-05）：`docs/compatibility.md`
- **现状分析**：
  在仓库根目录下，上述文件均不存在于 `docs/` 或 `overlay/` 根路径，而是实际位于 `computer-use/docs/` 或 `computer-use/crates/gcu-overlay/`。与 P7-06（`computer-use/docs/wayland.md`）和 P9-04（`computer-use/docs/security.md`）显式书写完整路径相比，前缀标准不一致。
- **建议改法**：
  统一补齐 `computer-use/` 或 crate 前缀，例如：
  - `overlay/README.md` -> `computer-use/crates/gcu-overlay/README.md`
  - `docs/script-sandbox-review.md` -> `computer-use/docs/script-sandbox-review.md`
  - `docs/tool-profiles.md` -> `computer-use/docs/tool-profiles.md`
  - `docs/cli.md` -> `computer-use/docs/cli.md`
  - `docs/browser-extension.md` -> `computer-use/docs/browser-extension.md`
  - `docs/compatibility.md` -> `computer-use/docs/compatibility.md`

---

### 不一致项 4：P6-07 已合入子 PR 在任务表行内尚未回填分支
- **文件与行号**：`docs/plans/computer-use-v2/STATUS.md:72`
- **现状分析**：
  P6-07 对应的核心评测框架（PR #145，分支 `cu2/P6-07-ab`）、真实驱动与夹具装配（PR #159，分支 `cu2/eval-real`）以及代理透传（PR #160，分支 `cu2/eval-proxy`）均已合入基线，行 140 与行 152 也详细记录了进展。
  虽然按 08 规程第 8 条，全卡 DoD 未经真实模型实跑验收前，主状态保持 `todo` 完全合规（“拆卡仅最后子 PR 置 done，其余保持 in_progress/todo”）；但在第 6 列分支中留空，未像 P0-08 那样登记已合入的子 PR 分支（例如 `cu2/P6-07-ab, cu2/eval-real`）。
- **建议改法**：
  在 P6-07 行的分支列登记已合入分支 `cu2/P6-07-ab, cu2/eval-real`，状态保持 `todo`（或按规程调整为 `in_progress` 并注明已合入框架，待真机真实模型运行）。
