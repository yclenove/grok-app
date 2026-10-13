# Grok U03：按需授权卡与 Computer Use 面板体验

状态：**Codex implementation in progress；未派发给 Grok**。U04 App selected-scope 后端已接线，
最新UI接线/retained事实与验收见[UI checkpoint](2026-10-03-computer-use-task-consent-u03-ui-checkpoint.md)：
pending无run已转绿，卡/面板共用source与controller，前端298/Core982/隔离UI37通过。
完整native/其它平台、U04范围管理及U05恢复尚未交付；下方“未接线/仍红”是原阶段基线。
本批新增 U03 typed Host request readback、deny/cancel/原 task Stop 与 API 门禁；证据及未交付
项目见 [Host readback checkpoint](2026-10-03-computer-use-task-consent-u03-host-readback-checkpoint.md)。
显式 Host picker 后端与 typed API 已接线，见 [范围选择 checkpoint](2026-10-03-computer-use-task-consent-u03-scope-picker-checkpoint.md)；
Windows/Desktop producer 已编译，真实 UI/native 手势未验。卡片 UI、共享订阅和交互验收未完成。用户已要求 Codex 接手，单 Codex writer；
本稿不授权 Grok/其它 writer 或提交/发布。执行依赖先 U04 后 U03，原范围未删减。
后续已完成Host Channel/前端共享source基础链，见
[events checkpoint](2026-10-03-computer-use-task-consent-u03-events-checkpoint.md)；
卡/面板没有消费新hook，pending无run真实UI回归仍1 fail。下面接线和验收要求继续执行。
精确 Pause/Preview、typed retained failure 和共享 action controller 基础已实现，见
[controls checkpoint](2026-10-03-computer-use-task-consent-u03-controls-checkpoint.md)。
controller现已挂载；领域及隔离浏览器通过仍不能当真人App/native授权完成。

## 产品结果

普通聊天、启软件、切设置或开CU面板不先弹授权。模型执行任务实际需要电脑时才调用申请，
聊天中出现一张卡：做什么、需要哪些范围、本次任务有效；一键允许或拒绝。
feature-off首用按钮是“启用并允许本次任务”，同一Host事务完成，不能先跳设置开开关。
批准后范围内连续操作不重复确认；面板用于预览、状态、范围管理、暂停/继续/停止。
必要OS权限或浏览器Share仍准确显示为实际手势，不能用App授权替代或自动点击。

本批实现request卡/Host命令接线及UI；U04先提供真实多目标resolver，U05是一键resume内核。
未接的能力如实unavailable，不能挂个可以点但不工作的按钮。依赖不满足时继续其它ready项。

## 当前源码入口与先红测试

- `src/components/computer-use/ComputerTaskCardLive.tsx`仅在status.runId存在时渲染；
  `src/lib/computer-use/taskCard.ts`也只接受已有run/target。先补无run的真实Host pending事件
  能出现卡片的UI回归，旧路径应红，保留原输出。
- `ComputerTaskCard.tsx`当前只提供打开面板/暂停/Stop，无request或continue。
- `ComputerPanel.tsx`先选surface/目标/授权；feature-off入口要求进设置。
- `useComputerController.ts`与TaskCardLive各自800ms轮询，`useComputerPreview.ts`另负责capture。
- `src/lib/api/computerUse.ts`、`src/styles/computer-use.css`、`src/lib/computer-use/`。
- 聊天挂载在`src/app/WorkbenchChatStage.tsx`；资源侧栏在WorkbenchResourcesAside。

状态与决策抽到领域模块/provider/hook，不向App.tsx或AppWorkbench新增state/大功能块。
不要只改旧view predicate让所有status都画pending；请求必须来自当前Host权威记录。

## 1. 一张卡，从申请到执行连续展示

| Host状态 | 默认展示 | 用户动作 |
| --- | --- | --- |
| pending | 目的、请求范围、可读权限、期限 | 允许本次任务；拒绝；折叠范围调整 |
| preparing/OS等待 | 准备中及必要实际系统手势 | 取消/停止始终可达；主按钮防双击 |
| committed等待续行 | 已允许，等待任务继续 | 查看范围/面板；停止 |
| observing/running | 当前任务/目标与状态 | 暂停；停止；打开预览 |
| paused/resuming | 暂停原因或恢复中 | 内核已支持才显示一键继续；停止 |
| denied/cancelled/expired | 简短确定结果 | 无自动重申/重试授权；必要手动新任务入口 |
| failed/stale/unknown | 准确原因和恢复动作 | 原确切任务仍可停止；不把错误画成运行 |
| stop_requested/stopped/completed | 停止中、已停止、完成分别显示 | cleanup待结束不是重新授权；查看必要详情 |

同app/task/request/revision的事件幂等，React remount不重复卡/commit/续行。
回包核完整owner/revision；旧会话晚结果不能覆盖新卡。request无run时也可deny/cancel，
不能用session“当前run”补一个猜测身份。pending请求与执行卡属于同任务显示，不堆多层条。

## 2. 默认内容简洁，范围和技术信息逐层展开

