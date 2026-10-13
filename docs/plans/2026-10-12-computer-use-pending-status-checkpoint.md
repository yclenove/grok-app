# 授权等待期间的状态查询修复

2026-10-12；`feat/computer-use-implementation`。完整交付保持 active / partial：四 surface、Windows/macOS ARM+Intel/Linux X11+Wayland、installed 升级回滚、UI UX、真实模型矩阵、冻结 12h 和 D00–D31 均未缩减。

## 实机发现的断点

实际普通入口 app132 / Grok4.7 / 极高会话“内嵌网页与上传验收”在申请后查询 `computer_status`。原申请两次返回相同的 pending request/revision；查询状态则在未有 TaskGrant 时被 ACP preflight 拒绝。CLI 把自动 reject-once 写成 `User rejected the execution for tool use_tool`，并取消该轮，而 Host 申请仍 pending。

这不是用户主动点拒绝、代理失败或页面连接错误。实机同时出现“本轮中途被取消”和“等待你允许”。随后原申请正常过期，Host 无权限续行让实际模型解释“没有执行”；A/B 独立 receiver 各 count=0、note=未保存、events=[]，上传 receiver 为 uploads=[]。未自动重新申请、批准或选择文件。

另一个已确认的历史启动问题是开发版依赖本地前端服务；当前 preview 已使用内置前端，真实文档为 `http://tauri.localhost/`，没有 Vite1421 listener 时仍正常显示。不能把它与此次权限断点混为一谈。

## 修复合同

- `computer_status` 的空参数仅可读取原 transport/agent/owner/incarnation/connection 的最新未批准或终态申请元数据。返回 `statusKind=access-request`、`authority=none`、原 requestId/taskId/revision；不返回 run、target、backend、traces、timings、截图或 bearer。
- Pending 的 `nextAction=wait-for-host`；terminal 为 `explain-decision`。查询不改期限、不创建任务或 grant、不 sweep 或回收物理资源、不发 continuation，也不启用 feature。
- 明确传 runId/targetId 时仍走原执行 gate；null、空白、重复别名、未知字段、伪造 scope/session 或权限字段不得回退到申请状态。
- 最新申请 Approved 后仍走原 TaskGrant 状态查询。增量 B 未批准/被拒绝时，空参数只读 B 申请；显式原 A run/target 仍可查 A，既不借用 A 批准 B，也不撤掉 A 的权限。
- 未绑定、未知或 retired transport 拒绝。ACP 必须仍是准确注册的 CU wire、原 agent、活的客户端与 active turn，只选 allow-once；不落通用允许缓存、不选 always-allow。
- `computer_request_access` 回执及 MCP 两个工具描述明确要求 pending 时结束本轮，等 Host 事件，不轮询或重复申请。观察、输入、上传等 gate 没有放宽。

实现落独立 Core Broker/Consent 领域模块；ACP 新回归也放独立 test 模块。正式 app-webview descriptor 的 probe 灰度 gate 未解除，默认功能设置、App/AppWorkbench 与十五语言 UI 均未修改。

## 来源与验证

冻结 source133：3420 文件；SHA256 `cf7ec44c5d0e2c5341d72ad0db3ff6cf5a8bf3701e61b61dafc70d9c4f07599b`。与132相比14文件变更/新增，均为该合同、模型描述与测试；其它源码未改变。

原 red runner33117 实际 exit1：5项中3 passed/2 failed，两失败为 preflight `FeatureDisabled`。首次实现缺 BearerClass import 的 E0433 也保留；修正后定向5项通过，随后补增量范围回归并冻结133。

同冻结候选的最终结果：

- Core 完整 lib **950 passed / 0 failed / 0 ignored**，原 runner87944 exit0，前后 source Verify 通过。此前949项是中间候选，不冒充最终950结果。
- 默认 App fresh Cargo-selected libtest，复制后嵌入并回读 Common Controls manifest；去重 **173 passed / 0 failed / 84 ignored-not-run**，原 runner52493 exit0，前后 source Verify 通过。准确 ACP pending status 回归被发现并实际运行；不是完整 App suite，不是原生 picker 或真人授权验收。
- MCP protocol/drag/transport **47 passed / 0 failed / 0 skipped**。定向 ESLint、Core test-support strict、App default lib/default tests/probe tests strict、fmt/diff 与 final quality 实际通过。
- App+Workbench **13573**、≥1000 文件 **80/80** 未增长；`FINAL: PASS` 仅指代码质量，不是整体交付。

