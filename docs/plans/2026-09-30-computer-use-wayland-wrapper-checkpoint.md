# Computer Use：产品 wrapper 与 PipeWire 回收确认检查点

日期：2026-09-30。目标继续 **active / partial — not releasable**。

本检查点不是“所有功能开发完成”，没有缩小原始目标，也没有启用 App native Wayland。
上一轮和本轮均属于 progress：有生产代码变更、先红后绿的确定性回归和真实原生 IPC/像素/输入证据；不是仅重述状态或等待已不存在的进程。

## 本轮实际改动

### 产品 wrapper 不再丢失原生授权上下文

- `computer-use-core/src/owned.rs` 将 run-scoped discovery/claim/release、可信 capture generation、purpose/cancellation 传到原 adapter。
- 取消的新 model capture 必须让原生层撤销旧 ticket；不能在 wrapper 提前返回并保留旧 model 输入授权。preview 不替代 model。
- 配有 private worker 时，预检失败先撤销并退休 native owner，再按相同 run 释放；不将不确定清理当作 idle。
- `owned_ownership.rs` 以 wrapper instance、target 和 run 保存 Claiming/Claimed/Releasing/Retiring 状态。回调不持共享 registry 锁；迟到 claim、外来 release、未完成 release 不能转移授权。
- 保留原 adapter 支持的多个独立 run，不为了通过测试强行退化为单 run。
- 每次 reservation 变化更换 revision。native idle 查询只可退休查询开始前同一 revision 的记录，堵住旧 idle 读数和 release/reclaim ABA 两个已复现竞态。

### 原生回收需要有序确认，不只依赖本地析构

- `computer-use-wayland/src/capture.rs` 在 stream/listener 已销毁后，对**原 portal-granted connection**执行 `core.sync`/对应 `done` 屏障；没有另连默认 PipeWire remote。
- 正常 Stop、源/格式/捕获循环失败共用该清理路径。等待前撤销帧，取消 Stop waiter 不取消原 native owner 的清理。
- 等待有两秒上限。断连/超时仍 join 本地 native owner 并关闭资源，但 `session.rs` 保留失败 CloseReason，不能伪装为远端已确认退休。
- 受控测试暂停自己持有的私有 daemon，旧代码确定性提前返回，修复后等待恢复/确认；另测超时和确认期间 daemon 被杀。只操作夹具持有的 PID，guard 在 unwind 时恢复，不操作用户桌面。
- `PortalSession::stop` 的成功返回仍表示原 owner 已 join；远端成功与失败区别以 `SessionState::Closed(CloseReason)` 表达。deadline 证据明确 `remoteRetirementAcknowledged=false`。

### 测试隔离和证据身份

- private worker 保持生产环境 allowlist，不透传 TMPDIR。旧测试父进程去 `/var/tmp` 查找、子进程实际在 `/tmp` 写入，已用旧 binary 重现并读取真实 PID/side effect。
- `tests/driver.rs` 和 `tests/support/worker.rs` 改用授权 manifest 所在的独立 fixture 目录，继续要求真实子进程写入且 PID 不等于 Host。没有删除 side-effect 断言。
- 两条 Broker 原生回路分别记录 `nativePeerRoot`；独立 verifier 按该身份关联原始 C EIS 日志。两个相同事件数组不能共用一份 peer 证据。

## 红→绿及未掩盖的失败

全部路径相对 `tools/computer-use-probe/.run/wayland-wrapper-20260930/`。

| 问题 | 原始证据 | 修复后证据 |
|---|---|---|
| wrapper 丢 run/generation/ownership | `red3-core-tests.log` 6 fail；`red-native-tests.log` scoped discovery fail | 12 项新增 wrapper contract；最终两端 core 全量通过 |
| stale idle 与 same-run ABA | `idle-race-red.log` 0 pass / 2 fail | `idle-race-green.log` 14 pass（含已有 2 项） |
| worker 两个临时目录 | `legacy-temp-red.log`、`legacy-temp-audit.json`、旧 binary/hash | 最终 Linux/Windows driver 各 16 pass |
| 本地 Stop 早于 daemon 处理销毁 | `retirement-ack-red.log` 0 pass / 1 fail | `retirement-ack-green.log` 5 pass；三个新原生用例纳入最终全量 |

