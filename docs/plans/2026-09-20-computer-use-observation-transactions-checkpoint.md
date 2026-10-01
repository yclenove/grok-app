# Computer Use：观察/动作事务检查点

日期：2026-09-20（Asia/Shanghai）。工作区：`H:\aicoding\grok-app-computer-use`。
分支：`feat/computer-use-implementation`；HEAD：`30757366`。
总状态：**partial — not releasable**；本批是修复进展，不是最终版验收。

## 首败，不得改写为通过

`observation-transactions-red.log` 实际编译并进入断言：5 项中 1 pass / 4 fail，
exit 101。通过的是另一个 run 不应被慢观察阻塞；失败如下：

1. 同一 run 的两个模型观察/预览可以同时进入 adapter，晚返回覆盖模型快照。
2. 观察尚在进行时，使用旧快照的 click 实际执行并返回 Verified；重新授权也
   没有观察占用保护。
3. pending capture 的 Stop 错报 Stopped，而不是 StopRequested。
4. 失败的模型观察不计预算，可以无限重试；UI preview 原本不消耗预算是正确的。

## 本批实现

- 将观察实现从 Broker 主文件拆到 `broker/observation.rs`。共用每 run 的
  in-flight admission，覆盖采集开始、adapter 返回、校验、最终快照发布。
  重叠调用立即 typed LeaseHeld；不持有全局 Broker 锁等待 adapter，不阻塞其他 run。
- 模型观察在准入时扣预算，成功、失败及 unwind 都占一次；忙碌拒绝不扣，
  UI preview 不扣。RAII 释放异常路径，Stop/暂停/feature-off/恢复后的晚帧不发布。
- `prepare_authorize` 先拒绝当前有操作的 run，保留提交时的复验。
  这不等于已经验完所有并发授权的 provisioning/rollback 竞争。
- 原生动作使用 caller + worker 共同持有的 RAII guard。caller 必须完成账本
  写入，worker 必须物理退出，两者均结束才释放。timeout 不提前放开，Unknown
  actionId 不重放。同步 typed Wait 只能继承自己的私有 permit 进行内部采集。
- 浏览器动作的 `finish_browser` 不再手工清除 in-flight，交由外层 guard 释放，
  防止提交与 guard Drop 之间的新操作被旧 guard 误清除。
- 模型 `browser_observe` 不再直接绕过 Broker 调 Host；接入同一互斥、预算、
  cancellation 和提交检查。成功后失效旧 generic snapshot，不能把两套工具的
  refs 混用；成功的新模型观察才解除 Unknown。未增加任何目标授权。
- Tauri UI preview 的忙碌结果为 typed nullable response（`null` = 本轮跳帧）。
  UI 保留上一帧并标 stale，按原 1 秒间隔继续；不显示 lease held 错误闪烁。
  真正的 adapter 错误仍显示。隐藏/卸载/切 run 仍隔离旧异步结果。
  未增加 UI 文案、locale key 或 App shell 状态。

## 本轮验证

证据根目录：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

| 检查 | 本次结果 | 日志 |
| --- | --- | --- |
| 初始 5 个竞争回归 | 5 passed | `observation-transactions-green-01.log` |
| 扩展确定性竞争回归 | 8 passed | `observation-transactions-targeted-03.log` |
| 完整 Core | 372 passed，0 ignored，31.17s | `observation-transactions-core-final.log` |
| Driver integration | 12 passed，0 ignored，0.17s | 同上 |
| Computer UI/API/domain | 12 files / 59 tests passed | `observation-transactions-ui-01.log` |
| Core strict Clippy | all-targets/all-features，`-D warnings`，exit 0 | `observation-transactions-clippy-core.log` |
| App strict Clippy | default + probe 两种 all-targets，`-D warnings`，exit 0 | `observation-transactions-clippy-app.log` / `observation-transactions-clippy-probe.log` |
| 前端 typecheck / changed-file ESLint | exit 0 | `observation-transactions-typecheck.log` / `observation-transactions-eslint.log` |
| Code quality final | PASS，千行文件 80/80，App shell 无增长 | `observation-transactions-quality.log` |
| 重建 cu_probe | exit 0，1m06s；既有链接器“正在创建库”stdout 警告仍在，未禁用 lint | `observation-transactions-probe-build.log` |
| 实际 source managed preview | exit 0；text-only → PNG preview → 旧模型 ref 真 click；独立 oracle=2 | `observation-transactions-source-preview.log` |
| 实际 seed managed preview | exit 0；相同真实后置条件，明确使用生成 worker，不回退源码 | `observation-transactions-packaged-preview.log` |
| 私有 Node/MCP + App Broker/IPC + MV3 ExistingTab | 34 passed，exit 0；Stop 取消 pending observe 且 tab 保持打开 | `observation-transactions-existing.log` |
| seed 完整 check | exit 0；manifest/tree 与上一批完全一致 | `observation-transactions-seed-check.log` |
| fmt / whitespace | exit 0；保留 Git 既有 LF→CRLF 提示，未改变全局配置 | 本轮命令输出 |

