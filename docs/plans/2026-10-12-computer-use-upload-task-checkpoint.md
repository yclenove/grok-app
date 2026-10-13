# 上传原任务接线与任务内文件选择

2026-10-12，`feat/computer-use-implementation`，单 writer；保留原 WIP，未提交、push、PR 或发布。
本批是 [U1–U4](2026-10-12-computer-use-upload-integration-next.md) 的实质增量，不是完整交付。

## 产品代码已接

- Core 原 observe 身份上传意图，封存原 TaskGrant/transport/agent/run/target/document；
  模型不得提供路径、URL、selector、code 或文件字节。每 run 单一 pending，预算/账本只计一次。
- 原 `WebViewUploadCall` 持有等待；Host 用 exact execution address 与 opaque requestId 选择/取消。
  cancel 只退休原意图，不释放仍活的 native picker/assignment owner。用户取消不是红色动作失败。
- 文件实际选择后才打开原 handle，有界读取不可变内容；空文件支持，最大 8 MiB，超限不截断。
  MIME 根据所选 filename 推断，未知为 application/octet-stream；诊断不输出原路径/内容。
- stage/最终发布在原 Broker/native document guard 下完成；迟到结果 unknown，不重放。
  immutable bytes/metadata digest 加入原 replay ledger。字段赋值 verified 不代表网站提交成功。
- App 主 WebView 的 choose/cancel IPC、原 SessionManager/ACP execution pin、owned control registry 已接。
  新 TaskGrant 不等于任意本地文件读取权限。
- 聊天任务卡与 Computer 侧栏共用上传事务：按需显示目的页面/字段、选择并上传/取消、选择中/
  赋值中/取消/未知/失败/成功。保留原 chooser lock 穿过最后 UI 卸载和重开；旧 owner 回复不闪回。
  十五语言与现有 chrome、窄屏折行已接，不增长 App/AppWorkbench。

## 本批实际证据

路径均相对于此工作区；所有结果按运行作用域分类，不能互相冒充。

- `src-tauri/target/codex-test-runtime/app-upload-ui-core-full-20261012-01.log`：
  当前 Core default libtests **932 passed / 0 failed / 0 ignored**。
- fresh App Cargo `app-upload-ui-lib-build-20261012-02.jsonl` 的唯一 libtest executable，
  复制到 `app-upload-ui-unit-20261012-02/app-tests.exe`，嵌入项目 Windows manifest 并回读。
  SHA256 `91d6e3acbcd3ac3f06024a37c21aa46fd1bb31fddd352420f83cdfc481013871`。
  原列表确认各 filter 非零，去重实际 **201 passed / 0 failed / 0 ignored**；覆盖新 reader、
  WebView、原 owned control lifecycle 与 ACP CU。不是全部 App 测试或真实 picker。
- `app-upload-ui-vitest-20261012-02.log`：CU components/domain、task API、i18n 共
  **450 passed / 0 failed**。新上传 domain 15 项、API 2 项、UI 4 项包含精确关联、取消/
  原 chooser 占用、卸载重开、迟到旧回复、空文件成功与诚实错误。
- `tools/computer-use-probe/.run/task-consent-ui-2026-10-11T17-41-58-306Z/report.json`：
  实际 Chromium 渲染 **198 passed**，含新增上传 **36 场景**（十五语言 × 深浅主题 × 300px，
  六种 outcome/暂停/停止/取消）；目的页面、按钮、零溢出与单次 IPC 已验。
  手动查看中文 300px 与双入口成功截图，布局无错位。Host IPC/文件选择仍 synthetic。
- Node 本地 WebView fixture **3 passed**；session MCP 回归 **41 passed**。
- 第一候选 `app-upload-task-20261012-01`：真实 App main IPC/Wry 固定上传内核/原 TaskGrant
  **8 场景**，独立 receiver 恰好 **3 POSTs**，中文/二进制/空/8 MiB hash 正确，零 replay；
  同候选 download Close **2** 与 adapter upload **8** 通过，原 App/browser owners 退出。
- 最终候选 `app-upload-ui-probe-build-20261012-02.jsonl`：见下节最终收尾记录。

首败保留：新增 Core cancel 测试最初直接读未重新结算的 raw StopState，随后改用原
`stop_state()` 的真实收尾见证，未放松占用/最终 Stopped 断言；UI 首轮 4 项因测试 helper
导入在 product/API 之后、mock 绑定不一致而失败，修正测试导入顺序后 450 项通过。
最初 App filter 名无匹配时命令直接拒绝，没有把零测试算覆盖。

