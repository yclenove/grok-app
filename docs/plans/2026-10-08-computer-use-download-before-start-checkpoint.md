# App 下载开始前取消与 Windows 长路径转存检查点

2026-10-08，`feat/computer-use-implementation`，HEAD
`8d8b71c262ab222748bca32412eea884d4b892e0`。整体 active / partial，未提交、推送或发布。
本检查点保留失败过程与后续修复证据。开始前 Pause/Stop/grace 的 scoped 验证已通过，
其余下载竞争、upload、fresh 用户 App 和完整交付仍开放，没有更新用户窗口。

## 当前修复与验证

`-11` 恢复原 legacy path 后，同一 App 的普通 255/254 字符路径下载成功，260 字符路径仍
`FILE_FAILED`；原八秒 grace、unknown 和迟到真实回调正常。`-10` blanket prefix 保留已移除。

新增领域模块 `windows/destination.rs`：普通路径使用 native 兼容格式；超过文件/目录长度
边界，或 DOS 与 canonical 名称语义不同时，使用当前用户 Temp 下的随机私有目录。
没有固定用户路径、盘符或 8.3 文件名依赖。原 Broker destination、basename 和授权根不变。
真实原生终态后，以 create_new、64KiB 分块、原取消检查和原 8MiB 上限转存，
发生错误/取消只回滚本次创建的目标。文件仍经过原 Broker staging 验证及 document/epoch/最终提交 guard。
caller_done 在转存后才设置；已完成且 Wry 已回执的源不能被随后 browser exit 提前删除。
unknown 保留原 expectation 与私有 buffer，实际回调或精确 native exit 后才退休。
清理仅删除准确文件和空的私有目录，没有递归删除。

同一产品 EXE `-12`，SHA256
`D5F0439F67CD08D476FBB72A9862FC2DEE6621C7727DDD92B88F811217270AC5`：

- `app-download-before-start-20261008-12` 三场景全部通过。响应头释放后原终态分别 15/6/10ms；
  八秒 grace 的真实 measured expiry=8000ms，未知结果返回时原占用与同一个 private directory 保留。
  迟到回调后私有目录实际消失、零重放/迟到正文复活、同 browser 新 native owner 可下载。
  新文件仍为原 264 字符 canonical Broker 路径，native 临时路径 104 字符，37 bytes/hash 校验通过。
  原 App joined，14 个 observed owners 全退出，remaining=[]。
- `app-download-close-path-regression-20261008-12` 原并发 Close 与失败/显式重试两场景全部通过。
  原 retained view hidden、keeper/UI 响应，失败等待实际 5013ms，释放后的关闭分别 24/21ms。
  12 个 observed owners 全退出。毫秒数字是释放后的收尾时间，不是整个下载耗时。

上述仍是实际 App library/main close IPC/Wry fixture；Host approval/pin synthetic，
控制直达原 Broker，realAcp/realMcp/realModel=false，不能当作用户任务控制全链通过。

本轮 fresh manifested App library harness 位于
`app-download-before-start-regression-20261008-13/`。
Cargo source SHA256 `6B3BF93A6997CC4E39831EADD064679445E0DFFFF4F5F2BEF3582F33E846921F`；
SDK mt 嵌入原 windows-test-manifest.xml 并读回后的 copy SHA256
`68594896A573C1AEBEACF669BFCC5DB67B727AF79AC22BBB4FB7A8C84FB04035`。
WebView131 + Host9 + side_browser_blob4 = **144 passed，0 failed/ignored**。
新增四项实际文件测试涵盖 >400 字符中文/emoji 最终路径、转存中取消、拒绝覆盖、空/超限拒绝。
首次 `-12` harness 130/131：测试 fixture 没达到 >400 的原断言；已将目录层级六层增到八层，
断言未放宽，首败输出保留。该变化只在 cfg(test) 模块，产品源/上述 EXE 行为未变。

