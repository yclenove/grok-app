# U03 显式范围选择器 checkpoint

状态：**U03 implementation in progress；整体 active / partial — not releasable**。
分支 `feat/computer-use-implementation`，HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。
接续 [Host 请求回读](2026-10-03-computer-use-task-consent-u03-host-readback-checkpoint.md)。
Codex 单 writer；保留既有 dirty/untracked，不启动 Grok/其它 writer，不提交、push 或发布。

## 本批已实现

Host 的明确选择手势先同步预约原 request/selection generation，再开始有界候选读取。
开聊天卡、回读请求、展开说明不调用此入口，不枚举窗口、不捕获图像、不读取 AX/UIA。
`HostScopePickerCall` 和 `HostScopePicker` 留在 Host，保留原 executor generation、原 native
identity、request/connection/task/revision/feature epoch。前端仅收到 picker-local candidateId
和有界 title/appName/kind/surface；不收到 native proof、targetId 或 bearer。

Core 最多发布 128 个候选，metadata 最多 512 字符，选择唯一的 1–8 个原成员；discovery
每个阶段检查原选择状态和 5 秒合作式预算。Windows producer 最多扫描 1024 个顶层窗口，
排除不可见、最小化、不可控、受保护或非原 root 的窗口；读取名称前后复验实例，保持原
process birth/stamp，不激活窗口、不截图、不输入。同步 OS 调用返回前不能冒称已物理结束；
取消会立即 fence，原阻塞 handle 仍由 Host 保留并最终 join。

目前生产 producer 只有 Windows/Desktop；其它平台和 surface 准确 unavailable，没有
legacy list 或整个桌面的替代授权。新增 API **尚未接聊天选择器 UI**，不能据此声称体验交付。

完整链：Tauri commands → SessionManager 原本地 endpoint → ACP 原 RequestTransport →
Host picker → 原 selected-scope reservation → 已有首用批准事务。

- `computer_use_task_picker_begin`：只返回 Host pickerId 和原 key，尚不枚举。
- `computer_use_task_picker_load`：真正有界枚举，可缓存同一原 receipt，不重复 discovery。
- `computer_use_task_picker_cancel`：loading/ready/preparing 均绑定原 key/pickerId；旧关闭为
  no-op，不取消新选择器。此命令的 acknowledgement 不等于后台原生 worker 已退出。
- `computer_use_task_picker_approve`：只接受本 receipt 的原成员，再复验 native identity；
  沿现有 prepare/commit，不新增一层确认，不重新捕获同字符串 replacement。

同 picker 的批准互斥在 native reservation 前取得；第二次批准失败不能 fence 首次准备。
提交成功后相同成员的重试返回同一 outcome，不产生新续行；变化范围拒绝。
关闭已提交的 picker 不撤销 TaskGrant。取消 command waiter、Stop、新 picker、原连接退休
或重连均使原候选失效，晚回包不发布。discovery/reservation 各自的有界 OwnedTasks 在
App/probe shutdown 中 stop/join；command waiter 被取消不会丢失阻塞原 handle。

前端 `src/lib/api/computerTaskPicker.ts` 使用 `isDesktopHost()`；mirror/browser 不调用 Host。
字段为 typed picker/view/candidate/approval，不在 React 或测试输出持有真实凭据。

## 自动化证据

目录：`tools/computer-use-probe/.run/codex-u03-task-picker-20261003/`。
所有决定与 native identity fixture 均明确 **synthetic**；没有真实 App/OS 批准、用户浏览器
或凭据读取。这一批不冒领先前 native station 或 normal CLI 结果为当前 picker 的实机结果。

| 检查 | 结果 |
| --- | --- |
| Core 全量 | `core-full-1.txt`，locked/offline/test-support，726 unit + 220 integration = 946 pass，0 failed/ignored，exit0 |
| 默认 App 构建 | `app-build-2.jsonl/.stderr.txt`，locked/offline/lib/no-run，exit0；实际 build-finished 后取唯一 features=[] artifact |
| App CU | 最终 `app-computer-use-2.txt`，205 pass，0 failed/ignored，四线程，exit0；不是首轮产物冒领 |
| App ACP / routing | `app-acp-1.txt`：147 pass，明确 skip live_handshake；`app-routing-1.txt`：38 pass；均 0 failed/ignored、四线程、exit0 |
| API | `api-1.txt`，两个文件共 12 pass（6 项 picker、6 项 task readback/decision），exit0 |
| 类型/lint/UI | `typecheck-1.txt`、`lint-1.txt`、`ui-build-1.txt` 全部 exit0；既有 large-chunk 提示保留 |
| 质量与格式 | `quality-1.txt` PASS；最终 `fmt-2.txt`、`fmt-commands-1.txt`、`diff-2.txt` exit0；include! command source 另用 rustfmt 2021 格式化/检查 |
| Strict Clippy | 最终 `core-clippy-1.txt`、`app-clippy-2.txt`、`probe-clippy-3.txt` locked/offline/all-targets/-D warnings 均 exit0；App/probe 在 include! 最终整理后检查 |

