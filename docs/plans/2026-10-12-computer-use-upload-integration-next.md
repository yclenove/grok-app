# 上传完整接线：下一开发批次

2026-10-12 最新见 [上传 MCP 与原生接线检查点](2026-10-12-computer-use-upload-mcp-checkpoint.md)：
computer_upload 已公开到严格 catalog，上传专属等待与原 App MCP/native 四场景通过；普通 idle 仍 15 秒。
Host approval/file choice synthetic，原 ACP/model/真实 picker/真人和完整交付仍开放。
下方“模型工具/MCP 未接、computer_upload unavailable”是历史状态，不覆盖此次增量。

2026-10-12 增量见 [原 TaskGrant / Host 文件入口检查点](2026-10-12-computer-use-upload-task-checkpoint.md)。
U1/U2/Core 账本与 main IPC/ACP pin、共享任务 UI 已实现并有分层证据；模型工具、真实 picker/
原 MCP 等待/真人与用户新包仍未完成。下方保留完整验收合同，不因局部通过缩小 U1–U4/D00–D31。

2026-10-12。[原下载 error unwind](2026-10-12-computer-use-download-error-unwind-checkpoint.md)
已独立实测；[不可变内容/固定上传内核](2026-10-11-computer-use-webview-upload-checkpoint.md)
只证明 adapter。computer_upload仍 unavailable。本计划不缩小D00–D31完整交付范围。

## 体验与完成定义

用户正常发任务、选择目标；真正需要上传时才在该任务显示Host文件入口，明确目的页面和
共享意图。“选择并上传”可形成该次同意，不要预授权或重复弹通用批准/选文件/再批准三个框。
现有TaskGrant不授予任意本地文件访问。字段赋值verified不是网站已提交成功。
完成链为真实选择/同意→精确不可变内容→原Broker许可/一次账本→原file input→复验→独立receiver。

## U1 原意图和Host选择事务

1. 用原observe的run/target/generation/snapshot/ref和独立actionId；模型不提供路径、URL、
   selector、脚本或文件字节。旧ref/非file input/未绑定页面先拒绝。
2. Broker封存原transport/agent/TaskGrant/feature epoch/ConsentBinding/run generation意图。
   Host命令仅接受该reservation，并核主App WebView/session身份，不开放模型自批接口。
3. Cancel/Stop/Pause/换agent或transport/feature关闭/导航/关闭/重绑退休原意图；迟到picker
   答复不恢复权限。用户选择时看见目的页面，adapter构造值不代表文件同意。
4. 先核实所用跨平台picker的实际取消/关闭能力。不能Drop future或socket就宣称dialog/
   原线程结束；须保留原worker owner/join和App Quit barrier。缺能力继续开放，不伪造stopped。
5. 复用现有request/readback/event/Host continuation设计等待与续行，不让模型无限轮询、
   为上传建新聊天或抬通用超时。断连/迟到选择/新agent不执行旧动作。

## U2 同意后的内容与单次凭证

1. 实际选择后才读原文件handle，有界读取，不由adapter重新按路径打开。当前上限8MiB，
   空文件可用；超限/不可读明确返回，不截断。用户选后替换路径不改变已选内容。
2. 保持HostSelectedUploadFile的filename/MIME/不可变bytes合同；无法确定MIME时用
   application/octet-stream。Debug/trace/诊断不输出内容或原路径。
3. opaque ticket绑定U1原task/action/target/document/snapshot/ref，容量/生命周期有界，
   不接受模型自行构造，不跨task/run/agent/页面复用，不存入共享home。
4. 明确数据释放者和原Host物理操作结束者；退休数据只可用于原操作收尾，不复活ticket。

## U3 Broker、结果账本与模型工具

1. 复用broker/webview_download.rs的原许可、feature/ConsentBinding、暂停停止、snapshot/ref、
   budget/in_flight/guarded publication思路，抽领域模块，不留两份失配的大段复制。
2. fingerprint含原意图/文件内容摘要与必要元数据，不含原路径。同actionId只读原结果；
   冲突拒绝，pending/unknown不重读、不重发。文件同意与TaskGrant同时有效才stage_upload。
3. 最终成功在原Broker/native binding/document/cancellation guard下提交；unknown先重新观察。
   assignment后Pause/Stop不能假装撤回，保留原物理占用到真实结束，拒绝旧成功发布。
4. 接Core schema/preflight/transport dispatch、session MCP catalog、ACP权限和提示规则。
   模型面不含Host路径/任意code，不使其它surface静默回退。App WebView先完成整条产品链；
   其它browser旧path实现不冒充新同意链，分别接入与验收，未支持行诚实unavailable。

## U4 UI、效果、用户包

1. 任务内待选择/选择中/取消/赋值中/未知失败/完成状态与Pause/Continue/Stop一致；
   模型不能代点用户授权。state/UI落领域component/hook/store/API，不增长App/AppWorkbench。
2. 接十五语言、IA、键盘焦点和窄宽布局；复用产品chrome，不堆说明文字。
3. 真实选择/取消、中文/二进制/MIME/空/上限、旧ref/跨task/重复action、选择中断、赋值后
   取消、导航重绑、Pause/Continue/Stop、App/agent重启和Quit，分别有原owner和独立效果证据。
   固定本地receiver核bytes/hash和零重复，不向外网测试站上传数据。
4. 做相关Core/App/Node/UI回归、默认/probe strict、fmt/LF/quality；Windows libtest须Cargo
   唯一executable副本+项目manifest embed/readback。不能拿零匹配测试当覆盖。
5. fresh包用完整新前端/后端/资源，记录源/候选/包hash并回读；复用现有独立用户home，
   核验/退出确切被替换的预览owner，最多一个用户预览。probe EXE不是安装包，app115主页不是新代码验收。
6. 当前旧preview出现account/quota401，不证明新模型任务已跑过。真人验证须用户手动登录，
   不导出凭据/代点OAuth/擅自换账号，不伪造模型验收。
7. 原build/test/browser owners退出后清准确旧副本，保留当前候选/seed/依赖/source/home/
   首败；报告逻辑与空闲观察差值，旧已拒绝清理不换工具重试。

单writer，先读当前源/进程，不恢复旧授权或重启仍活的原验收。U1–U4未闭合不写上传可用。
继续D22、其它平台/surface、真实模型、安装升级回滚、UI UX、12h与分批交付；无新授权不自动提交发布。
