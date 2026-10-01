# Computer Use R17：运行时回读、真实状态与可读性

日期：2026-09-26，Asia/Shanghai。分支 `feat/computer-use-implementation`；
HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。没有 commit/push/PR。
整体 UI/UX 重设计和完整 Computer Use 最终版目标仍 active。

## 产品问题与修复

运行时 repair / rollback 原来把后续只读状态请求放在同一个 mutation busy latch 中。
原生操作已经返回、状态回读却不返回时，整个设置页仍永久 busy；操作进行中还可能保留
操作前的绿色“就绪”。原生失败又没有独立刷新实际运行时状态。

- 只有原生 mutation 回复能释放 mutation latch；不增加动作超时、不自动重放动作。
- 后续回读采用已有 10 秒只读展示期限和独立请求身份。超时可以手动重读，旧成功/失败
  不能覆盖新请求或解锁新请求；不把 UI 期限当成原生取消、空闲或动作完成。
- repair / rollback 进行时立即退休旧健康状态。失败信息保留，同时独立重新获取状态。
  卸载后迟到的 mutation 回复不再启动回读。
- 新截图审阅发现第二个缺陷：未知的运行时被显示为“不可用”。现在用独立 unknown 状态，
  不冒充组件已损坏或已就绪；补齐全部 15 个 locale。
- 启用开关下面的授权说明改用既有 `--text-secondary`，不会因开关 disabled 而不可读。
  不改变默认关闭、授权流程、权限策略或 Host 能力。

只改领域组件/样式、翻译与验收夹具，没有向 App.tsx / AppWorkbench.tsx 增加功能状态。

## 本批证据

基础目录：`tools/computer-use-probe/.run/x11-atspi-20260926/`。

- 原始 readback 回归 `ui-readback-red.log`：4 failed / 9 passed，保留。
- unknown 状态回归 `ui-runtime-unknown-red.log`：6 failed / 10 passed，保留。
- 当前 `ui-r17-domain-final.log`：**226/226，20 个文件，终态 exit 0**。
  包含设置、目标/会话/UI controller、API、真实扩展外壳与 15 个 catalog。
- 第一轮 compiled-browser **89/89，终态 exit 0**，artifacts
  `ui-redesign-2026-09-26T05-02-41-106Z`，含四个 repair/rollback 真实时间回读超时场景。
  该轮早于截图发现的 unknown 文案与对比度修复，不能当成最终修复后的验收。
- 修正文案/可读性后的最终 compiled-browser **89/89，终态 exit 0**，artifacts
  `ui-redesign-2026-09-26T05-07-54-446Z`，`ui-r17-browser-final.log`，零 page errors。
  15 个设置场景实际测量授权说明对比度：浅色 **4.742**、深色 **6.568**；均不受 disabled opacity 影响。
- `ui-r17-typecheck-reviewed.log`、`ui-r17-fixture-typecheck-reviewed.log`、
  `ui-r17-lint-reviewed.log`：根目录/fixture TypeScript 与定向 ESLint 均终态 exit 0。
- 新中文 320px 深色 timeout、英文 400px 浅色 recovered 截图已复查：未知状态与重试信息
  位置清楚，辅助授权说明可读，无文本截断、横向溢出或按钮重叠。
- 两轮 browser seed 前后均为 `c4430eb59a12744c3a7e8bea7334bc9a4a70bef40d99e4f9ed65fa9384bc88b2`。

## 当前原生复验

重新核对 R16 PID 56096、原始创建时间和精确 exe 路径后，仅选择其唯一返回窗口。
首次 AX-only 点击报 `coordinate input geometry is unavailable`；完整重绑定并获取截图/AX 后，
单次重试成功进入真实设置路由。没有开启 CU、授权控制、导入账户、修复、回滚或删除。
这表明本次工具输入通路恢复，不证明 R17 新界面已验收。

R17 独立冻结源码为 `H:/aicoding/cu-native-r17-20260926/source`，3471 项逐文件哈希核对。
新的 identifier、构建目录、App/agent home 与旧 R16/用户 App 隔离；普通复制缓存，不使用
硬链接或 mirror/delete。R17 在 13:26:12 +08:00 原生 debug/no-bundle 构建终态 exit 0，
13:26:45 启动独立 PID 59428，随后再次核对创建时间和精确 exe 路径。
exe SHA-256 为 `76BE6288911945FA0417A4429087830AA499B60E75D877FD851101683B322FAD`。
构建仍有既有 chunk/linker warning，不宣称无警告；资源审计 444 files、0 hits、0 links。

`tools/computer-use-probe/.run/ui-native-r17-20260926/` 保存实际 build/launch receipts，
以及 `native-ui-observations.json`（只记录窗口、AX 文本、截图尺寸/标识和结论，不导出截图 payload）。
当前源码的实际 WebView2 窗口已验证：首次导览跳过 → 设置 → 扩展 → 使用电脑；
中文深色/浅色 1202×802 布局、可读授权说明、诊断/维护折叠、实心危险操作确认框。
Tab 后可见焦点环落在“取消”，Escape 关闭确认框；没有执行确认、删除、repair 或 rollback。
AX focused_element 一直报文档，故不能把它当作精确焦点证据，键盘结果依据可见焦点环和关闭结果。
隔离 settings.json 复读仍为 `computerUseEnabled=false`、`sessionDataMode=independent`。

边缘拖拽未改变 1202×802 尺寸；Alt+Space 后一次观察 accessibility 为 null，随后拖拽报告
`Computer Use helper already has an active request`。重新选择唯一返回窗口后只读观察恢复，
未重放输入；本批窄窗原生验收明确 **未证明**，不能套用 R15 的旧 exe 窄窗结果。
新私有目录的真实 Host 报六项 `missing_file`，页面正确显示未准备运行时而不是绿色就绪；
未通过 UI 安装/修复运行时，不把这一检查当作 CU 控制功能可用证明。

四个新场景覆盖中文深色 320px、英文浅色 400px 下 repair / rollback：操作 pending、
状态回读 timeout、手动 retry 与旧回复，明确只发生一次 mutation、零启用/授权调用。
新增可读性检查读取实际渲染颜色并计算授权说明对比度，接受阈值 4.5，不对 disabled
按钮强行套用正文规则。浏览器测试使用私有可写 Chromium 副本；不直接运行或修改 seed。

**证据边界：** compiled product React/CSS + 实际 Chromium，Host IPC 为 fixture mock。
这些浏览器结果不证明原生 repair/rollback、安装版、实际模型或其他系统已通过。
Windows 原生新设置页的上述有限只读/取消/外观场景另有 R17 当前 exe 证据；窄窗与授权控制仍开放。
运行中的 R16 exe 早于本批修改；不得移用其旧原生证据。没有替换或关闭用户 App。

## 后续完整范围

继续新源码原生 UI、缩放/键盘/权限矩阵和完整平台功能。Windows/浏览器退出长尾与发布问题、
macOS AX/完整输入及签名运行时、X11 完整 IME/选择语义/实际 WM 与不确定动作恢复、GNOME
native Wayland、WebView 物理取消和关闭性能、正式 Chrome/Edge 扩展、安装/更新/回滚、
真实 Grok E4 与冻结 12 小时 active soak 均未由本批证明，最终版不宣称完成。
