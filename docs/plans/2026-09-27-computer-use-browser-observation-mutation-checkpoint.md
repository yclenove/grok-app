# Computer Use：浏览器观察与输入事务检查点

本机时间：2026-09-27，Asia/Shanghai。分支 `feat/computer-use-implementation`；基准 HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。

目标仍是接手 Grok 的完整 Computer Use 并推进最终版，**active，未完成**。本检查点记录真实代码修复及其覆盖边界，不以该局部通过替代全平台、全入口、完整输入/恢复、安装、UI、真实模型或长稳验收。未 commit/push/PR，未替换用户 App/运行时，未授予系统权限。

## 1. 本轮推进与先失败证据

开工时核对上一轮 `macos-native-fixture-20260927/source-receipt.json` 的 294 项指纹，未发现漂移；上一轮分类为 progress，而不是等待某个尚未验证存活的后台任务。

本轮在 managed browser 的观察/输入并发路径复现以下问题，红测均实际报 `Missing expected rejection`，没有修改测试期望来掩盖问题：

1. 点击等非导航输入已完成后，较早开始的截图/控件采集仍能发布为新观察；模型与预览均需拒绝。
2. 原生输入 Promise 尚未结束时，新的观察仍读取页面，并把中间状态当成空闲状态。
3. 新观察替换旧 map 后，等待旧句柄释放期间发生输入，已失效的新观察仍可能被返回。
4. 没有 document/input 事件的显式观察撤销，未拦截尚未发布的采集。
5. 最后一个未保留句柄的异步释放期间发生撤销，已退休的模型 envelope 仍可能返回成功。

第一组 `capture-red.log`：0/3，exit 1；第二组 `retirement-red.log`：0/2，exit 1。红测保留在本轮证据目录。

另外发现 run 级未知结果恢复不能只在观察返回时检查 `!inFlight`：另一标签页的旧观察可能跨过一项新动作的未知结果，错误解除隔离。加入 action admission epoch 的单元与真实 Chromium 回归；未把这一项计入上述五个已保存红测。

## 2. 实现

源码集中在 `tools/computer-use-browser/`：

- `page-state.mjs`：单独维护不可回绕的观察 epoch；显式撤销也淘汰未发布采集。跟踪 mutation Promise 占用，拒绝重叠 mutation 和输入中观察；成功/异常路径都正确清理该占用。
- 文档身份与观察身份分开。保留现有 `capturePageStateIdentity` 合同，使已准备拖拽的文档 guard 不会被动作自身入场误判为失效。
- `observation-extract.mjs`：采集、发布、旧句柄与未保留句柄释放构成完整事务；在**最后一次 await 之后**再次检查取消、页面/epoch 身份及当前 map 归属。失败只退休/释放自身观察，不复活旧 map。
- `worker-ledger.mjs` / `server.mjs`：在 `/observe` 开始时记录 slot 与 action epoch；只有同一 slot、无重叠新动作、当前已空闲的模型观察可解除 unknown 隔离。preview 不解锁，原 unknown actionId 永不重放；已完成动作重放与被拒准入不推进 epoch。
- `.github/workflows/ci.yml`：增加五个既有/新增事务单元文件的本地可复现命令；本轮未运行远端 CI。

这里的 `mutationInFlight` 只说明原始 Promise 占用，不声称已证明所有平台原生输入队列停止。未知完成与恢复仍遵循原有更高层隔离，未增加重试、放宽导航 8 秒期限或伪造完成。

## 3. 真 Chromium 证据，不是替身截图

新增 `observation-mutation-http.test.mjs` 与私有 `fixtures/capture-barrier.preload.mjs`：

- preload 仅注入本测试创建的 IPC 子进程；在**真实 Chromium screenshot 已取得 PNG 之后**阻塞回包，回报实际字节数/指纹，不合成图片或动作结果。
- 夹具通过真实点击改变 input 与计数器；只读 oracle 回读实际 DOM。跨标签页未知结果使用本地 HTTP 未完成导航，生产导航期限保持不变。
- 覆盖 model/preview 旧截图拒绝、pending navigation 中观察拒绝、跨页迟到观察不能解除 unknown；新的模型观察可以恢复，但未知 actionId 不可重放，服务器实际只收到一次原导航请求。
- 清理检查 `/pids` 的 workerPid 必须是原始子进程、一个 owned browser PID；确认生产 shutdown 200、同一 worker exit 0、原 browser PID 不存在后才删除 owned 临时 profile。强制关闭是失败路径，不被记录成正常通过。
- 测试 preload 不在运行时打包 allowlist 中，实际新包也不存在该文件；生产 server 未加入测试控制接口。

## 4. 最终回归及失败保留

证据目录：`tools/computer-use-probe/.run/browser-observation-mutation-20260927/`。

