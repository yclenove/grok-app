# Computer Use：当前标签页分享检查点

日期：2026-09-20；工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`；HEAD `30757366`。
总状态：**partial — not releasable**；目标保持全部功能，不缩减平台范围。
未 commit / push / PR；保留所有存量未提交改动。

后续 typed 文本观察通道的增量记录见
[观察通道检查点](2026-09-20-computer-use-observation-transport-checkpoint.md)。
本文件保留 C1 阶段证据，不自动代表后续源码全量验收。

## 本批行为

- MV3 manifest 增加 activeTab/scripting；保留 storage 和精确 loopback host
  permission。不增加 all_urls、debugger、Cookie 或全标签页枚举。
- Popup 显式 Share/Stop sharing；配对状态与分享状态分开。15 locale 的源文案
  与 Chrome 生成目录同步，生成脚本补齐分享键映射。
- Share 先比对弹窗显示的 tabId，固定 isolated main-frame 脚本读取元数据。
  Chrome documentId 在 offer 前后复验；Chrome 实际返回 32 位十六进制，
  不是带连字符的 UUID。loading/navigation/close 立即取消本地分享；同址重载
  不继承文档身份。这里不是 DOM 观察或动作执行。
- Unshare 期间保留已取消的本地条目；退役完成前不允许重新 Share。
  Host 清除候选和 borrowed grant/观察/预览，不关闭用户 tab。
- Host 每份新 offer 有随机 picker token，App 的
  `existing-tab:{tabId}@{pickerToken}` 在同一锁内校验并授权。旧显示行不能
  授权新文档；真实 tabId 与候选 selector 是不同概念。
- Share/unshare 在 await 前捕获连接，并传入期望 epoch。旧请求不得使用
  新配对凭据。worker 只推送无数据的状态变化提示，popup 重读本地状态；
  已打开 popup 可响应后台续租失败，额外网络轮询为零。

## 首败与修复

证据目录（gitignored）：
`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

1. `app-share-01.log`：分享返回值没有 paired 字段，UI 误设未配对；生成器
   也漏了分享文案键。拆分状态并补映射。
2. `app-share-02/03.log`：把实际 Chrome documentId 当带连字符 UUID 校验，
   main-frame 可见也遭拒绝；脱敏诊断确定形状，修复后 `app-share-04.log`
   的 14 项通过。
3. `picker-generation-red.log`：旧 picker 行错误授权新 document。token
   校验修复后，Core 回归通过。
4. `unshare-retirement-red.log`：网络退役未结束就能再次分享。保留 cancelled
   条目后回归通过。
5. `share-epoch-popup-red.log`：22 pass / 2 fail，分别证实旧 epoch 能借用
   新连接、后台撤销时已打开 popup 不更新；修复后扩展共 34 项通过。

## 当前证据

| 检查 | 结果及范围 |
| --- | --- |
| Core all-features + locked/offline | 345 passed，0 ignored；`core-share-final.log` |
| Driver 集成 | 12 passed，0 ignored；同日志 |
| PairingClient + SharedTabs + popup | 34 passed；`extension-share-final-02.log` |
| 前端配对/API/Panel/i18n | 69 passed（4 文件）；jsdom/mock，不是 App-shell |
| TypeScript | `pnpm typecheck` exit 0 |
| 15 locale 生成一致性 | `prepare-computer-use-extension.mjs --check` exit 0 |
| Core strict Clippy | all-targets/all-features exit 0 |
| App strict Clippy | default 与 computer-use-probe，两配置 all-targets exit 0 |
| Rust fmt / whitespace | passed；存量 Git CRLF 提示未通过改配置绕过 |
| 代码质量 final gate | passed，千行文件 80/80；不代表功能发布通过 |
| 自动 MV3 + 私有 Node + App Broker/IPC | `app-share-05.log` 19 passed；包含不同来源的未共享 tab、显式 unshare、同址 reload、受限页面拒绝 |
| 最新自动门禁 | `app-share-06.log` 20 passed，exit 0；新编译工件增加 metadata await 期间导航取消 |
| 真实工具栏手势 | 未通过，见下节；不算 C1 完成 |