## 仍未完成，不能声称上传已可交付

1. `computer_upload` schema/preflight/真实 transport dispatch/session MCP catalog/ACP permission/
   模型提示与真实原会话续行仍未接齐，模型工具仍不公开、旧 dispatch 继续拒绝。
2. MCP `hostCall` 当前有 **15 秒 socket idle timeout**。不得把等待用户选文件直接塞进
   普通同步工具后宣称可用，或抬所有工具全局超时。下一批须保留原 call/身份/取消者，
   做上传专属等待/结果回传与原 MCP/ACP 断连实测；模型不轮询、不新建聊天、不重放。
3. `rfd 0.15.4` 无公共强制关闭/cancel handle。Stop/IPC Drop 可退休权限，但不能宣称
   系统 dialog/thread 已退出；原 registry/Quit barrier 必须等实际 picker 返回。
   真实用户选择/取消、dialog-held Pause/Stop/Quit、原 picker close 能力与 UX 仍待验。
4. native TaskGrant 验收的 Host approval/file choice 为 **synthetic**，realNativeFilePicker/
   realACP/realMCP/realModel 全为 false。真实用户文件同意及完整端到端尚未证明。
5. 其它 surface 同意链、Mac arm64/Intel、Linux X11/Wayland、Windows installed、升级回滚、
   模型矩阵、D22 buffer 异常维护、完整 UI UX、冻结候选 12h active soak 和 D00–D31 保持开放。
6. 未重建/交付新的用户包；旧 app115 主页不是当前代码验收。最终进程检查再确认是否仍运行，
   不自动重开或复活旧授权。真人模型验证需用户手动认证，不代点 OAuth、不导出账号凭据。

## 最终收尾记录

- 最终 App probe SHA256
  `a452a41d8f534ee43fc817dc68e4be1ff4cf3e85f3ce938a103959bc8c483f5b`。
  `app-upload-ui-task-20261012-02` **8/8**，cancel 场景现在实际走 exact Host cancel API，
  receiver **3 POSTs**、zero replay，7 原 observed owners 退出，exit0。
  同 SHA256 `app-upload-ui-download-close-20261012-02` **2/2**，包括原五秒失败/显式重试，
  10 owners 退出；`app-upload-ui-adapter-20261012-02` **8/8**，7 owners 退出；两者 exit0。
  Host/file choice 仍 synthetic，realNativeFilePicker/ACP/MCP/model=false，不升级分类。
- 当前源 Core all-targets strict、App default lib/tests strict、App probe lib/tests/bin strict、
  cu-probe crate all-targets strict **四组通过**；各 log 为 `app-upload-ui-clippy-*`。
  `cargo fmt --all -- --check`、typecheck、定向 ESLint、diff check 通过；441 个 scoped
  changed/new 源文件 LF 检查通过。质量 gate final PASS，App+Workbench13573、≥1000 文件80/80。
- 浏览器验收原 browser/server 关闭后，runner 已清其 exact `site`/`browser-runtime`，
  逻辑 **372737134 bytes**，report/screenshots/hash/inventory 保留；该阶段未独立测量盘空闲增量。
- `src-tauri/target/codex-test-runtime/cleanup-upload-ui-20261012-01.json`：原 owners 终态和
  exact paths/所有祖先 non-reparse 复验后，仅清 **6 个本批被替代/重复 EXE**，无递归删除。
  logical **547952128 bytes**；H 实际空闲观察增 **547958784 bytes**（约522.6 MiB）。
  保留最终 task02/manifested unit02 两候选并复验 hash；source/WIP/seed/deps/all homes/
  首败及全部日志/截图不删。旧拒绝/uncertain-owner 产物仍 deferred，未换工具重试。
  EXE 可按原 Cargo build + fresh RunName runner / libtest manifest 后链重建，清单有再生步骤。
- 最终确认本批 build/test/native owners 均已终态；cu-probe 最后 strict 只保留下一批所需
  当前依赖缓存，追加无已验证 obsolete target，未再删整个 cache。
  旧预览 PID62200 已不在进程列表，本批没有再开用户预览或替换登录 home；无重复测试 App 留下。

下一批直接实施上述原 MCP/typed tool 链与真实选择验收，不用本批分层通过冒充完整用户链。
