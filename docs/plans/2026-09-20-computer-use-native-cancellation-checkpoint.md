# Computer Use：原生下载与暂停恢复检查点

日期：2026-09-20。工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`，HEAD `30757366`。
总状态仍为 **partial — not releasable**。这是受管 Chromium 的局部增量，
不代表 ExistingTab actions、安装、平台、真实模型或长稳已经完成。

后续新增的真实 terminal Stop 与恢复响应竞争验收见本文末节；原生启动内部
仍未验收。前面的 266 文件指纹和单测数字属于原修复批次，不冒充后续新命令。

## 本批行为

- 下载流使用带 AbortSignal 的 pipeline，取消会唤醒阻塞读取及写入背压。
  文件流 close 完成后才归还；修复 Windows 下流尚未关闭就删除 `.part` 的竞争。
- 已获得 Playwright Download 的原生传输通过 `Download.cancel()` 收束。
  事件等待与取消任务都保留到 finally；取消后不发布最终文件、不重试下载。
- 下载前按链接类型选择一种路径：显式 `download` 属性走浏览器原生下载；
  普通链接直接流式获取一次，不再先 click 再用 fetch 补偿未知结果。
  普通获取在每个重定向重新按实际 URL 向浏览器取适用 Cookie，保留浏览器
  对 path、Secure 和过期的过滤，不把整个 context Cookie 列表发给任意同域路径。
- Chromium 在只收到极少量正文时可能仍未产生 Download 对象。原生下载事件
  四秒内未确认时，不能仅凭等待超时释放物理占用；现在先关闭该 run 独占的
  managed context，终止未归属传输，再返回 unknown。profile 文件仍保留，
  该 context 的标签页须显式重新打开。此例外必须与正常下载暂停后保留页面
  区分，也不能声称它已经提供无损恢复体验。
- 被取消的点击仍可能真正完成导航；它的迟到结果不得回写 Host。用户恢复后
  显式重新授权时，使用新请求身份刷新原 page 的真实 generation，再发布授权。
  先本地检查目标归属，跨 run 请求不进入远端页面刷新。

## 首败与修复证据

日志根：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

1. `native-cancel-first-red-02.log`：真实 App Broker 下，持有下载正文时后台未
   收束；取消导航后恢复使用旧 pageGeneration；慢截图的忙碌保护通过。
   更早的 `native-cancel-first-red.log` 缺 fixture pageGeneration，是探针错误，
   不作为产品失败证据。
2. `download-stream-first-red.log`：流只检查下一块数据才发现取消，停流时一直
   阻塞。首修又暴露 Windows 文件 close 时序，`download-stream-green-02.log`
   的三个测试通过。这个单测不能证明 Chromium 原生传输也已经停止。
3. `native-cancel-core.log`：402 passed / 2 failed。新的 reauthorize 页面刷新
   遇到旧 LoopbackHttpWorker 未实现 list_pages；跨 run 的错误先在 Host route
   返回，改变了原 adapter 的拒绝顺序。补齐静态页面 fixture、将归属检查移回
   刷新之前，保留两项原断言。`native-cancel-core-02.log` 为 404 passed。
4. `native-cancel-source-green-01.log` 仍实际失败：导航和截图已恢复，下载仍挂起。
   隔离 Chromium `native-download-isolated-first-red.log` 明确没有 Download
   event，只进入了 HTTP 请求。这是下载开始前的等待，不能冒充“已拿到原生
   下载对象”。`native-download-header-diagnostic.log` 证明 2048 字节时进入
   真 Download，已接线的 cancel 能完成。
5. `native-download-pre-event-first-red.log`：保留五字节正文直到旧四秒事件
   等待结束，断言发现函数已经返回、HTTP 传输却仍活着。修复后
   `native-download-pre-event-green.log` 验证关闭 owned context 后才完成。
   真实探针现在分别保留五字节 pending-start 和 2048 字节 native-transfer
   两行，不以更改数据量掩盖前一种失败；native-transfer 的三秒空闲限制不变。
6. `plain-download-cookie-first-red.log`：仅给 `/private` 的虚构 fixture Cookie
   被普通下载错误发到 `/slow.bin`。按 URL 取 Cookie 后完整回归通过；重定向
   测试也检查每跳重新选 Cookie。所有值均为隔离 fixture，不读取用户登录资料。

## 当前验证记录

| 门禁 | 当前结果 | 日志 |
| --- | --- | --- |
| Core library | 404 passed，0 ignored | `native-cancel-core-02.log` |
| Driver integration | 12 passed，0 ignored | `native-cancel-driver.log` |
| 严格 Core Clippy | exit 0 | `native-cancel-clippy-core.log` |
| 完整 browser Node | 122 passed，0 skipped | `native-cancel-browser-final.log` |
| 最终 App Clippy | exit 0，严格 warnings 门禁 | `native-cancel-clippy-app-final.log` |
| cu_probe 重建 | exit 0；既有 linker 创建库 stdout 提示仍保留 | `native-cancel-probe-build-final.log` |
| source 真链 | 四行全部 passed，exit 0 | `native-cancel-source-final.log` |
| seed 真链 | 同样四行全部 passed，exit 0 | `native-cancel-packaged-final.log` |
| 实测后 seed check | exit 0，manifest/tree 未漂移 | `native-cancel-seed-check.log` |
| cargo fmt / diff whitespace | exit 0 | `native-cancel-fmt-check.log` / `native-cancel-diff-check.log` |
| quality final | PASS，千行文件 80/80 | `native-cancel-quality.log` |

Node 命令先用 `rg --files tools/computer-use-browser -g '*.test.mjs'` 确认显式列表，
再用 owned App 私有 Node、`--test-concurrency=1`、仅 System32 PATH 运行。
完整 Node 回归使用显式 CfT chromium-1223；真实 App probe 使用 seed Chromium。

source/seed 的两种下载均先证明确实发起 fixture HTTP 请求及 worker 活动请求，
再暂停；本地两秒内返回非成功，远端在持有正文时完成取消/关闭。普通传输三秒内
空闲且保留页面；五字节 pending-start 在原四秒事件期限后关闭 context，旧目标
不得重新取得授权。两者均恰好一次请求、没有最终文件或 `.part`。
点击/截图暂停后仍物理忙碌时拒绝 resume，fixture 释放后才恢复并重新观察。
点击的恢复同时验证重新授权取得新 pageGeneration。原有 preview/Wait 页面点击
oracle 在两套 worker 中仍各恰好两次，暂停/恢复不添加输入。

`native-cancel-browser-full.log` 是之前误清 PATH 导致 rg 不可用、Node 自动发现
到范围外测试的无效矩阵；175 项中有私有 Cua/extension jsdom 环境失败和跳过，
不能列为本模块验收结果。之后正确的显式全集为 119 项，新增本批三行后为 122 项。

## 工件与边界

266 文件局部源码指纹：
`2896FA733AE9BAA5F189CB919A65EEED062FDD135818EACAB2304CAE88C08C44`。
范围和算法同 managed Wait 检查点：指定的 Core/App Computer Use、browser、
extension、MCP 和前端域文件，经 rg 枚举、路径 `/` 化、Sort-Object 排序，
每行 path + 空格 + 文件大写 SHA256，用 UTF-8/LF 无末尾换行合并后再散列。
这是局部追溯记录，**不是整个候选冻结**。日志 `native-cancel-source-fingerprint.log`。

- seed manifest：`17cf11bec4d8572f4a2c6c15f5309c30d4e1be0361432d2d3e02a6c40a3affe1`。
- seed tree：`3210ebe5e11aca100c17c2554911a800ea441f5c502ffcd0ac8ace28b65afd80`。
- cu_probe SHA256：`7B65456E0A5A9CEB24F19020DCE44D1DB5E770D4C9C6B5E06BB8CD43F68FA255`。

seed 通过标准 `cu-prepare-runtime --prepare --target x86_64-windows` 生成，没有
手工修改打包副本或放宽哈希验证。最后只读进程核对未发现本批 cu_probe、私有
Node 或 managed-contract profile Chrome 残留；分支/HEAD 不变、index 为空。
Git 对部分既有 tracked 工作副本提示以后会转换 CRLF，whitespace check 本身
exit 0；没有修改仓库/全局行尾设置。本批涉及的新增源文件检查为 LF、无尾空格。
一次诊断命令临时关闭 autocrlf 后把既有 CRLF 行都当成尾空白，产生了大量与本批
无关的输出；没有写配置或改文件。恢复使用仓库正常换行归一化后检查 exit 0、
输出日志为空。它不证明整个已有工作树已经统一 LF。
本批没有重跑全仓前端、扩展/MCP、安装、其他 OS 或真实模型，不复用旧通过数。

## 原修复批次的接续顺序（后续进展见末节）

1. 增加真实原生启动与 pending 操作的 terminal Stop、恢复回复丢失和迟到
   Pause/Stop 竞争；各 browser 业务路由还需独立身份/页面后置条件。
2. 收尾 App/ACP 生命周期与 UI；无原生 Download 句柄时 context 关闭后的
   重新打开体验要在真实 App 中验收，不能用 Broker 拒绝旧目标代替 UI 可用。
3. 继续 ExistingTab typed actions、原生 toolbar activeTab/截图、Edge、安装
   升级回滚卸载、macOS arm64/x64、Linux X11/GNOME native Wayland、真实模型。
4. 冻结同一候选后至少 12 小时主动长稳。旧 IPC Windows 10053 偶发根因仍未
   确认，后续局部通过不自动关闭此项。

未提交、推送或创建 PR；默认关闭不变。保留全部用户/Grok 改动。未动真实账号、
日常浏览器资料、代理或此前审批拒绝删除的两个空测试目录。

## 增量：真实 terminal Stop 与恢复响应丢失

本增量仅增强 probe/验收链，没有修改产品 Core、worker JS、UI 或默认设置。
`browser_native_contract.rs` 对已有四行再执行一次 terminal Stop；另两个新增
probe 模块 `browser_resume_fault_contract.rs` / `browser_resume_relay.rs` 在真实
worker 前放置只监听 literal loopback 的临时中继。Bearer 仅保存在内存、没有
打印凭据或增加产品调试路由；退出时 release、shutdown、join 全部归还。

新增七行均经真实 App Broker、HTTP client、Playwright worker 与 Chromium：

| 新增场景 | 独立后置条件 |
| --- | --- |
| 下载中 Stop | 正文仍被 fixture 持有时，HTTP 传输结束，远端 stopped/idle，没有最终或半截文件 |
| Download 对象建立前 Stop | 本地先 stop_requested，取消打开中的传输；无需等待 fixture 放行 |
| 点击导航中 Stop | 关上下文并收束请求；旧目标、观察和 resume 拒绝，迟到完成不复活 |
| 慢截图中 Stop | 在 held font 尚未放行时完成远端清理，不把本地 socket 取消当清理证明 |
| 恢复已应用、HTTP 回复中断 | 独立 run-status 确认 revision=2/running，再中断真实响应；本地保留 unknown、不可重发 |
| 恢复已应用、回复被新 Pause 覆盖 | 新 Pause 先本地 fence；迟到成功不恢复权限，只能终态 Stop 收尾 |
| 恢复已应用、回复被 Stop 覆盖 | 先清理真实浏览器，但本地 resume admission 未返回前仍 stop_requested；返回后才 stopped |

每行 native Stop 旁边还保留另一个 owner 的真实 context，确认它仍在。三个
恢复竞争都断言只发出一次 `/resume-run`，Stop 后真实 worker 为 stopped/idle，
并让新 owner 重新打开同一 profile，验证旧浏览器已释放锁，而非只改本地标记。
这些是故障注入的真实传输和页面资源验收，不是 App UI 或真实模型验收。

`native-stop-resume-source-first.log` 第一轮已通过；没有人为制造红测或修改产品
绕过失败。随后补上丢回复必须为 `WorkerCompletion::Unknown`、每行最终清理
必须为 `Stopped` 以及清理耗时，重建后在 source/seed 分别完整重跑：原四项
暂停 + 四项 Stop + 三项恢复回复竞争，共 11 行均通过。原有 Wait、preview、
两个点击 oracle 和 Host profile 门禁同时通过。

| 当前增量门禁 | 结果 | 日志（仍在同一日志根） |
| --- | --- | --- |
| 严格 App Clippy | exit 0 | `native-stop-resume-clippy-final.log` |
| cu_probe 重建 | exit 0，既有 linker stdout 提示保留 | `native-stop-resume-build-final.log` |
| source 全部真链 | PASS，含 11 行 | `native-stop-resume-source-final.log` |
| seed 全部真链 | PASS，同一 11 行 | `native-stop-resume-packaged-final.log` |
| 实测后 seed check | exit 0，manifest/tree 与上批一致 | `native-stop-resume-seed-check.log` |
| fmt / diff whitespace | 均 exit 0 | `native-stop-resume-fmt-check.log` / `native-stop-resume-diff-check.log` |
| quality final | PASS，千行文件 80/80 | `native-stop-resume-quality.log` |

本机受控 fixture 的 Stop 清理阶段耗时：source 下载/建立前/导航/截图为
170/108/109/110ms，seed 为 204/95/104/137ms。这是清理 RPC 加远端状态确认，
不含浏览器启动、App UI，也不是用户页面或其他平台的性能承诺。

Core 404、Driver 12、Node 122 是上一批命令；这次没有更改那些产品源文件，
未重复运行这些全集，也没有将它们记成此次新增验证数量。

增量局部指纹范围和算法同上，新增两个 probe 模块，共 268 文件：
`57F6FE60E862E98F0AA67C764295343EB5F5CEF0CA1C0680A64A0788D15FEF11`。
cu_probe SHA256：`E163FDB2F93149018C4928FD807A9887BBAA695E47EB960A15C359BE7177AE6A`。
日志 `native-stop-resume-fingerprint.log`。不是完整候选冻结。最后只读进程
检查无本批 probe/私有 Node/managed-contract Chrome 残留；HEAD 与分支不变，
index 为空，未 commit/push/PR。

下一轮优先接通 ExistingTab 的 typed 操作链，同时保留以下未完成项：

1. 原生 Chromium 启动内部的 Pause/Stop，其他业务路由的身份/后置条件；本次
   delayed resume 只延迟控制回复，绝不冒充 native launch 内部竞争。
2. ExistingTab 的有类型动作协议、Host/扩展两侧权限/请求生命周期、真实
   click/fill/key/scroll/wait/navigation 及 MCP 独立页面后置条件。当前代码仍
   只有 Observe 命令，adapter act 明确 unavailable，不能宣称已经能控制已有 tab。
   已交付请求的取消需要远端结束证据；现有只读观察的队列删除逻辑不能直接
   充当动作停止证明。按键必须验证实际默认行为，合成 KeyboardEvent 本身不算成功。
3. 无 Download 句柄时 context 关闭后的 App 重开体验、C4 App/ACP 生命周期；
   toolbar activeTab/截图成功、Edge/安装、三 OS、真实模型及冻结后 12 小时长稳。
4. 旧 IPC Windows 10053 偶发根因仍未定位。
