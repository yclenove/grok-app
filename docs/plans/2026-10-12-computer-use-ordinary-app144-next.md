# 普通 app144：新真人任务就绪，尚未发送

## 当前状态

app144/PID19768、creation UTC `2026-10-11T21:53:09.3899735Z` 仍是唯一用户预览。普通页面、历史与关于版本已有启动证据；本批 session API health 真实返回 ok=true/connectLockBusy=false，未输出 token。Computer Use 已由此前用户操作启用，未由本批修改权限设置。

通过真实 Windows 界面打开资源侧栏的三个独立 native 内嵌页，并实际核对页面内容：

- A：`http://127.0.0.1:14895/a`，“内嵌页 A - Computer Use 验收”。
- B：`http://127.0.0.1:14895/b`，“内嵌页 B - Computer Use 验收”。
- 上传：`http://127.0.0.1:14898/`，“本机文件上传 - Computer Use 验收”。

独立GET /state 初值 A/B备注“未保存”、计数0、events0；upload receipts0。没有用API伪造业务效果。页面导航只用于准备，未代模型保存、计数或提交。新任务草稿已放在新会话输入框，**未发送，没有新的待批准请求**。已向用户询问现在是否方便配合；避免再制造会自然过期的无人批准请求。

notes原TTY session47838/PID19096；upload原TTY session78382/PID61324。当前准确监听14895 IPv4/IPv6、14898 IPv4均核对owner。测试结束用对应原session的`quit\n`退出并join，不能按名称停Node或退出别人的App。暂保留这组当前验收页面，零新构建/临时编译站点，额外清理0 bytes。

证据根 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/ordinary-app144-20261012-01/`：fixture-owners、task-prompt、expected-upload、validation-145。公开测试文件 `public-upload.txt` 168 bytes，SHA256 `8a63914b1ced20b22cfb5d4614c7784045f0d6fb57ece8e4f5d36a923f1546c4`；只可由用户选择，并传给本机接收器。

## 立即执行顺序

1. 用户确认现在方便时，重新读取实际窗口/输入框/模型状态，发送该新任务一次。若用户自己已发送，先读取，不重复提交。
2. 原Grok模型应识别并申请app-webview范围。由真人选择上述三个目标并批准；不得自动批准、不放大全桌面、不复活132过期请求。
3. A：保存“app144：甲页已保存”一次，计数3；B：保存“app144：乙页已保存”一次，计数2。由模型通过CU操作，再独立读取接收器events检验实际后置条件。
4. 上传需任务内新文件申请和真人原生picker，仅选public-upload.txt。模型用CU上传并点提交一次，独立receiver必须恰好一条、文件名/168 bytes/hash准确。取消/过期/unknown不可重放副作用。
5. 保存原ACP/MCP/request/run身份与实际工具/错误、UI状态及接收器证据；成功后再做新独立下载、Pause/Continue/Stop/Quit验收，不拼成已经通过。
6. 原测试结束，精确退出fixture owners；用户预览按用户使用意图保留，当前登录/home/history不清理。

如果用户暂不方便，草稿保持未发送，继续其他无需人工的开发验证；没有批准不标通过，但也不把整个目标标为blocked。

## source145：辅助服务EOF收尾修复

本批第一次plain-pipe备注服务PID37004的stdin已关闭却继续监听；upload服务PID60828则按既有EOF逻辑正常exit0。备注服务无法写quit，核实准确新task owner后只停止该辅助进程，原tool session54694 exit1；不冒充正常退出，不涉及产品App/浏览器。随后以两个新的TTY原sessions准备当前页面。

`app-webview-fixture.mjs` 新增stdin end→正常shutdown，与upload fixture一致；test提取同一原CLI启动助手，新增原child EOF正常exit0以及两种loopback URL不再可连接的断言。不声称覆盖held HTTP body或产品退出延迟。

source145仅两个fixture源/测试文件与source144不同，3432 files，manifest SHA256 `a7f4863cc6c0c4a5a71ff420b412c3ade33a17596c5fe0fac89284ef6128dfb7`，前后Verify通过。实际Node9 passed/0 failed/0 skipped（原tool chunk404d48），四fixture文件ESLint exit0（19d5d1），diff check/final quality通过（1119e1）；App+Workbench13573、≥1000源码文件80不增长。

当前用户包仍source144（产品前/后端没有本批改动）；已运行的notes辅助服务加载的是修复前SHA256 `925529…` 的source，记录在fixture-owners，不冒充145运行时。145修复由新独立Node child回归验证。没有新增commit/push/PR/发布。

## 范围与开放项

本批普通页面准备和辅助程序质量验证是实质进展，**不是原Grok真人CU执行通过**。Windows descriptor正式灰度gate保留；D22其它资源/正常退出历史138/139原因、三平台两Mac架构、四surface、installed升级修复回滚、UI/UX、真实模型矩阵、同候选12h、D00–D31整体仍 active / partial。