**首次节点快照失败的直接来源仍未完全证明。** `final4-tests-3.log` 为 72 pass / 1 fail：`capture node leaked after Closed`；当时没有保存失败 registry 快照，不能反向声称已看到其后节点消失。20 次单项和 10 次完整诊断未复现，不能当作修复。随后以受控暂停 daemon **独立证明**缺少 teardown barrier，并修复此可复现顺序缺口。最终三个回归保留“一次即时 registry 查询必须无 capture node”的断言，不用轮询或第二次通过掩盖首次失败；诊断第二张快照只在失败时保存，不改变断言。

其他失败亦保留：错误的单 run 限制、private `root` 字段编译失败、不同 Cargo feature 组合造成 binary identity 不同、串行 libtest 在 test name 和 `ok` 之间插入 stderr 导致 verifier 错配。后者改为在**下一条 test 之前**确认该记录的 `ok`，不改原日志或测试结果。

## 当前冻结源码与验收

`source-freeze.json`：321 项；前阶段 317 项中 6 项变化，新增纳入 4 项。`freeze.mjs --verify` 检查当前字节不漂移；693 个既有 Cargo package 版本/checksum 不变，没有增加 package。

`final5-*` 是本阶段当前最终回归；`final4-*` 及更早失败/通过记录均保留，不互相覆盖。

| 检查 | 当前证据 |
|---|---|
| 同一个 Wayland executable，线程数 1/4/4 | 三轮各 **76 passed / 0 failed / 0 ignored**；其中 23 个显式 native 用例 |
| 核心完整回归 | Linux **576**；Windows **577** |
| private worker driver | Linux/Windows 各 **16** |
| macOS wait FFI contract | Linux/Windows 各 **21**，**不是 macOS 原生验收** |
| X11 单测 | **49** |
| owned Xvfb / GTK AT-SPI | **19 / 41** 组 |
| 编译质量 | 联合 Linux strict Clippy、Windows core strict Clippy、Windows App library check、workspace fmt、git whitespace 均通过 |
| 独立像素/输入核对 | 自行解码 PNG/crop 像素；Host 13 个事件，direct/wrapped Broker 各 5 个原始 C EI 事件，绑定不同 peer；重复 action ID 不重发 |
| 夹具归档和残留 | 当前最终 **141**；本阶段全部 **1023** 个唯一目录；3402 个必需日志/配置文件验证；缺失 0、owned fixture 进程残留 0 |

`.run/.../receipt.json` 由 `seal.mjs` 在所有门槛通过后创建；Node 和独立 Python 分别验证 SHA-256/字节数。前阶段 receipt SHA-256 保持 `1726ef314f1d6c0b50b3ec629709d061929cbe7e628d0e3df6c3e9208d3cbc39`。
Receipt 的 `fullGoalComplete=false`、`historicalSnapshotFailureDirectCauseProven=false` 不得改成已完成。artifact 哈希证明字节未变，不代替真实系统功能验收。

## 原始完整目标仍保持

以下要求不以本轮较窄的私有夹具验证替代：

1. Windows x64、macOS arm64/x64、Linux X11 **和原生 GNOME Wayland**。
2. Desktop、managed browser、已有 Chrome/Edge、App WebView。
3. App、ACP、MCP 全调用路径。
4. 全输入、中文 IME、clipboard、取消和恢复。
5. 签名 clean install/update/repair/rollback/uninstall。
6. 原生窄窗口、DPI、权限 UI。
7. 真实 Grok E4。
8. **同一个最终冻结候选 12h active soak**。

本轮未完成：App factory 的 run-owned portal registry、持久 runtime/parent handle、权限/锁屏/用户接管/restore、真实 GNOME 自动连接与应用效果、IME/clipboard、完整平台发布/安装/UI/Grok/soak。`native_wayland` 仍 false；没有 commit/push/tag、签名发布、真实安装或用户桌面输入。

下一步应接入 App 端原生 run 生命周期并补齐上述原生权限及恢复边界，继续保留未能直接解释的历史节点快照失败；不能在完成本检查点后宣告全目标完成。
