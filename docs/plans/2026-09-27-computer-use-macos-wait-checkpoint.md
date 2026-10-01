# Computer Use — macOS retained Wait / product Host forwarding checkpoint

日期：2026-09-27（Asia/Shanghai）；完整目标 **active**。

本批实现 macOS 精确原生对象的只读 Wait，并修复产品 Host 包装器丢失运行级观察语义的问题。**不是最终版完成，也不是 Mac 原生验收通过。** 分支 `feat/computer-use-implementation`，基线 HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`；保留已有 dirty/untracked 工作，不提交、推送、替换用户 App/运行时或操作权限提示。

## 两项真实产品缺口

1. Broker 的旧 Desktop Wait 通过重新观察并比较 `node_ref` 字符串等待名称。Mac 每次观察生成新的 `mac-ax-*` 不透明引用，因而原先已观察的控件无法通过该路径匹配；按相同名称或坐标找另一个控件也不能证明身份。
2. 产品 `platform_adapter()` 实际使用 `HostOwnedAdapter`。该包装器没有转发 `capture_for_run`、保留引用 Wait 策略和运行级释放；默认路径调用 Mac 的 legacy preview 观察，丢失 model/preview、run、截图策略和取消边界。仅直接调用 Mac 适配器的测试不足以证明产品调用链。

## 实现与不变量

- 新增 `macos_adapter/ax_tree_wait.rs`：保留原模型快照和原 AX 对象，逐轮复验进程出生身份、窗口实例、整条 AX 祖先链/成员关系、角色、保护状态、可见性、几何与显示修订；名称可以变化，对象身份不能变化。
- 可见节点公开只读 `wait` 能力；禁用或未聚焦但仍可见的控件可等待。隐藏/零尺寸/受保护控件不能获得该能力，坐标不冒充语义等待。
- 只读名称查询，不读取 `AXValue`，不重新抓图、不按名称/位置替换引用、不抢焦点、不派发输入。成功为 `applied=false / verifiable=true / postcondition_ok=true`。Broker 延续既有 Wait `Verified / executed=true` 的尝试记账语义，不表示发生了输入变更。
- 参数仅 `nameEquals / timeoutMs`；默认 2000ms，允许 1–10000ms。每次查询前后、匹配返回前检查取消/当前快照/期限；25ms 轮询间隔受剩余时间约束。过期后返回的匹配不能成为成功。
- 单轮 AX 查询预算同时受请求期限和既有 2 秒预算约束。原生 AX 单调用超时约束不等于可强制抢占；**不宣称包括 Host worker 准入在内的全链路硬实时期限证明**。
- 超时不替换模型快照；预览不获得或取代模型授权；Stop、释放或新模型观察令旧授权失效。既有未知输入占用仍保持隔离，Wait/Stop 不强制清空或重放。
- Core 新增默认关闭的 `wait_uses_retained_reference()` opt-in；Mac 开启，Host 如实转发。其他 Desktop 原策略、浏览器已有策略及输入动作验证不被替换。
- `HostOwnedAdapter::capture_for_run` 保留 worker 准入和 target 回读校验，向内层传递原 run、完整 CaptureOptions 和取消信号；`observe_for_run` 明确请求模型观察；运行级释放传给内层后释放外层登记。

## 先失败的证据

证据根：`tools/computer-use-probe/.run/macos-wait-20260927/`，失败日志不覆盖。

- `red.log/.exit`：初始 3 项 Wait 合同中两项失败，证实操作缺失；不是先把能力置 true 再宣布完成。
- `green-initial.log/.exit`：首次实现因 Rust 可见性 E0364/E0603 编译失败，修正模块导出范围；之后 `green-r2` 3/3、`expanded-r1` 16/16。
- `host-wrapper-red.log/.exit`：真实 Host 包装后的 21 项 Wait 合同中三项失败：包装器策略转发、Broker 正向等待、`computer_wait` 工具正向调用。修复后 `host-wrapper-green` 的 Capture 33、Value 14、Wait 21，共 **68/68**。
- 第一轮双宿主全量 `windows-final / linux-final` 在 Capture 32/33 处失败：旧断言仍要求根节点无动作。改为精确只读 `wait`，并同步 Value/AX 的动作列表；隐藏/零尺寸与输入拒绝断言仍保留。旧失败不是 Mac 原生失败，也没有用删除测试或放宽原生输入验证掩盖。
- `driver-green-r1` **16/16**：新增四项真实 owned 子进程合同，验证 worker 收到真实 run、图像策略、预取消不联系 worker、错误身份/未执行拒绝不进入内层、已抵达 observe 后取消会终止并回收 owned worker，且不观察原生目标。该 Rust 子进程仍是协议 fixture，不是发布版平台 driver。

## 验证范围

| 项目 | 当前结果 | 证据边界 |
| --- | --- | --- |
| Windows Core + 10 Mac 合同 + driver 合同 | `windows-final-r2.log/.exit`：**781/781、exit0** | 561 Core + 204 Mac 合同 + 16 driver；没有 ignored/filtered |
| Linux 同范围 | `linux-final-r2.log/.exit`：**780/780、exit0** | 560 Core + 204 Mac 合同 + 16 driver；WSL Debian，不是 Mac SDK/真机 |
| Core strict Clippy | 双宿主 `*-clippy-final-r2.exit`：**0** | all-targets、test-support、`-D warnings` |
| 格式 / Shell / Node 语法 | `format-final-r2 / shell-syntax-r2 / node-syntax-r2`：**exit0** | Rust workspace 与原生 runner/管道 fixture |
| 原生探针构建 | 双宿主 `*-probe-build-r2.exit`：**0** | 非 macOS 构建，不是 Swift SDK 编译 |
| 实际运行非 Mac 探针 | 双宿主 `*-not-run-r2`：**not_run、exit2、0 passed / 29 required** | 没有伪造或跳过原生门禁 |
| 原生 Wait 夹具 | 门禁从 26 增为 **29**，双宿主验收器合同各 **24/24** | 实际 Mac 执行仍 NOT_RUN |
| 测试冻结范围 | `source-before-r2.json / source-after-r2.json`：**246 文件零漂移** | 选定源码/依赖/CI/夹具，不是整个工作树 |

完整 runner 返回后才增加 CI 的必需 `--features test-support --test driver` 步骤，覆盖三宿主而不静默跳过 Windows。`ci-contract.log/.exit` 对 YAML、matrix、命令、条件和不可忽略失败做结构断言，**exit0**；远程 CI 没有执行。`source-current.json` 和 `final-audit.json` 另外核对：相对冻结回归后的 246 项，仅 CI 配置这一项变更，其余 245 项不变；不把 CI 后续改动谎称为整包零漂移。新增步骤运行的 driver 16 项已包含双宿主本地完整回归。

Wait 的 21 项 FFI 合同覆盖原对象名称变化、跨轮变化、替代对象相同名称、超时保留、预览/无图/跨 run/目标/取消、失效快照、祖先/保护/权限/几何变化、getter 内撤销、迟到匹配、未知占用、Broker 预算/去重、工具会话绑定与 Host 模型/预览/运行级释放。它们链接生产 Rust Mac 模块，但 Quartz/AX 是 doubles。

## 原生验收扩展与未完成范围

- owned Cocoa 门禁新增名称匹配、名称未达超时、旧快照拒绝；同时验证原应用文本、选区、键/点击/指针状态和事件历史不变。超时后原引用仍可使用，新模型观察后旧引用拒绝。私有管道没有增加条件注入命令。
- 新夹具门禁只是可执行验收入口；**Mac arm64/x64 SDK 编译、29 项真机执行及 PNG 核对仍 NOT_RUN**。新三项也不覆盖原生延迟名称/Stop/控件替换竞态，须继续验收。
- 本批 worker 集成覆盖真实进程/管道上的测试 peer；发布平台 driver、App/ACP/MCP 全链路及其他 OS 原生回归未因这些合同而完成。
- 选择替换、自绘/无 AX 控件、剪贴板、中文 IME、多应用/并发输入、原生完成证明和完整恢复仍需开发与验收；未知占用不能以强行置 idle 解决。
- Windows 偶发种子发布拒绝访问、浏览器导航/PID/退出长尾根因保持开放；本批通过不自动关闭历史故障。
- 完整范围不缩减：Windows、macOS arm64/x64、Linux X11 与 GNOME 原生 Wayland；Desktop、托管浏览器、既有标签页、App WebView；App/ACP/MCP；签名安装/升级/回滚；重设计原生 UI 的窄窗/系统缩放/权限矩阵；真实 Grok E4；冻结源码 **12 小时 active soak**。

本批有真实实现和红绿证据，属于 progress；未把状态复述或未执行计划计为进度。完整目标保持 **active**，未调用完成、暂停或阻塞状态更新。