Core 12 项新增覆盖无预枚举、wrapper 转发与脱敏、foreign key/取消/Stop、旧选择器与晚关闭、
同 target 字符串实例替换、原 producer 禁止替换、subset/重试、非法选择、有界 inventory 和
producer failure。首轮 Core filter 11 pass/1 fail 的原因是测试误以为 registry 允许替换 executor；
实际 register 会准确拒绝 replacement。已改为验证拒绝替换且原 receipt 仍绑定原 producer；
没有放宽产品 registry，不将该测试假设错误冒称产品行为 red→green。首轮原工具输出保留。

App 4 项新增 worker 回归验证取消前零准备、取消中立即 fence、原 discovery/reservation
阻塞 handle 必须真正返回后 join。ACP 7 项验证原 wire/key、脱敏/缓存、旧 picker 关闭、
双批准互斥/幂等、Stop、同 agent 的 reconnect 晚回包及取消批准 waiter。
API 6 项验证原 key/picker-local IDs/Host 门禁/错误传播。
App 的 CU/ACP/routing 过滤集合重叠，不能将 205/147/38 相加当独立测试总数。

`probe-clippy-1.txt` 是错误 package 名导致 Cargo101，未检查代码；保留原日志，正确包名
`grok-computer-use-probe` 的 `probe-clippy-2.txt` exit0，不能把第一次失败隐藏或算作代码通过。

默认 App test harness 构建完后嵌入现有 `windows-test-manifest.xml`，没有改产品 linker。
最终 harness 路径 `src-tauri/target/debug/deps/grok_app_lib-dc611c8f8bc4f678.exe`，SHA256：
`e04c8f6e67a7919807364960ca2224adcf59745d049871593c9d4fbad6264edf`。
`source-files-2.sha256`：git cached + nonignored untracked 的 Core/App/probe/MCP/fixture 与
两个 task API、两个 picker API 文件，Sort-Object -Unique，672 文件；lowercase SHA256 +
两个空格 + repo path，UTF-8 无 BOM、LF 终行。manifest SHA256：
`eb66169bceb5b071f5af5fd72ab448d57d108fe688a6e12e077e12312427cd1e`。
第一份 manifest 使用 Windows 换行且在 include! 最终格式化前；不与最终源清单混用。
这是本批受检子集，不能视为整个产品的发布清单。
本批所有原检查/构建/测试工具句柄均已终态并收集；文档完成后复验672源与最终 harness
hash 一致，没有遗留工作会话。后续源改动归下一批，不能冒用本批结果证明未验代码。

## 后续顺序与完整范围

1. 冻结真实 Host change subscription 合同，再补每 session 的共享领域源；events 唤醒后
   有界回读，无 ambient discovery。卡片/面板复用，移除各自 800ms 轮询，保留 stale watchdog
   和原确切任务 Stop。subscription 取消仅释放观察者，不取消任务/消耗决定。
2. 先保留无 run pending 卡片的真实 UI red，再接完整 pending/preparing/approved/execution/
   terminal 状态、上述 picker、拒绝/取消/Stop 和既有首用事务。批准不是运行；Stop 不随
   approve busy 被禁用，旧 key 不猜新 task 或当前 legacy run。
3. 用项目 tokens/GlassModal 重排卡片/面板，完成 15 locales、键盘/焦点、窄栏/DPI 及隔离
   渲染逐张截图检视；这些不能用 backend fixture 冒充。
4. Windows 可见原生组的 App/native 联测、其它平台 producer、关联 dialog、browser context/
   tab/document、跨 surface、增量/缩小继续 U04；首版三平台目标不因准确 unavailable 被取消。
5. U05–U09、D00–D31、signed installed 生命周期、真实 Grok E4 与同一最终候选 12h 长稳仍
   保留。App shutdown 同步 native cleanup 与 feature_lifecycle orphan worker join 仍需收尾。

本批完成不会把 overall goal 标为 complete，不自动 commit/push/PR/release。
