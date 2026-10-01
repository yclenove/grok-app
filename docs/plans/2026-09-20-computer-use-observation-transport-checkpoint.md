# Computer Use：已有标签页观察通道检查点

日期：2026-09-20；工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`；HEAD `30757366`；未提交/推送/PR。
总状态仍为 **partial — not releasable**，C2 部分实现，不代表完整 Computer Use。

## 本批实现

- 新增严格 typed request/result 和认证的 `/cu/extension-poll`、
  `/cu/extension-result`。绑定协议、请求 UUID、递增 sequence、deadline、
  App/connection、session/run、tab、document/grant generation 和 snapshot。
- Host 队列最多 8 项，每 tab 最多 1 项，连接最多 1 项已派发请求。内部使用
  单调 10 秒期限，派发后不重投；结果匹配全部字段后消费一次。队列与授权
  共用锁；撤销/导航/归还/断开/到期后清除或拒绝旧请求，接收者也再次检查授权。
- 长轮询最多 2 秒，每 50ms 重新认证；只允许 1 个活动 poll，不占用模型工具
  的 8 个并发槽。poll 不代替 heartbeat，不延长租约。结果接口限 128KiB，
  其他接口仍为 64KiB；transport 40/800ms 与 pairing 8/800ms 分开限流。
- 配对后 MV3 启动可取消的串行 poll；连接 epoch 改变即停止旧循环。HTTP
  各有 5 秒超时，读取响应仍有上限；空响应也限速，失败结果不自动重发。
- 新增固定 isolated-world main-frame observer。仅显式共享、当前活动且可见
  的 HTTP(S) tab 能读取；注入前后复验 Chrome documentId、URL 和本地共享状态。
- 输出有界可见文本、标题/URL、viewport、最多 64 个控件引用。排除 password、
  hidden/inert/aria-hidden、隐藏祖先、脚本/样式、iframe 内容和输入框值。
  元素引用只保留当前 snapshot 的 WeakRef，不输出任意 HTML 或可执行源码。
- App 私有探针管道可授权 fixture 并请求观察；真实操作经过 MV3 → 认证 HTTP
  → Host 队列。生产 App picker/MCP 的 ExistingTab adapter 仍未注册，未用
  probe 的授权来冒充生产界面已经可用。

## 首败、修复及证据

证据目录：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

1. `transport-route-red.log`：有效测试实际到达断言，旧 `/cu/extension-poll`
   返回 404 而不是认证拒绝。实现路由后对应集成测试通过。
2. 首轮 Node 测试 39 pass / 1 fail：textarea 默认内容被 `textOf()` 当作
   控件名称输出。修复为不读取 input/textarea 文本值，并限制只有 ELEMENT
   节点创建控件引用；隐私用例通过。首败在本任务工具输出，未伪称有独立日志。
3. `transport-tab-id-red.log`：JS 正则隐式转换允许数字/前导零/超安全整数的
   tabId；新增严格字符串、标准十进制和安全整数检查后定向回归通过。
4. 编译测试时误用了不存在的 Result::is_err_or；改为显式匹配结果。Core
   strict Clippy 指出的 filter_map(bool.then) 也已修复，未豁免 lint。

| 检查 | 当前结果与边界 |
| --- | --- |
| Core transport 定向 | 9 passed（含 1 项既有 worker 测试）；`transport-core-01.log` |
| 扩展全部 5 文件 | 43 passed；`extension-transport-final.log` |
| 私有 Node + MV3 + App process-wide Broker/IPC | 21 passed，exit 0；`app-transport-01.log` |
| locale 生成一致性 | 15 locale passed；本批无 UI 新文案 |
| 代码质量 final | passed，千行文件 80/80；非功能发布验收 |
| Core all-features 全量 | 353 passed，0 ignored；`core-transport-final.log`，102.07s |
| Driver 集成 | 12 passed，0 ignored；同日志，0.93s |
| Rust fmt / whitespace | passed；未改 Git 换行配置规避存量提示 |
| 最终 strict Clippy | Core all-targets/all-features，App default 与 computer-use-probe 两配置 all-targets 均 exit 0，`-D warnings` |

真实门禁新增 `typed-observation-through-extension`：fixture 可见中文文本
确实返回，控件 ref 对应 snapshotId，viewport/documentId 非空；password、
hidden、input/textarea 默认值的独立 sentinel 均不在结果内。所有 script
inspection 的 tabId 仍只属于共享 fixture，另一来源的未共享 tab 未被读取。

测试授权通过私有 parent pipe，不是 HTTP 暴露的新授权入口；模型没有新增
authorize 工具。成功来源仍为 loopback fixture，不补足真实工具栏 activeTab
验收，也不是截图、真实模型、动作或安装验收。

本次浏览器测试工件 SHA256（`src-tauri/target-cu/debug/cu_probe.exe`）：
`391F6CB74E171030AFFA49744386A6C67389D8C839E2E40496E6C71CFF9291D9`。
构建后增强了 JS tabId 输入校验并跑定向及全部 Node 测试；没有声称源码已 freeze。
探针结束后未发现本轮 cu_probe/pairing-live/owned-profile 浏览器进程。没有读取
真实账号、Cookie、Token、日常 profile，也未更改代理/VPN。

## 必须继续

1. 本批 Core/Driver/Clippy 收尾已完成；继续下列未完成项，不继承为发布验收。
2. C1 原生工具栏手势：上一检查点的 sky 窗口归属校验仍未通过；没有再次
   操作日常 Chrome，也没有绕过工具的窗口身份检查。
3. C2 截图：只 capture 用户当前共享的目标，前后核对 active tab/document/
   viewport，不能主动切 tab。截图字节需 route-specific 上限与 PNG 验证。
4. C2 生命周期：补真实扩展处理中导航/切 tab/取消/超时、Host 未接受结果的
   独立 oracle，以及待处理请求被停止后迅速回收；不要只靠 stub/Fake 结果。
   大 DOM 场景也需验证扫描时间预算，不能只看 4096 节点/64 refs 的输出上限。
5. C3 生产 ExistingTab adapter/Broker/MCP：观察提交、截图、typed actions、
   node refs、错代动作拒绝、副作用后置条件。检查数字 tabId 的 surface 路由，
   禁止落到 Desktop/managed fallback。未准备好前保持诚实 unavailable。
6. C4 全生命周期/App-shell，C5 安装/Edge/macOS/X11/native Wayland，C6
   freeze/真实模型/至少 12h 主动长稳仍完整保留。没有以“本批通过”缩减目标。