卡片顶部任务名与状态，正文最多目的+范围两段短文。底部主/次决策，更多范围管理折叠。
主按钮使用项目solid，拒绝使用ghost，停止用既有danger；不把所有按钮都做同等醒目。
feature-off首用必须调用U02冻结的Host协调事务，不能只调用core commit、前端改开关或
用旧persist_feature_transition提前attach执行资源。核settings/进程flag/Broker gate在
实际成功后保持一致；persistence失败/Stop/用户关开竞争不能迟到启用，旧整个settings
快照不能覆盖同时修改的其它偏好。偏好enabled仍不是执行批准。增加三处状态与真正
MCP准入的首用回归，不仅assert按钮文字或局部feature bool。
模型purpose/scopeIntent按纯文本展示、长度有界；不渲染它的HTML或当批准目标身份。
普通用户不看token/runId/agent/IPC/backend报表；runtime/trace放折叠诊断，错误先给可操作原因。
期限文案“本次任务”及Host实际TTL，不暗示永久允许；UI计时不决定Host授权是否过期。

范围解析/枚举只能在Host允许的准备阶段发生。开卡/展开不能提前抓屏幕或列所有窗口标题。
身份确定可一键commit；歧义时必要picker说明“选择要操作的窗口”，保留在本次请求流程。
现有窗口picker/系统Share只做真实必要决定，不再跳设置→选目标→授权→手发继续的链条。
权限选择只影响Host申请/批准，不由前端存一个可执行grant。增量拒绝保留原范围。

## 3. 面板重排成预览与控制

默认顺序：任务/状态 → 当前已允许范围 → 预览 → 持续可见的控制栏。
surface/picker、扩展配对、runtime/diagnostics放高级范围管理或相关设置，少量必要提示按状态出现。
feature-off或idle仅展示说明“任务需要时会申请”，不因打开面板就弹卡/调用申请/启用feature。

卡片/面板按钮调用同一Host领域controller；批准/deny/cancel/Stop不各实现一套。
busy不能统一禁用所有操作，Stop优先、准备可取消；重复点击只一个命令事务。
Native stopped与MCP目录cleanup分开，不能cleanup慢就把“停止”误画成继续运行。
新target未fresh observe前不显示旧预览为当前；preview不更新模型snapshot/ref或充当执行观察。
隐藏/切走/未批准/暂停时停止capture；待处理旧capture回包核身份后丢弃并收束。

## 4. 共用Host状态源

U02源码尚无完整 typed UI Host events；本批已实现 task_readback/task_decide，下一批先冻结并
补真实 Host subscription/events，再用 events +有界回读建立每app session领域源，卡片/面板共享。
保留stale watchdog和原任务停止能力，去重800ms双轮询，不靠更短interval掩盖异步问题。
live订阅生命周期明确；卸载只是释放观察者，不撤销任务或丢票；最后订阅者不启动无用capture。
Host重启/agent退休/事件序号变化准确失效，前端不得从旧缓存重新授权。
所有敏感token只在Host/MCP环境，React状态、错误、UI日志不读取/保留bearer。

## 5. 样式、无障碍与15语言

使用现有字体/space/radius/surface/color tokens以及btn/Select/ContextMenu/GlassModal；
重排信息层级和留白，不新建平行主题，不用window.confirm/prompt/alert/native select。
各块min-width:0，长目的/范围可换行，状态+按钮不被长标题挤出；preview保持原比例contain。
300/320px侧栏时按钮可换行但决策/Stop不得裁切；900px窗口与100/125/150/200%DPI下可操作。
light/dark、中文、英文及长德语/俄语检视；长说明默认折叠，不挤占预览。
以内容高度布局，不用固定高度藏错误/系统等待；菜单有实体背景/正确portal层，不透明穿透。

键盘Tab/Shift+Tab/Enter/Escape可完成卡片决策/范围调整/Stop；busy保留取消焦点。
必要modal按项目focus trap与返回焦点，不自动激活用户桌面窗口；aria-live只读状态变化，
pending连续轮询不每800ms播报一次，危险动作可识别。错误文本与状态同时可读。
新文案以en key为权威，同步15locale；新增setting登记settingsCatalog，删旧强制前置授权描述。

## 6. 实际验收，不只mock渲染绿

Host命令/事件联测先验证：pending无run、双批准、拒绝/过期、prepare取消/Stop、晚回包、
并行session、remount、stale、feature-off首用，以及原范围增量拒绝保留。
有意义的UI交互回归验证实际command计数/绑定，不只assert按钮文本；旧Stop/readback测试保留。
测试所有表中状态，未接内核动作不能用mock成功冒充已接线。

在隔离自建Vite/Playwright fixture中用同一候选源渲染真实组件，保存owned截图并逐张检视：
pending首用、准备/系统等待、运行+preview、暂停、stale/error、stopped+cleanup；
窄宽窗口/侧栏、深浅、中文和长语言、DPI/键盘都覆盖，不冒充installed App真人批准。
不操作用户当前浏览器/profile或真实App的授权框；本批fixture决定均synthetic。

验证受影响组件/领域/API测试、pnpm typecheck/lint/build:ui、locale键完整/设置索引/壳大小、
Host受影响tests、diff/fmt/strict Clippy。只有改相关合同才扩大测试，避免重复刷绿填时间。
交`2026-10-03-computer-use-task-consent-u03-checkpoint.md`：实现/验证分开、截图路径与
source/runtime hash、每条真实行为/失败/命令退出码、manual与U04/U05依赖，不能说全平台已验。
Codex接收后再安排U05，完整U00–U09/D00–D31继续保留。
