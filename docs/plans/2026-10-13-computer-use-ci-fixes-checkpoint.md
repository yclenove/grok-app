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
