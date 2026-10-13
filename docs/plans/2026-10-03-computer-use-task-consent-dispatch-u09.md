# Grok U09：包含本轮未提交代码的独立Windows开发包

状态：**draft / not completed**。Codex已接手；U08 Windows当前候选门禁后继续本批。
单Codex writer完成owned构建/可复现工具/验收，不启动Grok/子代理；本批不安装、不发布。
U00–U09授权体验与整体D00–D31状态分开，包可供继续开发不意味着正式CU全部交付。

## 1. 沿用已核的开发包边界

先读docs/BUILD.md、AGENTS与当前bundle/portable工具。原对照包只读保留：
`H:/aicoding/releases/grok-cu-dev-20261002-8d8b71c2/`。
其build-record.json已核：Grok CU Dev、identifier `com.grokapp.desktop.cu.dev`，
applicationVersion0.2.38，独立snapshot、本地unsigned NSIS+完整portable，
official updater关闭；installer execution/native CU/model/其他OS/soak均not_run。
旧包基于当时clean HEAD，不能借旧测试或旧record证明本轮dirty授权实现入包。

当前候选以U08接受的源/runtime/oracle/test/build指纹为准，不临时fetch/合main改基线，
不执行install-latest（会切main并安装），不修改用户现有软件或原开发包。
若本批修复产品源码，重新冻结并按影响重验U08/U07，不能构建后把修改摘要当验证。

## 2. 冻结完整源码到owned snapshot

记录HEAD、branch、tracked diff binary digest、全部新增产品文件、锁文件与逐文件SHA256。
dirty版本不能只git archive HEAD；必须包括本轮实际tracked修改和未跟踪产品文件。
先明确manifest allowlist与排除项，再复制到新独立目录；保留原所有dirty/untracked不动。
构建snapshot内可用build-only overlay，不能写进live产品工作区，也不为了版本固定commit。

复制前后核源指纹一致；writer已终态，避免中途源码改变造成混合candidate。
排除.git/凭据/auth.json/secrets/agent-home/profile/VM/日志/输入/截图/浏览器用户数据；
必要ignored runtime seed按项目既有immutable manifest显式入包，不能一锅复制整个.run。
runtime/source worker/extension当前合同和schema匹配，正式资源中不包含fixture secret或
测试shortcut。borrowed缓存/dependency复用明确记录，不称clean重建。

新目录和产物名称唯一，带日期/基线short SHA/dirty候选digest，不能复用旧路径覆盖。
文件操作验证最终absolute root归owned builds/releases目录；失败产物保留，不递归删根。

## 3. 构建与资源合同

沿用pnpm only、现有target/锁与runtime准备/检查，Windows x64 MSVC、NSIS unsigned与
完整portable；从旧record的真实tauri build命令核当前overlay解析路径后再运行。
保持应用语义版本按仓库；显示版本明确dirty候选与开发用途，不伪造正式发布tag。
产品名/identifier独立，正式updater/自动升级关闭；运行时缺失给准确错误而非假可用。
新包默认feature-off，预接仅无权申请通道；没有账号/Cookie/bearer随包导出。

用实际prepare/check/audit/stage工具接口，先回读--help/源码，不猜flag。
Node/worker/Chromium/extension架构、版本、来源/NOTICE/hash、immutable seed副本保留。
installer仅解payload审计，不执行UAC或安装。资源逐文件比portable，exe允许差异必须
是经逐字节核验的Tauri bundle marker，而非只说两个exe版本相同。
ZIP roundtrip回读文件inventory、大小/hash；路径link/traversal/多余敏感文件严格拒绝。
build log保留实际内层exit；下载/缓存/链接失败不通过复制旧exe来冒充本轮构建。

## 4. 当前包实际smoke与限制

使用随包Node/worker/Chromium，在新的owned profile做本地fixture导航、真实PNG、
observed ref动作/业务计数恰好一次、profile close和worker terminal/exit/join；
前后包/seed资源hash不变。不从系统Node/PATH浏览器成功推断随包运行时可用。

便携exe用独立GROK_APP_HOME/GROK_HOME启动，只核本测试PID/start/path/窗口响应、
新默认off与无权申请状态；不继承用户正式App数据或自动批准CU。
如U07已有当前候选真实批准链可在本包按其准备好的真人scene验证，缺真人就明确未验。
至少owned合成Host决定的MCP/loopback实际链在包内可审查，标synthetic不是真人。
smoke失败先保留原artifact/hash/log再修，新的artifact需要新record与相关回归。

结束只关闭精确原测试进程，核其children/ports/profile/lease/listener收尾；用户原App与
browser不操作。native startup、browser smoke、real model、installed权限和安装生命周期
逐项分开；installer未执行就not_run，不据payload审计声称已安装/升级/回滚/卸载成功。
macOS/Linux签名/安装、全部模型矩阵与D30长稳仍归原任务，不能删未验行。

## 5. 交付文件与Codex验收

新release目录交NSIS、完整portable ZIP/目录、SHA256SUMS、source manifest、
build-record.json、简洁使用说明和原失败/验收manifest的精确引用。
使用说明按新按需授权流程写，不能保留旧“先设置→面板授权才有工具”描述；
说明CLI/WebView2/随包资源依赖、独立data root、使用旧包回退和用户数据边界。
不自动替用户安装/启动为日常写入端，不注册协议关联或自动更新。

build record至少含：dirty基线+patch/新增文件digest、snapshot/allowlist、实际commands/
exit、toolchain/lock/runtime指纹、artifact大小/hash/signature状态、逐smoke效果、原owner
terminal/join、未验的平台/真人/安装/模型/soak及overall partial；未知结果不能写passed。
交`2026-10-03-computer-use-task-consent-u09-checkpoint.md`，Codex复核实际文件和原输出。
U授权链交审后继续原D00–D31ready queue；不以“打包成功”关闭完整目标。

不commit/push/PR/merge/tag/release/reset/restore/clean/stash/rebase；不改凭据、共享~/.grok、
代理/VPN/用户profile/日常进程/其它worktree，不自动系统/App/CU/Share批准。
