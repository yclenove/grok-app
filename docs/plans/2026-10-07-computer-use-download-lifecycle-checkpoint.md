# App WebView 慢下载生命周期检查点

2026-10-07；`feat/computer-use-implementation`，保留原 WIP；未 commit、push、PR 或发布。
完整目标仍 active / partial，不能据本检查点宣布 CU 可交付。

## 本批实机结果

固定 probe（Windows SDK mt 后链）位于
`src-tauri/target/codex-test-runtime/webview-download-lifecycle-exit-20261007-233500/cu_probe.exe`。
SHA256：`58DBD6595EECF5976A1FBC847EC7D1FA29781F15A5C6A14A929D854F4BDE89DC`。
一次完整四场景 exit0，原 `lifecycle.log` 保留：

| 操作 | 控制至结果耗时 | 实际下载终态回调 | 原浏览器退出见证 | 清理 / 旧请求 / 新 owner |
| --- | ---: | ---: | --- | --- |
| Pause | 68 ms | 1 | 收尾时确认 | staging 空 / 零重放 / 新下载成功 |
| Stop | 70 ms | 1 | 收尾时确认 | staging 空 / 零重放 / 新下载成功 |
| Navigate | 35 ms | 1 | 收尾时确认 | staging 空 / 零重放 / 新下载成功 |
| Close | 350 ms | 0 | 原环境 BrowserProcessExited | unknown / staging 空 / 同 label 新实例成功 |

服务器发送 1 MiB attachment 的前 16 KiB 后以 Condvar 保持剩余 body；
首段发送、原请求/认领计数、caller 未返回及 adapter busy 共同见证真实在途。
要求 body 尚未释放时就收到真实取消终态，或 Close 的精确浏览器退出见证，
以及 caller 结束和 staging 清空。随后释放 body、join worker，检查旧文件零复活。
全链经过原 transport/request/picker/preparation/TaskGrant/Broker/adapter/native coordinator；
Host approval 仍为 fixture synthetic，未经过原 ACP、前端批准卡或真实 Grok 模型。

每项随后建立新 owner/run；再次 Stop 和重放旧请求不能影响新文件。
Close 使用全新 disposable profile、同 native label 新实例，两个环境都确认真实退出。
其余三项是在同一 native view 上建立新任务。没有固定等待、增大业务超时或伪造完成回调。

同候选还通过原 `webview-download`：两次普通下载、成功/失败重放、错误 identity 零副作用、
超 8 MiB 拒绝发布，network/claimed/finished 全为 3；独立 normal-profile 的退出也确认。
上一批 40/40 属于另一候选，不能将其算作本候选稳定性或长稳证明。

## 发现并修复的真实问题

未修前候选 `E9E0CA3E7EF6FF8D32087812256389E3C0756E3786BBCAF2AC2110DADF83CE56`
在 `webview-download-lifecycle-bound-20261007-233000/`。
Pause/Stop/Navigate 分别 67/113/101 ms 通过；Close 原 BrowserProcessExited 已收到，
但 caller 五秒 deadline 失败、native occupancy 仍 busy。失败日志与 profile 保留。

生产 coordinator 新增 `download/windows/exit.rs`。安装 handler 时保留原 environment 的
BrowserProcessExited observer，核对原 browser PID 后仅退休原 native instance 的 expectation。
下载等待允许实际浏览器退出作为丢失回调的物理证明，返回 `dispatched_unknown`；
不会增加实际 Download.StateChanged/Wry 计数，不会报告成功，也不会仅因逻辑 Stop、
alive=false、导航、关闭请求或业务 timeout 就释放占用。cleanup 仍仅处理确切下载文件。
caller 结果尚未处理时继续保持占用；已 abandoned 的 caller 在物理退出后可退休。

准备阶段还发现探针的组合 target ID 超过 256 字节，被现有 bind 错误文案报成页面变化。
探针改用短 label/tab id，保留 same-label 替换验收；未扩大 target 限制。
首个未嵌 Common Controls manifest 的探针 0xc0000139 未进入验收；后链步骤恢复正常。
上述准备失败不计为下载网络或控制场景结果。

## 回归与代码质量

当前 manifest 后链 harness：
`A6B90155A16A85C307486A561BEAD47EE74C2B9129F3ED7B44D4B518FDB84FED`。
WebView 124 + Host 9 = 133 passed，0 failed；原日志位于同成功候选目录。
新增两项真实语义回归：错误 instance 不退休、精确退出保留 unknown、迟到旧退出不伤新操作，
以及 abandoned caller 的物理退出退休；没有伪造 Wry 完成计数。

三个原 ≥1000 行 CU 文件按领域拆开：App native view、STA primitives、导航/动作回归。
下载 coordinator 的原测试也独立到 `download/windows/tests.rs`，避免新修复再次超过预算。
测试数量与行为保留；最终 quality 脚本实际 exit0，≥1000 文件为 80，App+Workbench 13573。
以下三组 strict Clippy 均实际 exit0：App probe-feature lib/tests、App default lib/tests、
probe all-targets；cargo fmt check、git diff check 也实际通过。
最后一处 Clippy 修正仅将首字节 `&[b'x']` 写成等价 `b"x"`，无协议或时序改变。
实机固定候选在该等价字面量修正及 cfg(test) 模块搬迁前构建，133 回归来自搬迁后的源码。
后续冻结/12h 长稳前必须重新构建最终同一候选，不能把本批组合结果当作冻结包验收。

## 当前预览、owners 与产物

全部本批 cu_probe 和 test harness owners 已退出；实机 list_windows 没有遗留测试窗口。
收尾另观察到非本批 Cargo PID7132（23:46:07，`test --lib --no-run --message-format=json`）
仍活跃，归属未明确；不停止它，不清理其可能使用的 target/deps/incremental。
唯一用户预览仍 PID24536，`target/debug/grok-app.exe`，20:23:41 构建。
本批重新激活并实际截图，主页、侧栏和输入框正常，来源 `127.0.0.1:1421`。
它未重建，不能说本批下载修复已在用户当前 App 里。

本批没有再生成发行包。源码、当前可用候选、测试证据、失败 profiles、登录/home 保留。
删除 0，实际回收 0 bytes；上一批自动执行策略拒绝的 obsolete harness/profile 清理仍延期，
没有换 shell/工具绕过。当前成功 harness 副本可由 Cargo 输出 exe Copy 后 SDK mt 后链重建；
成功 disposable profiles 可由 lifecycle gate 再生，已实际确认 browser exit，待允许安全清理时处理。
当前运行的用户 App 与 Vite 属于明确交给用户的预览，不为清理而停止。

## 下一步与完整范围

1. 增 shared/live browser environment 的关单个 WebView 验收，确认仅关闭 view 时的
   exact native download 终态；不能用 disposable 整个 browser 退出推断它已完成。
   同时审查 observer 在长期共享 environment 下的注销/保留边界。
2. 补 timeout/cancel-grace、丢回调以及最后提交与 Stop/navigation 的竞态门禁。
3. 接 upload 与原 ACP/MCP/统一 UI，构建 fresh 用户候选并做真人 Grok 全链验收。
4. 继续原三平台（macOS 两架构、Linux X11/Wayland）、四 surfaces、模型矩阵、
   installed/升级回滚、UI/UX 与冻结同候选 12h soak；完整范围不缩减。

下一批不重复已通过的底层下载工作，也不把一次 fixture 通过当作稳定发布。