原生 action commit-gap 测试不是靠 sleep 猜时序：adapter 进入后持有 Broker
账本锁，让 worker 完成并归还它的 adapter Arc，检查 caller 尚未发布结果时
in-flight 仍为 true，再释放锁并验证唯一动作后置条件。浏览器结果发布另有
独立 guard 存活测试。超时旧测试改为“未 quiesce 时观察被拒绝”，仍保留
零第二次动作、StopRequested 与同 actionId 不重放的断言。

测试覆盖释放/错误/预算/晚帧；不代表已经完成所有 surface 的实机竞争矩阵。
前端为 jsdom + typed invoke 检查，不冒充 Tauri UI 实机点击。

ExistingTab 本轮 Stop 取消测得 6ms，仅是该隔离 fixture 结果，不是通用 SLA。
此链仍包含“没有 toolbar activeTab 手势时正确拒绝截图”，不能把拒绝测试写成
截图 happy path 成功。source/seed managed 测试实际使用私有 Node 20 与 seed
Chromium；它们不代替安装版、Edge 或跨平台验收。本批没有修改 Node worker，
没有重新生成 seed，也没有声称上一批 83/54 个 Node 测试是本批新执行的结果。

cu_probe SHA256：`CE2F82F7DF7C7A7AA0ADB9C3A3C552D5987AB9F94417CF6AEAA1B44E87607C37`。
seed manifest：`32d02879416175cd612b971c431933d03e2405a136ac79f75d94da52343896bb`。
seed tree：`e39e6db9b770bf21b255366ed98a31342a221cff551b17d24891db0b2178a630`。

本批 13 文件局部指纹：
`1A31AFFD317D5AB40D193A05EA1E496F596B48F55CB727A701F7DB3B28F46666`。
范围为 Core `broker.rs`、`broker/{observation,actions,browser_dispatch,
tests_observation_transactions,tests,tests_postcondition,tests_browser_tools}.rs`、
`tools_browser.rs`，App `commands/computer_use.rs`，前端 `api/computerUse.ts`、
`useComputerPreview.ts` 与其新增 test。仓库相对路径以 `/` 分隔排序，每行
`path + 空格 + 大写 SHA256`，LF 拼接、UTF-8、无末尾 LF 后 SHA256。
该指纹范围不同于上一批 228 文件，不可作直接前后对比，也不是 C6 候选冻结。

最终检查 HEAD 仍为 `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`，index 空；
未发现 cu_probe、本轮私有 Node 或 owned-profile Chrome 进程残留。

## 仍需接着做

1. **真实浏览器 Wait**：当前 Wait 把旧 elementRef 与新观察的 ref 做字符串
   比较；真实 managed/ExistingTab ref 会按观察重新生成，Fake 稳定的 n1 不能
   证明它工作。还要统一 `computer_wait` 工具的完整事务及失败等待预算，不能
   通过 model-side preview 无限避开模型观察预算。先写真浏览器失败用例，设计
   同一元素身份/文档寿命的 typed wait，不用模糊 name 匹配替代目标身份。
2. managed 旧 model handles 的 action-aware 回收；不能 invalidate 时销毁
   已准备、尚未执行的 action handle。
3. managed loopback HTTP 进行中的取消仍可能等 15 秒；本批只保证 Broker
   停止状态诚实和晚帧拒绝，没有缩短该网络等待，也不声称 Stop 已物理 quiesce。
4. 完整并发授权、profile open/launch 与跨 profile PID 归属后续继续审查。
5. ExistingTab typed actions 仍 NONE；真实 toolbar activeTab/截图 happy path、
   C4 两轮 App shell/ACP 生命周期、C5 安装/Edge/macOS arm64+x64/Linux X11/
   GNOME native Wayland、C6 真实模型及同一候选 ≥12 小时主动 soak 全部保留。

不碰日常浏览器/账号/代理，不提交、不推送、不提 PR，feature 默认仍关闭。
