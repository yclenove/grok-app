# Computer Use：远端 CI 修复与同步

2026-10-13，`feat/computer-use-implementation`。

再次 fetch fork 与源仓库后，`origin/main` / `upstream/main` 均为当前 HEAD
祖先，没有新的 main 提交需要合入。前批同步仍有效，不覆盖用户运行中的
App144、隔离 home、登录、历史或未发送草稿。本批仅提交到 fork，不创建 PR、
tag 或 Release。

## 远端失败及修复

原 [CI run 38372012718](https://github.com/yclenove/grok-app/actions/runs/38372012718)
已结束，整体 failure，frontend 和两种架构的原生 SDK job 成功。

- Rust Windows/macOS/Linux：Rust1.99 新 lint 拒绝 `bounded_store_gates`
  六处 `map_err(&fail)`。改为 `map_err(fail)`，closure 捕获引用且可 Copy；
  失败清理、有界存储和隐私检查不变，不降低工具链或豁免 warning。
- production Jammy：`tempfile.mkdtemp` 的随机后缀可含下划线，违反 checker
  明确的 owner 目录命名合同，`unchanged-inputs` 在断言前 exit2。夹具改用
  UUID hex 后缀并以 mode0700 原子创建，不放宽产品路径校验。
- Rust Linux-arm64：受管浏览器 target/资源缺失，仍未修复，不跳过运行时
  prepare 或用 x64/EOL 浏览器伪装成功。

## 本批实际验证

- Windows 现有隐私门禁两项：2 passed / 0 failed / 0 ignored。
- Windows Core all-targets strict clippy 和 workspace fmt：exit0；本地为
  Rust1.98，Rust1.99 的全平台结论须由新 CI run 确认。
- Debian WSL 的现有输入夹具：6场景通过，包含正常输入、唯一生成 owner
  变动、源码改变、新增文件、原 seed 改变、源文件缺失；每个 bash owner
  已 joined，精确生成夹具已自行删除，逻辑612 bytes，物理回收量未知。
- 双架构原生 SDK 的实际远端证据见
  [SDK检查点](2026-10-13-computer-use-linux-sdk-targets-checkpoint.md)，不是本批重跑。

仍继续维护运行时、ARM64 browser、完整产品/安装/模型/四surface/UI UX及
同最终候选12h门禁；本批绿灯不代表 CU 已交付。

## 后续实际 CI 与第二批修复

`fb4309bf` 已推到 fork，新
[run 38374373979](https://github.com/yclenove/grok-app/actions/runs/38374373979)
中的 frontend 与双架构 SDK 再次成功；Jammy 的原输入夹具已成功，正式
runtime prepare/check 也已成功，完整 release App 检查仍运行中。
没有为观察超时取消它，或将 live job 记为成功。

这轮不再报原六处 closure borrow；继续暴露以下真实阻塞，已修复源码：

- Rust1.99 弃用 `Atomic::fetch_update`。八处改为相同语义的 `try_update`，
  checked overflow、比较/存储 memory ordering 和返回值不变；包括 X11/
  WebView epoch 与测试的故障计数。本机 Rust1.98 也实编通过该 API，未加
  warning 豁免或抬最低工具链。
- macOS 的 `libc::S_ISVTX` 是 u16，`MetadataExt::mode` 为 u32，原代码不能
  编译。使用固定 POSIX sticky bit `0o1000`，保留目录 owner/写权限/链接门禁。
- 普通 Linux Rust job 缺私有资源 manifest；改为在 App 编译前构建并审计
  维护 SDK、按 rustc host target stage 资源。x64 runner 固定 Jammy，与
  原 ABI2.35 合同一致；ARM64 同样 stage，不跳过仍缺失的浏览器门禁。

本批最终 Windows Rust1.99 App all-targets strict clippy、workspace fmt均
exit0；真实 manifested App harness **2399 passed / 0 failed / 87 ignored**。
87项 ignored 未算原生验收。Core 现有 surface/lifecycle16项及隐私2项全部
通过。Windows全量执行后新增的 sticky-bit修正仅在 Unix cfg内，最终 Windows
strict已再次通过；Mac/X11原生结果仍待新 CI。

来源/诊断工具62项、Node target/command15项、定向ESLint、源格式与完整
quality门禁通过。两份 YAML校验脚本实例化 shell后通过 bash语法检查，
确认双架构 SDK准备和 ARM64诊断只上传JSON/log/TAP。ARM64来源及后续真实
执行见[维护浏览器检查点](2026-10-13-computer-use-arm64-browser-checkpoint.md)。

证据：`tools/computer-use-probe/.run/ci-followup-20261013-01/`；保留两轮
远端错误日志及最初 Windows PowerShell5 在 native stderr重定向时提前中断
的本机编译输出，后续 pwsh7按真实Cargo artifact完成，未删除首败。

原测试 owner已退出；验证绝对路径/所有父级无reparse、测试copy/source SHA
和原日志后，只删除 `ci-followup-20261013-01/app-tests.exe`。
逻辑删除 **74,039,808 bytes**，H盘可用空间即时观测增加 **74,043,392 bytes**；
无法隔离并发磁盘活动，因此不将观测差值当作精确物理回收量。
原Cargo测试binary、源码、编译依赖、日志/manifest/hash与当前ARM64诊断均保留；
再生命令与暂缓项目记录在 `cleanup-receipt.json`，用户App144没有退出或替换。
