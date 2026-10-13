# U01 Codex 接手收尾检查点

状态：**核心已接收；产品接线未完成，整体仍 active / partial / not releasable**。
分支 `feat/computer-use-implementation`，基线 `8d8b71c262ab222748bca32412eea884d4b892e0`。
用户要求 Codex 接手实现；batch05 没有启动。本检查点不代表提交、push、PR 或发布。

## 修复与核心合同

- 修正 Grok batch04 的编译阻塞，保留原断言；新增未经 sweep 的申请、准备、grant、续行直接过期回归。
- complete_task fence 同任务全部 incremental run 与尚未完成的 preparation，不只处理第一条 grant。
- 过期队列满时保留原 cleanup 归属，执行权仍立即失效；失败模型调用也交接 expiry cleanup。
- 空闲期限从真实物理 worker 完成开始计。完成信号不获取 Broker mutex，结果未发布时仍保持原 admission。
- unknown outcome 不重放；仅向原 owner 查询实际 idle，原完成信号不能延长新 grant。
- 修正已死目标的错误分类，保留原 actionId 消费与零重放。
- 修正三条 fixture 自相矛盾，按 scope/resources/capacity/idle 拆分超长测试文件；未放宽产品容量或原反例。
- 参数以 ExecutionTargetClaim / WorkTiming 分型，无新增 Clippy 豁免。

核心只有 Broker 内 ConsentState 一份权威。transport token 不授予电脑权限；正式 agent 绑定前无可执行 grant。
grant、最终准入与 continuation 消费都直接核 monotonic deadline，不依赖 timer 先赢竞争。

## 最终候选证据

证据根：`tools/computer-use-probe/.run/codex-u01r2-20261003/`，所有旧失败与旧 batch 证据保留。

| 检查 | 实际结果 |
| --- | --- |
| `cargo test --manifest-path src-tauri/computer-use-core/Cargo.toml --features test-support --offline` | exit 0；647 unit + 220 integration = 867 pass；0 failed/ignored；doc 0 不计成功项；`core-test-support-full-final-1.txt` |
| `cargo clippy --manifest-path src-tauri/computer-use-core/Cargo.toml --features test-support --all-targets --offline -- -D warnings` | exit 0；`core-clippy-final-1.txt` |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`、`git diff --check` | exit 0 |
| 原五条独立反例 | 5 pass / 0 fail，Cargo exit 0；`independent-review-1/review-result.json` |
| 当前核心与独立 snapshot 比对 | 173 源文件、0 hash mismatch；全量复验包含最后两处 idle reset |

独立反例原算法文件字节未改；manifest SHA256 `135232ec3f98d1d7dcbf374aeb42d19d117fd199b7b819e58ac3a0a501b8816a`。
独立日志 SHA256 `bb46bee80caf78ce6766c1754b86b0343c8a6cd7f6dfcf4d16c3e59b1987ce36`。
该 snapshot/result 自此保持不改，后续 API 修改使用新证据。

全部授权、adapter 与独立回归证据均为 synthetic/FakeAdapter，不是真人批准、App 用户链或原生平台验收。
之前 Node MCP 37 项与 quality PASS 属相同核心候选的既有检查；此处不冒领新一轮运行。

## 后续接线顺序

执行 U02 Host/ACP：无执行资源的 request endpoint、精确 App owner/process 绑定、本机 eligibility、冷/暖/new/load/fork 目录、CU 专用权限回执、一次续行账本、到期 watcher与首用偏好事务。
随后 U04 → U03 → U05 → U06 → U07 → U08 → U09；U00–U09 / D00–D31 完整范围不缩减。
当前仍没有生产 Bound 构造器；U04 负责真实四 surface 的范围绑定，U03 负责十五语言请求卡与真实手势。
