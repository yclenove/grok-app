# E4b.2：内置扩展导出与慢准备交互

状态：**verified_scoped / full CU active / partial — not releasable**。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer，原 dirty/untracked/
`fix.patch` 保留；未启动 Grok/子代理，未提交、push、PR 或发布。验证已经收尾，
本批最终缓存清理已完成并复验，详见下文；下列实际证据不代表完整CU交付。

## 实际实现

1. App 可执行文件内置固定44个扩展生产文件，包含15locale，排除测试与fixtures。
   从实际manifest读取版本1.1.0、Chrome/Edge最低112、扩展ID
   `bgegbabkegkanjbmjbeaockdijnbkjgi`；Tauri打包前检查生成locale和生产依赖清单。
   当前源码bundle SHA256为 `1d0b8d7b8516db72d0ee91464a6ee4ec2f504b2c8df12fb12857abbe3fda6d77`。
2. 明确点击才导出至Host派生的App数据目录
   `computer-use/browser-extension/v{version}-{bundle SHA256}`，查询info只读。
   主WebView命令不接受renderer提供的路径/profile，不注册镜像或模型工具。
   不下载、不启动浏览器、不安装入用户profile、不配对、Share或发布执行权限。
3. 导出采用唯一staging与原publication lock。已有目录必须marker、44文件字节和精确树
   全部匹配；外来、被修改、额外条目、links/reparse/hardlinks拒绝，不覆盖。
   Windows `MoveFileExW(0)`、Linux `renameat2(RENAME_NOREPLACE)`、macOS
   `renamex_np(RENAME_EXCL)`做真实不替换发布。失败staging保留，显式重试新staging。
   Unix是安全祖先检查和文件O_NOFOLLOW，**不是恶意同UID并发rename的完整openat沙箱**。
4. 共享任务准备与旧配对入口复用折叠安装UI，展示匹配版本/ID、完整目录、复制和打开目录，
   给出手动Load unpacked步骤。失败、重试、折叠、Stop/Cancel与迟到回包有隔离；12个新key
   同步15locale。未增长App/Workbench；没有商店安装或自动Load unpacked功能。
5. 慢metadata读取不原生disable复制或confirm。明确confirm遇到在途read只排队一次，
   绑定原challenge全六字段和原有效期，hide/Stop/失败/替换/ready/到期均丢弃队列。
   已在途confirm不会再排第二次；不确认新nonce或续期后的挑战。
   clipboard pending改aria-disabled配合同步guard，保留焦点，禁止重复写入。
6. 扩展导出单测补实际编译字节与当前checkout逐文件对照及info输出，避免仅证明旧内置
   bundle内部自洽。UI fixture每次picker具唯一身份；新真实组件/浏览器场景覆盖Share撤销
   后空receipt→重新Share→同picker重试、隐藏时撤销排队confirm、原卡片退出后面板重开。
   Share、visibility信号和Host回复在这些UI场景中仍是synthetic，不冒称真实MV3权限链。

## 首败和已取得证据

