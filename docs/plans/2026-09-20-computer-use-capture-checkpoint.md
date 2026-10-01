# Computer Use：截图边界与观察取消检查点

日期：2026-09-20；工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`；HEAD `30757366`；未提交/推送/PR。
总状态仍为 **partial — not releasable**。本批是 C2 增量，不是完整发布验收。

## 实现

- Observe 请求增加必填布尔 `screenshot`，结果图片可空。Host 拒绝未请求的
  图片、请求图片却缺图、坏 PNG、错误尺寸或比例。源码 Host/扩展需同时更新。
- 扩展调用 `captureVisibleTab(windowId)` 前后检查已共享 tab、Chrome documentId、
  URL、活动标签、视口、DPR、滚动、visual viewport 与观察期间的变化序号。
  切走又切回、滚动后复位也使旧结果失效；不主动切换 tab。
- 截图拿到后先复验共享/连接状态；撤销后不再为该截图注入元数据脚本。
  缺 activeTab 时返回拒绝，不静默把无图结果当作截图成功。
- 原始截图限制 24MiB data URL、8192 单边及 16,777,216 像素；使用实际
  Chrome PNG 编解码，最大输出边 1280，最多 7 次缩小，base64 最多 400,000
  字节。保持比例；pinch zoom/visual viewport 偏移不支持时拒绝。
  编解码间检查连接/取消状态和 4 秒预算；浏览器原生调用本身不能中途强杀。
- `/cu/extension-result` 单独上限 768KiB；其他 IPC 仍 64KiB。图片解码使用
  独立单任务槽和 blocking job，不占模型槽、不阻塞 listener 的 async 线程。
  Host 先认证待处理请求，锁外解码，再持锁复验完整请求/授权/期限并消费一次。
- PNG 校验包括有界 base64、chunk 序列、完整 IEND、CRC、deflate 完整解码、
  8-bit RGB/RGBA、尺寸和比例。拒绝尾随数据、APNG、未知/文本/profile chunk。
- DOM observer 增加 200ms 单调时钟预算、16384 工作步、4096 扫描节点、128
  祖先/64 标签子节点预算；缓存继承的隐藏状态，避免重复无限祖先遍历。
  单个浏览器 DOM/layout 调用不能被打断，所以这是协作式预算，不是硬实时保证。
  超限诚实返回 truncated；隐藏祖先、密码和输入字段值继续排除。

`captureVisibleTab` 的 API 参数是窗口，不是 document。前后校验和事件序号
可以丢弃可检测到的错目标/过期结果，但不能宣称 Chrome 提供了原子 document
截图。需要真实工具栏授权后的成功图像、焦点竞争、DPI/缩放实机验收，才可
补齐截图 E2/E3；没有加 all_urls/debugger/Cookie 权限绕过限制。

## 本批证据与首败

证据目录：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

- `observation-budget-red.log` 是测试运行时错误：私有 Node 20.18 无法加载
  开发用 jsdom 的 ESM 依赖，未到断言。单元测试改回开发 Node 24.15；实际
  App 浏览器探针仍由 App 的私有 Node 启动，没有换系统 Node 冒充运行时。
- `observation-budget-assertion-red.log` 实际断言失败，证明原 observer 没有
  elapsed-time 预算。修改后预算和深层隐藏内容测试通过。
- `app-capture-discovery-01.log` 为错误 gate 名，未执行测试；02 为探索运行，
  增加的检查项尚未进入父进程 expected list，整体 exit 1，不记为通过。
- `transport-capture-01.log` 为 test helper 可见性编译错误，未到断言；修正
  crate 内可见性后，02/03 的真实 HTTP、Host 图片和 feature-off 测试通过。
- `app-capture-01.log` 失败在 normalizer 测试尝试于 MV3 worker 动态 import。
  产品静态 import 不受此限制；测试改为可信扩展页导入同一模块并实际编解码，
  同时确认 worker 有所需 API。不把扩展页测试称为 captureVisibleTab 成功。
- `app-capture-02.log` 29 项通过，但其中旧 deadline 用例只证明扩展 5 秒
  executeScript 超时。已拆出 script-timeout，并在 Host deadline 用例中同时
  延迟真实 HTTP result 派发，要求实际等到 Host 10 秒期限；不得继承旧用例
  为 Host deadline 证据。

已观察到 Chrome 真正的截图拒绝：loopback host 权限只能注入元数据；
`chrome.action.openPopup()` 虽返回 opened，随后真实 capture API 仍报权限不足。
这不是原生 toolbar 点击，也没有补齐 C1 activeTab 成功验收。

`app-capture-final.log`：App 私有 Node + MV3 + process-wide Broker/IPC 最终
30 项通过、exit 0。所有 Browser API 调用仍是真实调用，测试只延迟其返回或
HTTP 派发，不替换结果、权限或时钟。两级超时单独断言：脚本约 5 秒返回拒绝；
延迟 result 后，Host 自己的 10 秒单调期限释放消费者（从已收到观察的检查点
测得 9,622ms）。最终一万控件的 Host 观察往返 280ms，返回 truncated。

同轮 Chrome normalizer：纯色 1600×900 → 1280×720；高熵图片缩至 419×236，
base64 396,336 字节。检查了实际解码像素、比例及错误 geometry 的拒绝。
这是可信扩展页中同一 normalizer 的实际编解码；worker API 可用性另验，
不能冒充 activeTab capture 成功或完整图像经过 App 生产 UI/MCP。

| 检查 | 当前结果 |
| --- | --- |
| 扩展 Node 全部 5 文件 | 48 passed；`extension-capture-final-02.log` |
| Core transport 定向 | 11 passed；`transport-capture-03.log` |
| Core all-features | 356 passed，0 ignored，31.16s；`core-capture-final.log` |
| Driver 集成 | 12 passed，0 ignored，0.16s；同上 |
| 私有 Node + MV3 + App Broker/IPC | 30 passed；`app-capture-final.log` |
| Core strict Clippy | all-targets/all-features，`-D warnings`，exit 0 |
| App strict Clippy | default / computer-use-probe 两配置，all-targets，`-D warnings`，均 exit 0 |
| Rust fmt / whitespace | passed；未改 Git 换行配置规避已有提示 |
| locale / code quality | 15 locale 一致；final passed，千行文件 80/80 |

本批无前端 UI 源码变动，没有重复跑上一批已通过的前端 69 项/typecheck，
也没有把该旧结果写为本轮新执行。浏览器与测试退出后未发现本轮 private Node、
cu_probe 或 owned-profile Chrome 残留；未处理之前另行记录的遗留临时目录。

本轮浏览器用 `src-tauri/target-cu/debug/cu_probe.exe` SHA256：
`E4A33A4FB0FC7C89E8240E2575F5A596B78ACE1EACCB33712C075B69CA4008DD`。
未冻结发布候选，不将上述局部证据升级为最终版验收。

## 接续顺序

1. 本批最终浏览器、Core/Driver 与 Core/App 两配置 Clippy 已完成；继续
   下列未完成项，不将当前计数当作发布矩阵。
2. C1/C2 缺口：真实 toolbar Share → screenshot 成功经 HTTP 到 Host；实际
   capture 期间的切 tab/导航/撤销；DPI/缩放/窗口移动。当前只验证截图拒绝、
   Chrome 图像归一化、Core PNG/HTTP 图片接收及文本观察的真实竞争。
3. C3：生产 ExistingTab adapter 与 Broker/MCP，保留正确 surface 路由，
   再实现 typed actions、ref/snapshot/document fences、独立副作用后置条件。
   不把数字 tabId 路由到 Desktop；检查通用 Observation 是否能保留可见文本。
   当前 `protocol::Observation` 确实没有 text 字段，不能在转换时丢掉本批
   已取得的文本。Host 当前只返回 ExtensionObservation，尚未提交可供动作
   核验的 refs/snapshot；提交时需区分模型观察与 UI preview，避免预览覆盖
   模型仍引用的 snapshot。新增 adapter 不能照搬 managed 的固定 idle/abort。
4. C4 生命周期/App-shell、C5 安装/Edge/macOS/X11/GNOME native Wayland，
   C6 freeze/真实模型/至少 12h 主动长稳仍全部保留，不能缩减首版范围。