自动成功分享的页面仍在 loopback，依赖已有 loopback host permission。
另一张 `http://unshared.cu.test/fixture` 仅由 Playwright 本地 fulfill，没有访问
Internet。两张 fixture 创建后、点击 Share 前，executeScript 调用数为零；
共享成功期间所有注入仅针对被分享 tab。后续显式尝试分享非 loopback tab
被拒绝，证明把 popup.html 当普通 tab 打开不会伪造 activeTab。

测试对真实 executeScript 的 wrapper 只记录目标/控制异步返回时机，仍调用
原始 Chrome API，不覆写权限或伪造注入结果。私有测试控制管道只查询候选；
没有把候选伪装成生产 ExistingTab adapter 可操作目标。

最后一次重新构建的 `src-tauri/target-cu/debug/cu_probe.exe` SHA256：
`8A202679BD04048B2F26B05EAD9B3B74CB5F03E6380E44F74CE56F21AB06A4D9`。
它随后完成上述 20 项；构建/普通源码检查通过不等于安装包 freeze。
新导航回归先完成真实 metadata 注入，再延迟其异步返回，同时实际导航 fixture；
释放旧返回后 Share 失败，Host 无候选、配对仍有效。没有放宽 document 检查。

## 原生工具栏验收缺口

新增 `cu_probe existing-tab-extension-toolbar`：owner-marked 临时 profile、
Windows Job、私有 Node 和 process-wide App Host。先证明普通 popup tab 无
权限，然后等操作者从真实浏览器工具栏打开扩展，显式 Share/Unshare。
页面 `http://toolbar.cu.test/fixture` 由本地路由提供，不含真实账号或数据。

本轮 `app-toolbar-01.log` 已进入等待真实点击阶段。Windows Computer Use
技能工具找到唯一自建窗口，但 `get_window` 连续两次失败：
`window id 13372512 no longer belongs to Chrome; current owner is Chrome`。
第二次已刷新 list_apps 并尝试只传已返回的 window id，仍失败。未改用原生
按键注入或绕开窗口校验；没有发生工具栏点击，因此不得把这一行记 passed。

核对 PID、父进程、程序路径和 toolbar/owned-profile 参数后，仅终止本轮
Node 37952；父探针 51556 返回 exit 1，并通过 Job 回收 browser/profile。
随后无本轮 pairing-live/owned-profile 活进程，Temp 仅剩上一检查点记录的
`grok-cu-pairing-owned-f605cbe7-3b68-4f76-9191-72bbb5f729b0`；未碰该旧目录。

## 接续顺序

1. 当前 automatic gate 已重新编译并通过 20 项；不必无修改反复重跑。
2. 恢复真实工具栏验收：修复/更换受支持的 UI 工具绑定，或让操作者点击
   专用 fixture。不要扩大 manifest 权限或以注入授权绕过这一缺口。
3. C2 typed transport：协议/有界队列/一次派发/取消及 late result 拒绝；
   先固定 main-frame observe，再真实截图目标复验。路由体积上限按 route
   设置，不能把全局 64KB body limit 简单改成无限制。
4. C3 注册真正 ExistingTab adapter。特别检查数字 tabId 与 Desktop 路由
   的区分，保留 surface；不可把 tab selector/raw tabId 混用。session/run/
   document/grant generation 必须传输和执行两端验证。
5. C4 App-shell 生命周期，C5 Edge/安装/macOS/X11/GNOME native Wayland，
   C6 freeze/真实模型/至少 12h 主动长稳仍完整保留。缺证据不能记已完成。

本批没有操作真实账号、Cookie、Token、日常 browser profile 或代理/VPN。