证据根：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`，不覆盖旧首败。

- build8因缺Windows filesystem feature编译失败；补feature后build9完成。
  首native导出10 passed/2 failed：std::fs::rename会替换空目录、Windows canonical
  fixture把`..`自身规范化。实际no-replace与非canonical反例修后artifact10的12项通过。
- `existing-slow-red-1.txt`有慢metadata/queued confirm反例；原有效期与全challenge保留、
  open transition去重等修复后定向71通过。旧前端全量8104属于复制pending修复前候选，
  不作为最终前端证明。
- `existing-bundle-browser-1.txt`实际复制时原生disabled导致Chromium丢焦点，已修；
  原截图、failure和hash保留。新复制焦点/重复操作/失败重试/到期单测3项通过，原10项
  fields回归完整保留，共13项通过。
- `existing-bundle-browser-2.txt`：**153场景通过，actual exit0**，实际结果
  `.run/task-consent-ui-2026-10-04T05-05-16-531Z/report.json`。含原115、两个2200ms
  慢metadata、三条新增生命周期和33条扩展导出场景。15locale/深浅色/300px/1.5x、
  键盘/原owner/Stop/Cancel/失败重试通过；英文浅色、中文深色截图人工复看通过。
  input保留完整路径，窄屏内可选中/复制；按钮可达、ID换行、无水平溢出。
- Node首轮 **182 passed/1 failed**：原CJK/emoji TypeText返回unknown。
  原14项隔离复测通过；`existing-bundle-node-2.txt`原预算/默认file concurrency
  **183 passed/0 failed**。生产50ms resolve guard及整体deadline未放宽。
  原detail缺失，**首败根因未证明，不以复测销项或断定仅调度压力**。
- App首宽回归 **288 passed/4 failed/1 intentional ignored**。
  cleanup场景实际已Stopped，随后测试root删除PermissionDenied；其余包含worker_timeout
  或未证明Stopped，不能把四项统写成同一超时原因。原四场景以同artifact10/原预算隔离
  复测全部通过（managed_owned7/1ignored、native1、cleanup1，集合重叠不相加）。
  `existing-bundle-app-cu-2.txt`宽回归 **292 passed/0 failed/1 intentional ignored**，
  119.80秒。**自然21530ms关闭长尾与本轮首败根因仍开放**，不是生产修复结论。

## 本轮验证收尾

| 实际范围 | 本批结果 | 证据文件 |
| --- | --- | --- |
| 全前端 | 707文件 / 8107 passed / 0 failed，174.41s，actual exit0 | `existing-bundle-full-2.txt` |
| 类型、lint、fixture类型、UI build、quality、fmt、diff | 全部actual exit0；UI build保留既有Vite大chunk warning | `existing-bundle-{type,lint,fixture,build,quality,fmt,diff}-3.txt` |
| Node扩展/bundle回归 | 183 passed / 0 failed，原预算与默认file concurrency，actual exit0 | `existing-bundle-node-2.txt` |
| Fresh默认App编译资源witness | 12 passed / 0 failed，逐文件匹配当前checkout，actual exit0 | `existing-bundle-native-3.txt` |
| Fresh默认App宽CU | 292 passed / 0 failed / 1 intentional ignored，126.76s，actual exit0 | `existing-bundle-app-cu-3.txt` |
| Fresh默认App ACP | 177 passed / 0 failed，2.66s，actual exit0 | `existing-bundle-acp-2.txt` |
| Fresh默认App routing | 38 passed / 0 failed，2.34s，actual exit0 | `existing-bundle-routing-2.txt` |
| 默认App严格Clippy | actual exit0，包含新增native witness | `clippy-app-4.txt` |
| probe配置严格Clippy | actual exit0，原all-targets / -D warnings | `clippy-probe-3.txt` |
| MCP/golden协议 | 38 passed / 0 failed，actual exit0 | `protocol-2.txt` |

默认App使用fresh build11并嵌入CI Common Controls manifest，artifact为
`app-artifact-11.json`：

```text
executable: H:\aicoding\grok-app-computer-use\src-tauri\target\debug\deps\grok_app_lib-63e3432e557be5b6.exe
SHA256: 16764941cf6d92df595434daa6b3161ddff2fc9f5c2ff7fdfb0c15e7663d2b91
```

artifact9/10/11指向同一路径，当前已由11覆盖；旧hash仅属历史，不声称旧binary仍可回放。
最终source记录为`source-files-9.sha256`，3246个源文件，SHA256
`5d607541ba26ae1a8952d35d75c400699fdf3137a4f74dc88a008a4f0418b97b`，已逐项复验。
Browser2及全前端2在最终manifest9创建前运行；它们覆盖的UI/fixture/TS源码随后没有改动，
不声称所有命令都在manifest9生成后运行。source manifest5–8只作历史。

只读Node scanner与编译native info一致：44文件、174715源bytes、version1.1.0及上述
bundle digest。Core生产源码本批未改，**没有本批Core全量重跑**，不把旧1068/1084借作本批实测。
集合重叠不相加，首败根因保留。最终清理后3246个source及当前App/Core/seed/fix.patch
和保留cache hashes再次核对不变；App/Workbench合计仍为13573行。

## 构建卫生

- 首失败browser轮精确清自己的site/runtime **372702233 bytes**；成功browser2轮
   **372702991 bytes**，ownersClosed=true，日志/截图/failure/report/hash均保留。
- 原native fixture误生成的Temp/escape下精确digest目录，经44文件和marker指纹验证后
   删除174835 bytes；外层escape和publication lock保留，见escaped-test-export-cleanup.json。
- 新bundle-cache-plan只含一组已验证旧/新代次；执行后旧cache逻辑2930145043 bytes，
   H空闲观察增加 **1565446144 bytes，约1.46GiB**。当前App/Core、最新代次、seed、
   源码/fix.patch清前后hash不变，见bundle-cache-cleanup.json。已执行清单禁止复用。
- build11和两种strict完成后，生成独立`bundle-final-cache-plan.json`，只含两个同配置
   的已替代代次。首次dry-run被Windows PowerShell5.1的JSON array pipeline形状拦住，
   **未执行删除**；修正一次性脚本的值枚举后inspection通过，不改变安全检查或目标范围。
   原execute又被新Rust编译安全阻止，**未生成before或删除内容**；未杀其它工作区进程。
   原owners退出、精确pairs/source/current hashes重验后，本次实际执行已completed / exit0。
   旧代次逻辑 **3295009587 bytes，约3.07GiB**；H空闲从338369114112到340336791552，
   观察增加 **1967677440 bytes，约1.83GiB**，不把共享hardlinks的逻辑大小当作腾出空间。
   见`bundle-final-cache-before.json`和`bundle-final-cache-cleanup.json`；当前程序、最新
   代次、依赖、immutable seed、源码/fix.patch、报告/日志/截图/hash保留且复验不变。
   两次已完成的cache清理H空闲观察增量合计约3.29GiB，不包括browser逻辑文件大小。
- 不确定的失败fixture roots、build8 working代次及跨配置cache继续保留；不杀无关进程。
   删除未进回收站，可按原cargo/pnpm构建测试重新生成。依赖/immutable seed和当前dist保留。

## 继续路径与完整范围

本批不是发行包，也不是真人App、浏览器手动安装、toolbar activeTab/Share、Grok或新
TaskGrant联合验收。production ExistingTab descriptor目前仍关闭。

下一批先核新TaskGrant所需source MV3/认证HTTP/原App ACP publication联合路径的实际
缺口，不能用旧existing-mcp-live的legacy mcp-open替代。原任务真实Share撤销/重新Share、
browser/profile/document替换、权限手势及installed安装入口仍需对应证据；按已接受E4/E5
门槛再开放compiled descriptor，不变默认feature/raw parser/模型授权面。

完整U00–U09/D00–D31、四surface、Windows/macOS/Linux及双Mac架构、全部约定输入/
恢复、签名安装升级回滚、真人Grok、同冻结最终候选12h与完整UI/UX仍开放。
保持active / partial — not releasable，不把synthetic UI、native unit和旧grant拼成整链。