三组本轮 strict 实际 exit0：probe+custom-protocol App lib/tests/optional bin、
default App lib/tests、probe crate all-targets，均 `--locked -- -D warnings`。
Cargo JSON、manifest 读回、三份 test logs、三份 Clippy logs 保留；不使用未加 manifest 的 harness。
最终 fmt、diff、runner parse、LF 回读和 final 质量检查 exit0，App+Workbench13573、≥1000文件80/80。
FINAL: PASS仅代码质量；没有将辅助 native fixture 当成完整 CU 或 fresh 用户包交付。
当前源 SHA256：windows.rs `565BFD7FFD2D5AB88C4535B9D9CCDB32403964BE0B11BB650289A71C07B03FFD`；
destination.rs `3B111815725841CAC229D746C26C780B9F6FD3C62B9F0CF18ACA11C0F0AC4551`；
exit.rs `38BDD683EED9CEA8533892E630EC379DE14E783BF966B422CF959DC66D60E458`。

以下 `-02` 至 `-10` 是修复前历史，不覆盖当前 scoped 通过结果。

## 实现与证据边界

新 `webview-download-before-start` debug App 模式使用原 App library、Broker、
TaskGrant、SideBrowserLiveView 和实际 Wry/WebView2 回调。服务器在响应头前保留原请求，
分别验证 Pause、Stop、Stop 后原八秒取消 grace 实际到期。
服务器请求、响应头、响应体各有真实 witness，收尾释放两个 gate。
probe 记录真实 dispatch acknowledgment、native cancellation fence、grace expiry 和回调阶段。
生产八秒取消 grace、十五秒 script watchdog、五秒 join 不变；等待不跨过剩余取消 grace。

fixture 控制直接调用原 Broker；Host 批准/publication pin synthetic。
main IPC 证据仅对应关闭路径，不代表 UI Pause/Stop、ACP、MCP、真实 Grok 或完整用户任务链。
没有新增产品前端；用户预览后端未替换。

证据根：`src-tauri/target/codex-test-runtime/`，运行名
`app-download-before-start-20261008` 及 `-02` 至 `-10`。

- `-02` 的 Pause、Stop 和 same-label 新 owner 37-byte 校验通过，八个已观察 owners 全退出。
- `-03` 至 `-08` 保留诊断失败。最初 caller 等待正好八秒，与 native 截止/返回竞争；
  现等待真实 expiry witness，并断言其 measured elapsed 至少八秒，再使用原五秒 terminal join。
- 实际阶段记录证明 script acknowledgment 约四毫秒完成，响应头仍 held；
  等待在 native cancellation，而非 script evaluation。曾加入的 speculative cancellable
  script 实现已精确移除；只保留原十五秒常量命名。
- `-09` 的 grace/unknown/占用保留/迟到真实回调/无重放/清理已过；
  新 owner 下载失败，native `FILE_FAILED`，received=0/total=37。
  该场景普通路径长度 260，另外两场景分别 255/254。这是待隔离的路径边界证据，
  不能单凭长度相关性宣称根因已完全证明。
- `-10` 尝试直接保留 canonical Windows namespace；Cargo build exit0，
  但实际 App fixture 在第一个 Pause 后新 owner 下载就失败：`FILE_FAILED`，0/37 bytes。
  **该尝试未修复，且出现普通场景回归，不能移入用户包。**
  App exit0 仅是进程正常退出；`native-result.ok=false`，runner exit1。
  七个 observed owners 全退出，remaining=[]。

`-10` EXE SHA256：`85021688AA12C1AD44BF1E5A864AEA4C80475BD7F6DD058D1D41D9C0533751A0`。
当前 `windows.rs` SHA256：`61D6FD5B764E262540C7DB566D0FC7473F0CDD3F21DF04F5E30CEA61CF95F546`。
`before_start.rs`：`CFF228CFE06C40A85F308BAE272F76D01AA39640451D5A1AEFA48EEA1EFB818C`。
`server.rs`：`23893B4DC5AE7AF3DEC2FBE29DDD6436E82602225BFCB804630E3332843C9828`。

