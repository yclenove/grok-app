# Computer Use：Windows 原生完成回调与私有 hive 验证

日期：2026-09-30（本机 +08:00）；工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`；HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。

**完整 goal 仍为 active / partial — not releasable。** 上轮和本轮均为 progress，不是等待或最终完成。
本阶段把上一阶段“只编译生产回调、运行合成回执”的证据推进到**执行完整生产 NSIS 回调**，并修复实际暴露的 nonce 校验问题。回调只在独占测试进程的私有应用 hive 和 UUID 目录内执行，仍不等于真实 Grok 安装、签名更新或完整安装态验收。

## 本轮实现

1. 从 `src-tauri/nsis/update-completion.nsh` 提取共享原生回执 writer，生产入口仍必须先通过原来的更新模式、scope、nonce、注册信息和安装文件检查；没有添加跳过生产门禁的测试开关。writer 保留 CREATE_NEW、FILE_SHARE_READ、WRITE_THROUGH、OPEN_REPARSE_POINT、UTF-16LE BOM/CRLF 和原生 PID/创建时间。
2. 新增 inert NSIS fixture：真实 `.onInstSuccess` 调用同一个 `GrokWriteUpdateCompletion`。先 RegLoadAppKey / REG_PROCESS_APPKEY 创建私有 hive，再 RegOverridePredefKey；通过唯一 private fence 回读后才允许任何注册写入。失败立即退出，不进入安装 Section。override 保持到进程退出，未在后续回调前恢复宿主 HKCU。
3. 核对本地打包模板 `utils.nsh` 的 SetContext 后，fixture 显式采用 **64 位注册表视图**，匹配生产 x64 路径；独立 PowerShell readback 同样指定 Registry64。每个场景前后检查宿主 HKCU 32/64 两种视图没有该 fixture 的唯一 root/fence。此结论不是“全局注册表字节未变”。
4. 真实回调反例发现 `${StrLoc}` / `StrCmp` 的大小写不敏感行为会接受大写 UUID。增加 `StrCmpS` 与 canonical lowercase hex 字符对照，拒绝该输入；Rust 后端此前仍会拒绝不匹配的回执，因此不将它描述成已证实的完成权限绕过。
5. Node fixture 的中文、空格、emoji 和字面 `$` 路径/字符串正确编码；修复 JavaScript replacement-string `$` 转义。process owner 在释放 permit 前保管确切进程 handle，读取原生创建/退出 FILETIME（以字符串存储，避免 JS 数值精度丢失）；限时失败只清理自己持有的测试进程。
6. 新增显式 ignored 的 Rust 原生输入测试，通过 `GROK_NSIS_NATIVE_CALLBACK_FIXTURE` 读取**实际回调 bytes**及独立进程身份，调用生产 `verify_receipt`；再拒绝错误 nonce、创建时间、非零退出和截断回执。它构造测试 Record，**不**授予 journal 退休，不代替 signed inventory / live-file 完整对账。
7. Windows CI 必须有 NSIS，运行 native fixture，并把唯一成功回调输入传给显式 Rust 测试。缺少 fixture 或精确测试名都失败。App 与 updater harness 均由该次 Cargo JSON artifact 选择，不再 glob 旧缓存 EXE。本轮仅本地检查并验证选择表达式，未执行远程 CI。

## 验证结果

证据根：`tools/computer-use-probe/.run/windows-completion-writer-20260930/`。

| 项目 | 实际结果 | 范围与限制 |
| --- | --- | --- |
| 完整生产成功回调 | **21/21 场景通过** | 原生 NSIS 3.11、私有 hive、64 位视图；不是整个 Grok installer |
| 共享原生 writer | **5/5 场景通过** | 精确 Unicode/进程字段、已有文件、hardlink、目录、缺失父目录；不覆盖注册门禁 |
| 默认 updater library | **86 passed / 0 failed / 8 ignored** | features=`default,rustls-tls,zip` |
| 独立 no-zip updater library | **85 passed / 0 failed / 8 ignored** | 独立 consumer，features 仅 `rustls-tls`；manifest/lock 与原 pinned 输入相同 |
| 两套原生回执 Rust 测试 | **各 1 passed** | 直接运行上述第 8 个 ignored 测试；另外 7 个为由父测试调用的 owned-child 入口，不计为额外顶层 passed |
| App 直接依赖回归 | **38 passed** | updater 7、shutdown 3、browser process 7、MCP session 21；不是全 App library |
| Node producer/portable/hygiene/NSIS | **86 passed / 0 failed / 0 skipped** | 15 + 29 + 12 + 8 + 22；包含父级 test 节点，不代表 86 个原生安装场景 |
| 两套严格 Clippy | **通过，0 warning** | App + 默认 updater、独立 no-zip，`--offline --locked -- -D warnings` |
| 格式、脚本和工作流 | **通过** | workspace/vendor fmt、两个 Node test 语法、CI/release YAML、4 个提取的 PowerShell block + 2 个 helper 仅语法解析；diff check 无空白错误，保留 Git CRLF 提示 |
| ACL / upstream IIFE | **保持既有边界** | recovery 仅本地 main/session-* 两条命令；不进入 default/remote；IIFE hash 与上游记录相同 |
| 选定源码与依赖 | **110 项源零漂移** | 不是全产品最终候选；此前 App Cargo.lock hash 保持不变 |

21 个完整回调场景为：ok、bad-product、bad-version、bad-publisher、bad-main、bad-location、bad-uninstall、bad-root、no-main、no-uninstaller、not-update、bad-nonce、uppercase-nonce、bad-variant、short-nonce、nonhex-nonce、missing-name、missing-receipt、existing、hive-failure、override-failure。只有 ok 创建新回执；existing 保留原哨兵文件。两个隔离失败反例核对具体失败阶段，避免把错误 API 调用产生的统一失败误当成功。

本阶段没有修改 React/UI，也没有重跑前端；前阶段 141 项前端结果仍只属于前阶段，不冒充本轮 fresh run。

## 证据入口

- 最终 complete callback：`grok-private-callback-vhBLnc/`；`private-callback-receipt.json`、`observations.json`、逐进程 JSON、私有 hive、实际 `native-callback.json` 和回执均保留。
- 当前 hook SHA：`b0d91d003d499833a6b0a84305deba71f3fa2bca583fdebc46f83ded562c584d`。
- 最终 native fixture EXE SHA：`36195014a8cce8cf010c89b69bce2cccee2ebaf4203cd1a128af22719631daf0`。这是测试 EXE，不是签名发行安装器，也不宣称构建可重现。
- 最终共享 writer：`grok-receipt-writer-J5A9DL/`；完整宏另有 `grok-nsis-completion-VDMjY8/` 只编译、不执行的旧式 fixture。
- 默认/nozip 使用同一个最终 64 位 callback：`native-input.json`、`default-native-callback-x64.log`、`nozip-native-callback-x64.log`。
- 全部选定回归：`updater-tests.log`、`nozip-tests.log`、`app-*-tests.log`、`node-final.log`、`clippy.log`、`nozip-clippy.log`。
- 构建来源/副本：`native-artifacts.json`、`nozip-artifact.json`、`executables/`；`final-review.json` 记录 feature、SHA、源变化和明确边界。
- 历史失败日志保留：早期 fixture 的 `$` 转义和 RegLoadAppKey 参数错误（在任何注册写入前中止）、`private-callback-v3.log` 的 uppercase-nonce 实际失败。后续默认视图的成功 run 保留为历史；最终结果明确指向上述 64 位 run，不按目录时间猜测最新成功。
- `source-freeze.json` / `receipt.json` 绑定本阶段选定源、本文与三个当前索引、测试证据；独立 Python 分块回读验证。前阶段 receipt 保持 SHA `4615d27eb6d52cfbcefa30877857107c43b444275fc5d654ec8c30a21c12fd43`，不重写已封存阶段。

## 尚未完成 / 下一步

1. **真实签名 Grok clean install → update → repair/rollback → uninstall → 下一次更新的完整安装态链未验证。** 本轮完整回调已真实执行，但主 App、生产全安装器和 uninstaller 未运行；inert 文件不是已安装 App。应在可销毁、明确拥有的 Windows 安装验收环境推进该链；进程私有 registry override 不继承到生产安装器可能启动的子进程，不能因此在宿主直接执行整个模板。
2. 无完成协议的历史安装器、MSI、失败/回滚和 dispatcher/机器失效后的未知结果仍需完整可用恢复；“安全 blocked”不等于最终功能可用。
3. 全目标范围不变：**Windows x64、macOS arm64/x64、Linux X11 与原生 GNOME Wayland；Desktop / managed browser / existing Chrome/Edge tabs / App WebView；App / ACP / MCP；完整输入、中文 IME、clipboard、取消/恢复；签名安装更新修复回滚卸载；原生窄窗/DPI/权限 UI；真实 Grok E4；同一最终冻结候选的 12h active soak。** 缺少任一要求的当前权威证据都不能 complete。
4. 本机未取得用于全安装验收的已验证隔离环境；没有为此启用功能、启动 Docker/VM 服务、修改宿主安装或读发行私钥。此项不构成“所有开发都无法推进”，不满足将整个目标标为 blocked 的条件。
5. 未 commit/push/PR/tag/release、远程 CI、签名/上传，也未终止用户 App/浏览器。测试仅执行明确持有所有权的进程；私有 hive 内有预期注册写入，未写真实 Grok 注册项。

**阶段判断：progress；完整 goal 继续 active，未满足最终完成审计。**