| 检查 | 结果与边界 | 日志 |
| --- | --- | --- |
| 五文件事务回归 | 51/51，exit 0；包含在选定 worker 回归内，不重复累加 | `transaction-final.log` |
| Windows 选定 worker 首轮 | **153/154，exit 1**；Unicode/空格 profile 的原生进程发现返回 null，而不是期望 PID 68076 | `windows-worker-unit-final.log` |
| Windows 相同源码再次执行 | 154/154，exit 0；不代表首轮发现失败根因已修复 | `windows-worker-regression-r2.log` |
| Linux 选定 worker | 154/154，exit 0；该选择集合含原生 `/proc` 子进程发现，不全是 mock | `linux-worker-unit-final.log` |
| Windows 新隔离包 + 真 Chromium | 31/31，exit 0，约 70.87 秒；五个文件，含本轮 5 项竞态、真实 typed actions/拖拽/wait/观察句柄生命周期/HTTP 错误合同 | `windows-pack-native-final.log` |
| Linux 当前源码 worker + pinned Node/Chromium | 本轮原生竞态 5/5，exit 0，约 17.00 秒；不是 Linux 新打包/安装验收 | `linux-capture-native-r2.log` |
| 早期 Windows 原生竞态 | 5/5，exit 0；发生在最后两项撤销修复前，只作过程记录，最终结论使用新包回归 | `native-first.log` |
| 最终静态检查 | 9 个编辑 JS 文件 `node --check`；CI YAML 可解析且事务命令含精确五文件；7 个聚焦文件 ESLint 通过 | `syntax-ci-final.log`、`focused-eslint-final.log` |

选定 worker runner 明确排除九个需要专门原生环境的文件，清单见 `run-unit.mjs`；没有声称执行整个仓库、全部 browser 原生套件或所有平台功能。

保留其余执行问题：

- Linux 原生首次启动命令尾部多了 CR，报找不到 `test.mjs\r`，**exit 1，不是原生测试通过**；只修正证据 Bash 文件的 LF，原测试源码未改变后再运行。
- 初次九文件 ESLint exit 1：原有文本清洗 control-regex 规则及本次命令缺少 Node `AbortSignal`/`setImmediate` globals。未关闭规则、未将全范围 lint 说成通过；七文件聚焦检查使用完整 Node globals，另两文件只报告语法通过。
- CI YAML 初次解析工具选择了不存在的顶层 `yaml` 包；改用项目 ESLint 已安装依赖 `js-yaml` 校验，不修改依赖、不假称首次成功。

## 5. 独立运行时包与发布问题

使用生产 `scripts/prepare-computer-use-runtime.mjs`，没有手拷 worker 冒充已重建 pack。

1. H: `.cu-probe-observation-mutation-20260927/seed` 发布失败：`AccessDenied (os error 5)`，staging 清理同时报文件使用中 `os error 32`。失败目录保留。
2. 确认前一准备进程终止后，在 H: 短路径 `.cu-obs-0927/seed` 再做一次独立准备，仍 `AccessDenied 5`。**短路径没有解决问题**，不据此断言具体根因。
3. 新建本轮独占的 C: Temp 目标，生产准备成功，`importProbe=passed`。该路径由 `isolated-seed-path.txt` 保存，不是用户已安装的运行时目录。
4. 使用该包的 Node、Chromium、`playwright/worker.mjs` 执行上述 Windows 31 项原生回归；测试后生产 `--check --target x86_64-windows` 再次 exit 0 / importProbe passed。

已验证包指纹：

```text
manifest 001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec
tree     6fdd22b23b1798cde0aa8d2c82784e8abbcf0967741cbc5c355ffa8b93d102d8
chromium 63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3
```

`packed-modules-final.json` 记录四个生产修改文件逐字节 SHA-256 对齐：`server.mjs` 对应包中 `worker.mjs`，其余同名；私有 barrier fixture 确认排除。

`source-before-final.json` / `source-after-final.json` 冻结核对 239 项，零漂移。该范围包括本轮源代码/测试/CI 与相关既有实现，不等于整个仓库所有文件冻结。

Windows scoped `Win32_Process` 回读未发现本轮 isolated seed 可执行文件/测试 marker 的残留子进程；Linux 另有 scoped `/proc` 回读。仅观察明确 owned 标识，不按进程名杀用户浏览器。测试中自己的 shutdown/PID 断言仍是主要清理证据。

## 6. 不关闭的最终版门禁

- H: 运行时发布/清理失败、Windows 原生进程发现偶发 null 的根因继续追踪；C: 成功和单次复跑通过不是它们的修复证明。
- 历史浏览器导航偶发 504 和退出长尾未由本轮竞态修复证明解决。故障夹具预期的 504 也不能与历史异常混为一谈。
- Windows/macOS arm64+x64/Linux X11+GNOME native Wayland 的完整原生能力与生命周期、取消、恢复；Mac 真实 Swift/SDK/输入/IME/剪贴板及自绘控件边界仍须验收。
- Desktop、managed browser、existing tabs、App WebView；App/ACP/MCP 的完整功能与权限/恢复合同，不能用本轮 managed browser 局部覆盖替代。
- 签名安装、更新、回滚；重设计原生 UI 的当前源码窄窗/缩放/权限路径；真实 Grok E4 与冻结 12 小时 active soak 都仍开放。

下一步以当前工作树及失败记录继续定位 runtime 发布/进程发现可靠性，并继续原有跨平台与原生 UI 等实现/验收，不把目标缩为“观察竞态测试通过”。未写 complete/paused/blocked；当前 goal 实际读取为 active。
