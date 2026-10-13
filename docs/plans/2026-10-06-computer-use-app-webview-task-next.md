# D12：内嵌浏览器接入任务授权

2026-10-06。单Codex writer；本批不commit/push/PR/发布。当前D12开发source124，用户试用包app115保持冻结。
第一批[原生选择基础与Windows真实效果](2026-10-06-computer-use-app-webview-selection-checkpoint.md)已通过；
source122的[多绑定与原生最终提交基础](2026-10-06-computer-use-app-webview-authority-checkpoint.md)已验；
source124候选/新run/proof/TaskGrant/后端恢复已接，Windows真实native程序fixture通过，
见[任务转换检查点](2026-10-06-computer-use-app-webview-task-checkpoint.md)。
原ACP publication pin、前端与真人模型仍缺。下方缺口/合同包含历史来源，当前执行入口见最后章节。
本文是剩余计划D12的具体执行入口，不缩减四surface/三平台/两Mac架构及整体交付范围。

## 已核实的缺口

- `acp_client/computer_use/task_view.rs::present_view`只展示Windows desktop与三平台managed-browser。
- `task_picker.rs::begin_computer_use_task_picker`和Core `broker/scope_picker.rs::begin_task_scope_picker`
  只接desktop/existing-tabs。前端`computerTaskPicker.ts`的surface类型和`taskActions.ts::choose`也相同。
- `TaskScopeTarget::parse`的raw路径仍只接受desktop；它不能作为绕过picker的WebView授权接口。
- 现有`WebViewAdapter::bind_side_browser`先绑定session/run/document，然后才能list/observe；
  旧`computer_use_authorize_surface`及`app_shell/webview.rs::production_bind_webview`确实在使用它。
  新TaskGrant不能把绑定前的用户选择与绑定后的执行身份混成同一个可由前端构造的ID。
- 原生内嵌browser的生命周期/固定typed脚本已存在，禁止把外置managed fixture当作D12证据。

## 分批执行

1. **候选与原生身份。** 在原任务key及Host手势下枚举App拥有的当前side browser；冻结原window/view/document
   身份，输出仅picker-local candidateId。枚举不绑定、不观察页面、不生成执行权限；标题/URL元数据要有边界。
   源码要检查side-browser所有权是否真的对应当前App会话；不可只用label前缀推断会话归属。
   当前单bound adapter与多会话任务并存也需明确占用规则：已有owner在原操作退休前不可被新bind替换。
2. **批准、绑定与publication。** 用户选定候选后复验同一原生document，准备原task reservation与run，
   在原App主线程绑定typed adapter，通过原TaskApprovalPin/transport/feature epoch发布同一TaskGrant。
   按原owned-worker规则保存实际调用的占用；取消等待者不等于原调用退出。
   在bind成功但publication失败、Close/导航/会话切换/ACP替换/Stop等切点退出旧绑定，不能影响新owner。
3. **前端接入。** 完成上述真实合同后再扩展picker surface、taskActions与统一候选对话框。
   无内嵌页、页面已导航、绑定忙、主线程调用失败都应有可理解状态；无自动Desktop fallback。
   所有文案走现有15语言键；覆盖层使用原nativeWebviewCover，避免授权/Stop菜单穿透。
4. **执行与恢复。** MCP从原TaskGrant取得执行身份，fresh observe→固定typed action→独立page效果验证。
   暂停需保持业务值，继续先fresh observe，主动Stop后零新效果且记录新终态；旧snapshot/ref拒绝。
   新网页document与run不能借用旧绑定；queued callback必须保持实际operation结束凭据。
   保留上游Google登录handoff导航逻辑，不导出Cookie/Token，不暴露任意eval。
5. **验证后开descriptor。** Windows真实Tauri/WebView2产品链确认成功、拒绝与恢复，才展示可用入口。
   macOS/WebKit与Linux/webkit2gtk必须分别实测；Windows结果不能标记其它平台通过。
   列清尚未支持的导航/history/文件流合同，按D12补齐，不能把click子集标为完整能力。

## 必须提供的证据

- 未批准零bind/observe/input，伪造candidateId/外会话/旧document均拒绝。
- 原Host→picker→reservation→bind→publication→MCP→真实DOM，至少中文输入保存与计数两个独立后置条件。
- Pause保持、Continue产生新效果、Stop保持；一申请、一次保存、无额外重发/重复授权/complete冒报。
- 导航/Close/原App或ACP退出/迟到callback各切点没有旧权限复活或借用新绑定。
- 相关Core/ACP/前端回归、typecheck/lint/fmt/strict/quality，加当前真实默认App构建；不借source115数字。
- 每个新候选先freeze再重编译，源码/包/会话/原owner身份分列。
  原owners正常退出再精确清理，保留失败证据、当前包、seed、依赖与原home。

## 并行状态

source115新的真人任务最终过期；网页0副作用、唯一预览及原子owner已退出。
该包的实时恢复与第二Stop验收仍开放，不用app114结果替代，也不因过期自动重复新建任务。
source118已修改Host选择/旧授权等待边界并通过真实native fixture；该fixture的Host bind仍synthetic。

## source122 已完成的基础

