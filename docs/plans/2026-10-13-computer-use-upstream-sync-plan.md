# Computer Use：本批后同步 fork 与源仓库

2026-10-13。用户要求在合适时机拉取最新代码；本批已完成两边 fetch，
实际合并安排在来源回读收尾后、下一轮产品修改和新包验收前。
保持 `feat/computer-use-implementation` 的全部开发内容，当前用户预览继续使用原隔离home。

## 已固定的引用

| 引用 | 本次完整 SHA | 当前 committed HEAD 相对它 ahead / behind |
| --- | --- | --- |
| 当前CU HEAD | `8d8b71c262ab222748bca32412eea884d4b892e0` | — |
| fork `origin/main` | `b58b97cd61522a9e92095cfcb1d8d1aa664edda5` | 6 / 24 |
| 源仓库 `upstream/main` | `9c5eb3b589fa6234258cb0bcbb26e5ee0971d8bc` | 7 / 52 |

fork相对源仓库另有8个独有提交，缺少35个提交。两边都不能替代另一边：
fork增加Linux ARM64发行及构建修复，源仓库增加聊天/上下文/输入框/会话修复。
这里的数字来自 Git 提交图，不是缺少的功能数量。执行合并前再次fetch，
如引用移动，应按新SHA重做差异和冲突预检，保留本次证据。

证据在 `.run/maintained-notice-source-20261013-01/sdk-ffmpeg-sync-review-03/04/`，
完整路径以 `tools/computer-use-probe/` 为根。使用 `git merge-tree --write-tree`
预检仅写Git对象；未checkout、修改真实index或合并工作树。
原HEAD、235份tracked WIP内容和真实index前后均核对相同。

## 预检发现

committed HEAD与fork冲突2项：`.github/workflows/release.yml`、`docs/BUILD.md`。
committed HEAD与源仓库冲突6项：`AGENTS.md`、`CHANGELOG.md`、
`src-tauri/src/session_manager/{connect,control,mod,turn}.rs`。

另外，fork改动与当前tracked WIP重叠7项；源仓库重叠36项，包括ACP、
command registry、session manager、turn interrupt、会话事件、15种语言的
workspace文案及rewind/stream/endOfTurn。当前未跟踪源码尚未参与合并预检，
真实合并可能新增冲突。即使自动合并成功，这些生命周期路径仍需逐项复核。

## 实施顺序

1. 记录当前HEAD、index、tracked/untracked源码清单和内容哈希；检查敏感文件。
   将CU产品、测试、开发文档分成可恢复的本地提交，或使用独立临时index创建
   仅含核准源码的Git快照。保留原staging状态；不把dist-portable、runtime/source
   cache、`.run`、用户home、登录或草稿加入Git；不做整工作区stash/clean/reset。
   有未完成源码也可保存在明确标注的本地WIP提交，不能将其写成验收通过。
2. 在可恢复快照上先合入fork main，再合入源仓库main，分别保留合并边界。
   避免重复cherry-pick已经包含的提交；保留fork的Linux ARM64发行修复与
   CU四目标资源映射。合并时不得用整文件ours/theirs覆盖会话模块或发布配置。
3. 逐项复核会话连接/结束/取消/工具事件、CU任务授权和撤销、MCP注入、
   rewind/fork、官方/自定义渠道、上下文及输入框草稿行为。保留当前未发送草稿，
   不自动发送真人模型消息。原App+Workbench增长冻结需要以新的上游基线与
   我们新增内容分别比较，不能随手抬高门禁来刷绿。
4. 先跑冲突和重叠路径的针对性测试，再跑项目要求的typecheck、前端测试、
   Rust格式/strict clippy及必要Core/会话回归；Windows harness按既有manifest
   规则启动。对macOS/Wayland等没有实际设备或环境的验收继续写not_run。
   合并不应顺带改变App macOS11+/CU受管Mac14+或晋升未验运行时。
5. 核对资源准备/包审计、版本号、CHANGELOG与工作流。两个main已经各自写过
   0.2.40，不能产生重复版本发布或重写已发布CHANGELOG章节；沿用项目发版规则。
   只有合并和回归完成后，才重建新的可用预览并按原home交给用户。
6. 保存同步报告与可回退点，按既有授权提交/push开发分支；不由这次fetch自动
   创建PR、tag或Release。没有执行实际合并就明确写“已fetch、未合入”。
   原owner退出后清理本批已确认可退休的构建副本；保留唯一当前用户预览。

同步后继续原R1–R4完整交付计划；此计划不缩小平台、surface、安装和实机验收范围。