中间 Windows PowerShell runner 因 native stderr 被 ErrorActionPreference Stop 当成错误记录而在编译开始时退出，未进入断言；首份 partial log 保留。修订 runner 保留 stderr 并按原 native exit 判断，随后真实完成。没有把那次脚本失败写成测试通过。

证据：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/consent-status-20261012-01/` 与 `src-tauri/target/codex-test-runtime/app-consent-status-unit-20261012-01/`。

## 用户包与收尾

source133 完整普通入口灰度包已完成：原 runner87585 exit0，fresh 前端、普通 App 与随包 runtime 实际构建；450 文件 ZIP roundtrip/source Verify 通过。EXE SHA256 `6767acb12f6ca2d83011aa561ba866e8cdc3dd731514afe0ab2c648da0dbb08c`，ZIP SHA256 `4b804dcdf9882ae5c96fc6eb38cf7ce8080b9a6daaba5a541f48da0ca77d799e`。它是本地未签名灰度包，不是完整发布验收。

原 App132 请求已过期，零业务效果。三本批验收标签已通过实际 UI 关闭，原 App51460 正常关窗，随后原 CLI1560/MCP71020 均缺失；没有持有 App 原退出回执，不宣称它 exit0。两原 fixture tool sessions47802/60528 接收 quit 后均 exit0，原 PID34388/16068 与1285/1689 listeners 均缺失。未停止其它工作区进程。

同独立 home 启动新 App133，PID10928、creation UTC `2026-10-11T19:43:01.7965166Z`、窗口10620114。通过 `@oai/sky` 直接检查实际截图和 accessibility：主界面 `http://tauri.localhost/`、关于页 `#/settings/about` 正常，关于文案准确为 `CU Gray Preview 20261012 app133`，没有 localhost connection-refused，1421 无开发服务仍可用。返回主界面后再次检查只有一个本工作区 preview。保留独立home、登录、历史和失败证据，未发新的模型任务、未代批授权或选择文件。

原 packaging `artifact.json` 与 launch `startup-process.json` 保持各自时点的不可变记录；后续窗口核验见 delivery-app-133 的 `startup-visual-verification.json` 与 `.codex-window-check/current-preview-20261012-gray.json`。启动/UI 正常不等于新的模型、真人批准或原生 picker 验收通过。

已在原成功 App test owner 退出后验证路径/hash/无 reparse/无进程引用/独占文件读取，移除唯一 disposable manifested `app-tests.exe` 副本。原 cleanup JSON 的逻辑尺寸读于删除后，记录为 null；已保留原记录并注明缺失，未猜测或重删。H空闲净观察增 **76296192 bytes**，并行构建中不能全部归因于该删除。源码 test EXE、manifests/logs/home/current包及依赖保留。

本轮启动收尾没有追加删除；新133为当前预览/用户包，132保留为准确故障候选及回滚，原home/immutable seed/依赖/首败报告保留。其他工作区构建仍在运行，不碰其产物或owners。最后观察空闲 C392553058304/H215399448576 bytes；不是删除回收量。

## 下一步与未证明的范围

实际修复包正常启动和窗口自查后，需经用户明确发起新的验收，在原产品 ACP/MCP/model 链验证等待、真人选择/批准、A/B效果、真实文件picker/上传、完成与物理收尾。过期旧请求不复活、不自动重发。正向验收之后还需独立 Pause/Continue/active Stop、旧 refs、picker cancel/Quit；不能用上述 synthetic 单元结果解除正式 gate。

D22 下载 buffer durable owner/journal/异常维护、其它平台与 surface、installed 升级回滚、真实模型矩阵和同候选12h仍开放。整体未交付，本批无 commit/push/PR/tag/Release。