实际 Core proof/final authority callback 和 App 多binding/共享 native occupancy 已接，
84项WebView、15命令、42ACP、Core1110/1126、四strict及质量通过；Windows真实双native页效果通过。
实机 Host bind 为 synthetic，没有 original task picker/TaskGrant/Grok；不能拼接测试宣称整链完成。
原生proof/最终guard/多binding底层已完成，候选阶段与原任务接线仍需落实。

## 下一批：候选与执行身份转换

1. `HostScopePickerCall::discover`目前经adapter拿到`ScopeCandidate`后即调用`recheck_scope_members`，
   其native proof必须能经`scope_identity(target)`取得。当前WebView实现只接受已绑定执行target。
   新候选必须保存原非序列化receipt，并有与执行proof区分的阶段；不能为通过现有检查提前bind。
   候选producer需由真实AppHandle在原主线程取owned child；取消/Stop先保留原worker直到实际枚举退出。
2. `reserve_selected_access_approval`当前把picker member写入request.bound，再调用`start_preparation`。
   `start_preparation`此时才创建新run，而candidate尚不可能带该run。增加明确的原reservation转换阶段，
   在新run分配后通过原receipt绑定，复验executor/native/document及原selection revision，再替换members。
   不修改model/renderer JSON以允许其传入session/run/native身份，不通过raw Desktop parser绕过picker。
3. `AccessApproval::prepare_execution`现仅Managed resources可转为Bound；接WebView候选转换时保持
   原取消/persistence rollback/feature epoch/TaskApprovalPin及owned worker。bind后任一切点失败必须精确清
   已产生的执行target；多个成员部分失败也清全部原新bindings，不调用全局unbind影响另一任务。
4. `HostScopePicker::committed_outcome`现比较原picker targets与grant.members；candidate变成执行target后
   两者自然不同。原请求/selection revision需保存候选成员到执行成员的Host映射，重批同组选项返回原outcome；
   换组选项/旧picker/外owner拒绝。不能按“同request已经approved”直接返回而遗漏所选成员核对。
5. 后端反例包括未批准零bind/observe/input、刷新/同label替换、producer迟到、绑定前后Stop、
   publication前导航/Close、部分组失败、原ACP替换、重复批准与旧cleanup不影响新owner。
   后端实际合同通过后再接task picker/统一selector/15locale/native cover；最后Windows真人Grok验收后开descriptor。

## 后续接线必须保持的底层合同

- 执行proof含完整target/native/document/session/run，最终guard持有同一实际native document。
  不用Synthetic、target_alive、URL/title或锁外一次liveness替代；不在authority callback做native/I/O。
- 多binding按exact native实例保留共享执行占用。旧close/release/回滚仅清原binding，
  caller取消不能提前归还真实占用；部分失败清理也不能影响另一任务或新owner。
- UI入口必须在上述候选/转换反例与真实产品链完成后开放，不增加未使用推测DTO或占位接口。

本批源码/产物需fresh冻结重编译；source122 native fixture可复用验证步骤，不能复用其成功数字。

## source124 后的下一批：原 ACP 与统一界面

候选 producer、原 reservation→新 run/proof 转换、原选择幂等映射、exact cleanup、
组内切换及后端暂停恢复已完成；不重复设计这些机制。source124 native fixture确有 TaskGrant，
但 synthetic Host approval/publication pin 与 Core tool dispatch 不能替代原 ACP/MCP/Grok。

1. **原 ACP 路径。** 在 `acp_client/computer_use/task_picker/tests` 加实际 WebViewAdapter receipt
   fixture，复用原 connection/session/request/key，不用 FakeAdapter 的 Desktop identity 冒充。
   从 exact `app-webview` picker 到原 TaskApprovalPin / feature preference publication；
   同 agent 换 ACP pointer/transport、取消等待者、选定后刷新、部分组失败等切点均精确退休。
   default-off 时不可先 bind；重复同组选项只返回原 outcome，换组/旧 picker拒绝。
2. **统一选择界面。** 扩 `computerTaskPicker.ts` surface 类型和 taskActions.choose，不另建授权页。
   卡片显示 App 已拥有的页面，候选只带 Host 局部 candidateId；不能让 renderer 传 native/run/session。
   无页、关闭/导航、binding busy、queued producer retired均有空/错误/恢复状态，不能自动 Desktop fallback。
   复用现有 Select/dialog/portal，覆盖 nativeWebviewCover；15 locale保持同键，不往 AppWorkbench 添状态。
3. **真实任务验证后再公开入口。** 先测试 UI+真实默认 App 的原 ACP/MCP链与一任务一次批准。
   Windows 真人 Grok验收：中文保存、计数、在两页间切回；Pause保持，Continue fresh观察后新效果，
   active Stop 后零新效果且卡片终态准确，旧 snapshot/ref拒绝，无重复批准/重发/保存。
   此时才开放 Windows descriptor。其它平台未实测不能据Windows结果开放或标为支持。
4. **用户包与全范围交付。** fresh frontend、实际新 App/portable/逐件ZIP验证后替换唯一试用预览，
   复用用户隔离home和登录会话。source115原实时恢复/第二Stop仍开放，不能借synthetic native结果销账。
   继续 macOS两架构/Linux native、导航/history/文件、安装升级回滚/模型矩阵/冻结12h与UI UX计划。

本批先串起原 ACP/界面，按自然切点验收；不要重复基础门禁或用额外授权任务掩盖旧请求过期。
所有新构建保持fresh来源，原owner正常退出后精准清理；不commit/push/PR/发布。
