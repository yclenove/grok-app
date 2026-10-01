# Computer Use：受管浏览器预览隔离检查点

日期：2026-09-20；工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`；HEAD `30757366`；未提交/推送/PR。
总状态仍为 **partial — not releasable**。这是 C3 观察一致性的增量，
不是完整 Computer Use 或 ExistingTab 动作闭环完成。

## 产品改动

- `CaptureOptions` 的 model/preview、screenshot 和 cancellation 经过 managed
  adapter、Host、Loopback worker，一直传到真实 Playwright capture。
- `screenshot:false` 在生产 worker 中不调用 screenshot；preview 产生独立
  snapshot，但不替换 worker、Host、Broker 的模型 snapshot/ref。
- 本次观察创建的隐藏/丢弃/超预算 handles、preview 的临时 handles 以及异常
  路径 handles 会释放；模型当前可用 handles 保留。旧模型 snapshot 的完整
  handle 生命周期仍有技术债，不能把此次局部清理称为没有泄漏。
- Host 在同一锁内复验 tab/run/document/cancellation 并提交模型 refs；preview
  不提交模型 refs/URL。不再先更新 page 再跨锁更新 refs。
- worker `/health` 声明 `page.observeOptions:1`。App 发 preview 或 text-only
  请求前检查此版本；旧 worker 返回 typed `observation_options_unavailable`，
  不向会忽略新字段的旧 worker 发送观察请求。普通 model+screenshot 保持兼容。
- worker 的 preview 不解除 unknown 写入锁；仍须新的模型 observe 才可恢复。

## 本批回归发现并修复的实际问题

### 1. 运行时启动会污染自身哈希

最初的 post-run `--check` 失败并非原始二进制被改。逐项对比已锁定的 Chromium
ZIP，84 个原文件字节一致，新增 `debug.log`。单独的 `--log-file`、环境变量
CHROME_LOG_FILE、stderr/disable-logging 尝试均未解决；失败记录未删除。

实际对照确认 Playwright 1.48 默认的 `--headless=old` 在此 Windows Chromium
130 中产生 GPU 日志。改为 Chrome unified `--headless=new`，保留 profile 内
diagnostic log 路径；去掉无效的环境覆盖和 stderr 试验配置。

新无头模式又暴露 Windows Chromium 把 Hunspell 词典存放到 executable 目录
`Dictionaries` 的行为。受管浏览器现在只在 **App-owned profile 启动之前**
原子合并 Preferences：关闭 spellchecking/在线 spelling service，清空默认
dictionary/dictionaries，保留其他配置。不修改 ExistingTab 或日常浏览器。
损坏、非对象、超 4 MiB、符号链接配置拒绝；不以覆盖损坏配置继续启动。

没有放宽 runtime tree hash、忽略额外文件或修改 Git/系统代理。生成器正常
重建 seed 前，新增日志和词典都保存在证据目录。`observation-http` 也检查
运行前后的整个 executable 目录文件清单/size/mtime；最终 seed 工具另做完整哈希。

源代码依据（版本与当前锁定 Chromium 一致）：

- [Chrome logging](https://github.com/chromium/chromium/blob/130.0.6723.31/chrome/common/logging_chrome.cc)
- [Windows dictionaries 路径](https://github.com/chromium/chromium/blob/130.0.6723.31/chrome/common/chrome_paths.cc)
- [拼写配置键](https://github.com/chromium/chromium/blob/130.0.6723.31/components/spellcheck/browser/pref_names.h)

### 2. 测试 HTTP oracle 提前确认不完整正文

旧 App probe 服务对 TCP 只 read 一次，可能仅收到 header 就返回 204，没有
记录点击正文；其他 URL 还错误地返回带 iframe 的同一表单，产生递归 iframe。
新分包用例先只发 header，验证在 body 到达前不能收到成功回复，真实触发首败。

probe 现复用已有 Axum 依赖，完整解析 JSON，限制 4096 字节，分别路由 form、
真实 frame fixture、oracle。该服务仅编入 probe。真实按钮仍经实际页面 fetch
更新独立计数器，不把 tool 的 applied 返回值当作页面成功。

### 3. 私有环境漏报浏览器 PID

完整 Node matrix 的进程回收检查失败：最小 PATH 中没有 PowerShell，旧
`childPids` 靠 PATH 查找导致空列表。改为 Windows SystemRoot 下的绝对
PowerShell 路径，仍隐藏子进程，并排除查询 helper 自身。新进程测试真实
创建 Node child，在最小 PATH 下精确识别它并回收；未扩展环境继承范围。

## 验证和证据

证据目录：`tools/computer-use-probe/.run/r2d/pairing-20260919T155500Z/`。

| 检查 | 结果 / 文件 |
| --- | --- |
| Core all-features | 最终复跑 362 passed，0 ignored，31.15s；`managed-preview-core-final-02.log` |
| Driver integration | 12 passed，0 ignored，0.16s；同上 |
| 全部 15 个 browser Node 测试文件 | 83 passed，0 skipped，33.91s；`managed-preview-node-final-02.log` |
| extension + MCP golden | 54 passed，0 skipped；`managed-preview-extension-final.log` |
| Core strict Clippy | all-targets/all-features + `-D warnings`；`managed-preview-clippy-core.log` |
| App strict Clippy | default 和 computer-use-probe、all-targets + `-D warnings`；`managed-preview-clippy-app.log` / `managed-preview-clippy-probe.log` |
| Rust fmt / diff whitespace | exit 0；保留已有 LF→CRLF 提示，不改 Git 配置 |
| 扩展 locale / code-quality final | 15 locales；final PASS，千行文件 80/80 |
| 最后一次 seed 重建后的 source/packaged preview | 两者 exit 0；`managed-preview-source-final-04.log` / `managed-preview-packaged-final-04.log` |
| 浏览器退出后的完整 seed 哈希检查 | exit 0，运行前后 manifest/tree 一致；`managed-preview-seed-prepare-05.log` / `managed-preview-seed-check-final-04.log` |
| 实际 App 私有 Node/MCP + ExistingTab MV3 | 34 passed，exit 0；`managed-preview-existing-final.log` |

Node 实际浏览器测试使用独立 Chrome for Testing；App managed preview 使用
私有 Node 20.18 + 锁定的 Chromium 130。源码 worker 与生成 seed worker 分开
运行，probe 打印实际路径。两者均验证 text-only 模型 observe → PNG preview →
旧 model ref 真 click → 独立计数器从 1 到 2；同时执行旧 worker 拒绝及分包断言。
它们不是安装版 Tauri UI 验收，也不证明用户已经授权截图或真实模型操作成功。

首败与中间结果保留：

- `managed-preview-red.log`：preview 替换 model snapshot、text-only 仍截图。
- `managed-preview-core-01.log`：测试引用不存在的 tempfile 依赖，是编译失败，
  未到断言；已改为项目既有 UUID owned-temp 模式。
- `managed-preview-seed-check-final.log`、`managed-preview-seed-check-env.log`：
  日志污染；`managed-preview-binary-log-red.log` 为真实失败断言。
- `managed-preview-chrome-log-matrix.log`：disable-logging / log-level 各新增 93
  字节，unified headless 新增 0 字节；不删除或改写旧失败。
- `managed-preview-app-stderr.log`：一次 worker_internal/Unknown，直接 launch
  和下一次相同配置能运行，但仍污染日志；未归因为产品操作成功或算作通过。
- `managed-preview-source-new-headless.log`：第二次点击 oracle 计数未收齐。
  `managed-preview-oracle-red.log` 进一步确定 fixture 过早响应；修复未增加
  点击重试、放宽计数或延长 action timeout。
- `managed-preview-seed-check-final-02.log`：新增 Hunspell 字典导致 tree mismatch。
- `managed-preview-node-final.log`：81 项中 79 pass，1 子用例和父套件失败；
  `managed-preview-pids-red.log` 确定最小 PATH 下 PID 发现失败。修复后 83 全过。

链接器 build 仍有既有「正在创建库」stdout 警告；strict Clippy 独立通过，
没有禁用 lint。没有在本批改前端产品代码，未把旧前端验收当作本批新执行。

局部源代码指纹（不是 C6 freeze）：228 文件，
`8B8C55C6B778E4C1CCD6D19D0024FAE9C261A89ADBD916E26632A9324F496303`。
覆盖 Core src、App computer_use、tools browser/extension/MCP/probe 中 rg 可见的
rs/mjs/json/html/css；路径 `/` 规范化后排序，逐行 `path + 空格 + 文件 SHA256`，
LF 拼接、UTF-8、无末尾 LF，再 SHA256。未覆盖完整 App/锁文件/安装工件。

探针 SHA256：`16D0105E04036A460C18BD21E3C7CD88809F908862E9A58085AD28C07BB5E258`。

最终生成 seed manifest SHA256：
`32d02879416175cd612b971c431933d03e2405a136ac79f75d94da52343896bb`。
seed tree：`e39e6db9b770bf21b255366ed98a31342a221cff551b17d24891db0b2178a630`。
Chromium 原 tree hash 仍为
`63c6075faf6d986bc8e9f11996f5d2e784e7f4d9471366b7a2ad6eebaca245e3`，
没有改为包含新日志或词典的 hash。

真实 ExistingTab 集成再次确认 Stop 取消 pending observation、用户 tab 仍打开，
本次测得 4ms，仅为该 fixture 结果，不是 SLA。工具栏授权后的截图成功仍未验；
此检查中的 capture permission denial 不能算正向截图成功。C3 managed 与
ExistingTab 是两种实际 adapter 路径，不把其中一个的通过移用于另一个。

本批结束再次检查：没有 cu_probe、本轮私有 Node 或本批 owned-profile Chromium
进程残留；index 仍为空、HEAD 未变。生成 seed 的替换只涉及可再生成资源；
发现的日志/词典已归档，未删除用户资料。

## 下一批，不能省略

1. Broker observe/preview/action 的完整事务互斥、模型观察提交顺序和预算；
   extension 一条 pending 不是 Host→Broker 提交阶段一致性的证明。
2. managed 的旧 model handles 要有 action-aware 生命周期；不能在
   runPageMutation 失效快照时销毁已经准备、尚未执行的 action handle。
3. managed observe 的 loopback HTTP 目前只有取消前后检查，进行中 HTTP
   仍可能等到 15 秒 deadline；不能声称像 ExistingTab 一样立即中断。
4. C3 ExistingTab typed click/fill/key/scroll/wait/navigation、真实副作用/no-replay/
   Stop/导航竞争。当前 ExistingTab 动作能力仍 NONE；browser_* 与 navigation/
   download 的 managed-only 语义也待统一。
5. 继续检查 profile 并发 open/launch 槽位及多 profile 的 PID 精确归属；本批
   PID 修复证明查询不再因 PATH 漏报，不证明所有跨 profile 生命周期已经完成。
6. C1/C2 原生 toolbar activeTab 与截图 happy path、C4 两轮 App-shell/ACP
   生命周期、C5 安装/Edge/macOS arm64+x64/Linux X11/native Wayland，以及 C6
   真实模型、同一候选版本至少 12 小时主动长稳，全部保留。

单 writer、重测试串行、不碰账号/日常资料/代理、不提交推送；本批是有代码及
实际测试的 progress，不是 final/complete，也不是因外部设备缺失而空转。