此前失败候选的 fmt、diff、PowerShell runner parse 和 `check-code-quality-gates.py --mode final` exit0，
App+AppWorkbench13573、≥1000文件80/80。质量 PASS 不证明 native 下载行为通过。
当时未对失败候选重新运行原下载关闭 fixture、
140 项 Rust 回归或三组 strict。前批通过数字是历史，不能覆盖本次 native 失败。

## 当前用户窗口

亲自选定原 preview PID24536 的实际窗口，再发送刷新并等待页面恢复，
看到正常主页、侧栏、输入框与 Grok4.7 极高，来源 `http://127.0.0.1:1421/`。
Vite PID35932 仅监听 IPv4 1421，显式 bypass proxy 的 loopback curl HTTP200。
此前 localhost/IPv6 与 IPv4 服务来源不一致已由开发入口检查点修正。
本次没有重启、替换用户预览或移动其 home/login；后端仍为十月七日构建。
尝试打开模型菜单时 CU helper 拒绝输入，未计本次菜单验收；截图与刷新成功只证明启动页面。
另一个工作区的 session-import preview 与 Grok Bot 未动。

## 清理

删除前复验 `-02` 至 `-09` 的原 owner records、creation identities、exact-home 无 live owner、
EXE 原 hash、全部祖先非 reparse point 与绝对目标仍在本任务 artifact root。
只删除八个 `computer-use-app-probe.exe` 普通文件；home/profile、logs、owners、native reports、
失败记录、源代码、依赖、当前源 EXE 和 `-10` 候选保留。

精确目标为上述证据根内 `app-download-before-start-20261008-02` 至 `-09`
八个目录各自的 `computer-use-app-probe.exe`，没有递归删除目录。
逻辑 **752,046,080 bytes**；H 空闲从 **265,644,589,056** 至 **266,396,655,616 bytes**，
实际观测增加 **752,066,560 bytes**。空闲变化可能含并发写入，与逻辑量分记。
副本可用对应源版本重新 Cargo build 并按 runner 创建 fresh RunName 再生成；
不能覆盖原证据，也不能将新 EXE 冒称旧 source/hash。
首次运行缺完整 owner records，及前批自动审批拒绝的两个 incremental generations 继续暂缓，未绕过。

本轮 original build/test/browser owners 全退出后，又只删除以下三份准确 EXE 副本：
`app-download-before-start-20261008-10/computer-use-app-probe.exe`、
`app-download-before-start-20261008-11/computer-use-app-probe.exe`、
`app-download-close-path-regression-20261008-12/computer-use-app-probe.exe`。
删除前复验 owner records、creation identities、hash、exact-home 无 live owner、所有祖先无 reparse point。
逻辑 **282,163,200 bytes**；H 空闲 **265,145,872,384 → 265,428,037,632 bytes**，
实际观测增 **282,165,248 bytes**。本轮 `-12` 当前 EXE、fresh `-13` harness、失败 harness、
原 home/profile、所有 reports/logs/manifest/source 均保留；旧首次/拒绝 cache 继续暂缓。
源构建加 runner 的 fresh RunName 可重建副本；不覆盖原证据或把新 hash 当成旧候选。
临时目录只读盘点未见 `grok-cu-download-<32hex>` 新 buffer 残留，旧 lifecycle/shared fixture 目录未动。

## 下一步

1. 完成最终 staging publication 竞争、开始前 navigation/close、原 App WebView upload；
   upload 目前明确拒绝，仅 managed Host 有实现，不开放任意模型本机路径。
2. private native buffer 加入 D22 异常退出/维护恢复的 owner 记录与安全清理验收；
   本批只证明原回调或精确退出后的正常退休，不声称 force-kill 后的跨进程维护已完成。
3. 保留当前 home/login 制作 fresh 用户 App，验原 ACP/MCP/真人 Grok 链；
   四 surfaces、Windows/macOS 两架构/Linux X11/native Wayland、统一 UX、安装升级回滚、
   模型矩阵、同一冻结候选十二小时 active soak 和分批提交范围保持。
