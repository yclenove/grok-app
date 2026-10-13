# 原截图未结束时的一次 Continue / 等待中 Stop

2026-10-05；单 Codex writer；分支 `feat/computer-use-implementation`。
本批只补原链验收，没有改生产权限、Core、应用或扩展，不 commit / push / PR / 发布。
当前实际应用仍是 [source108/app109](2026-10-05-computer-use-completion-app109-checkpoint.md)。

## 已核实的结果

source111 两项独立 native 联测实际 **2 passed / 0 failed / exit0**，40.06s。
它们使用原 session MCP、原 App/ACP publication、认证 HTTP、原 MV3 worker 和真实原生 PNG。
Host 手势与 ACP peer 是 synthetic，浏览器 action 使用 owned CDP；不是 installed App / 真人工具栏 / 真实 Grok 的恢复验收。

| 场景 | 原 PNG 仍 held | 释放原 PNG 后 |
| --- | --- | --- |
| Pause → 一次 Continue，重复点击 Continue | 原 admission 保留；同一个恢复 worker/reservation；零新读取、输入和 ACP 字节 | 原 observer/watch/settle 实际 join 后才有一次新恢复读取、新 generation；一次 continuation、一次点击；重复 action 不增加点击 |
| 上述等待中 Stop | 立即撤销恢复授权；停止不依赖释放旧 PNG | 仅原 cleanup；零新恢复读取、输入和 continuation，无自动复活 |

两项都证明原 PNG 退休后 decode=0、不发布旧图/正文成功，旧引用和旧 continuation 不恢复权限。
原 worker、Continue/Pause waiter、浏览器、Node 和 native fixture 均由原 owner 收尾；三个 Share 保留，B/C 无业务动作。
本批没有启动用户 App；此前 app109 真实中文备注/3次计数/明确完成的证据保持有效。

## 身份和门禁

证据根 `E=tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。

- source111：3325件，manifest SHA-256 `34c054e01044df1b6bc01650c17cef02b0e5a3696464ed29aba515b4c55fb463`。
- 默认 App lib 测试 EXE SHA-256 `a784000b28236df51ea35d696588cdbc7ff657748b5566da6e8da6f3d0d9a42c`。
  source111 仅改 source110 的外部 JS probe 字段，实际 cargo no-run exit0 复用相同编译产物；不声称重新编译 Rust。
- 原 CFT154 SHA-256 `f0b204ce7239e221f84c4030cd38e4b688ad05320acc8373da166f548bf57eb5`，真实版本 Chrome/154.0.8037.92。
- App default/probe strict 实际 exit0，30.64s / 32.40s；fmt、quality、diff 和四 probe syntax 通过。
  quality/fmt/diff 的保存记录为 source110；source111 只有已 syntax 检查的 JS `status` → `kind` 修正。
- 最终源码 Verify、app109 VerifyOnly、原两 witness 和所有日志 hash 复核通过。
  验证报告 `E/early-capture-111-result.json`；原定点日志 `E/early-capture-focused-111-1.txt`。
- 生产输入与 source108 的差异清单逐件核实仅八个测试/probe 文件；没有重新打包应用。
  Core/前端全量、完整 native 矩阵和真实 Grok 恢复本批未重跑，不借旧数字标绿。

## 两次首败保留

source109：0/2、exit101、32.69s，止于新版浏览器身份断言。
新场景没有加入原 `ExtensionPeer::spawn` 的 CFT154 选择表，实际选了 CFT140；尚未进入恢复切点。

source110：1/2、exit101、39.18s，等待中 Stop 已通过。
Continue 已证明原 join / 新读取 / 一次 ACP；新 probe 错读 ActionOutcome 的 `status`，应使用公开 `kind`。
source111 修正字段，并保持旧引用拒绝和页面点击计数断言。
原 log、manifest、artifact、失败 profiles/PNG/primary JSON 均保留；详见 `E/early-capture-first-failure-109.md` / `...-110.md`。

## 本批收尾与下一步

清理仅针对 `E/early-capture-cache-111-plan.json` 中核实过期的五个增量编译代，
执行前再次检查原 build/test/browser owner、路径/reparse point 和当前产物 hash。
结果、绝对 targets、逻辑 bytes、实际 H 空闲变化与重建方法记入 `E/early-capture-cache-111-cleanup.json`。
实际完成删除5组，逻辑4772807714 bytes（4.45GiB），H空闲观察增加4184752128 bytes（3.90GiB）；
不是回收站操作，`cargo test --manifest-path src-tauri/Cargo.toml -p grok-app --lib --no-run` / 原clippy门禁可再生成。
当前 app109/ZIP、最新编译代、依赖、immutable seed、登录/会话、源码/dirty/fix.patch 和所有失败证据保留。
不确定的 working、跨 variant 未核实缓存及失败 profiles 继续延期。

已知表达缺口仍存在：原截图 held 时 adapter idle=false，但 Host `cleanupPending=false`。
功能上的一次等待恢复已通过；下一步从原 owned 状态补准确等待回执，不能在 Broker 权威锁内查询慢 adapter。
随后继续安装态 Chrome/Edge、实际 App/真实 Grok 暂停恢复，以及其余表面、三平台/两 Mac 架构、
UI/UX、安装升级回滚与冻结候选12h主动长稳。完整 E5/U04–U09/D00–D31 不缩减，整体仍 partial。
