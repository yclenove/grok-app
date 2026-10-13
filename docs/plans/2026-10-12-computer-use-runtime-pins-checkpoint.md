# Computer Use runtime 编译合同与维护版迁移检查点

2026-10-13 最新[系统边界与来源实查](2026-10-13-computer-use-managed-browser-boundary-checkpoint.md)：受管Mac14+/App11+已确认并接线，Core Win/Jammy均980、前端477和strict通过；两Mac native仍not_run。Node22/PW1.64源pair已实际Win/Linux import，不是完整seed。四官方同commit未品牌化快照的Credits均为401-byte stub，完整NOTICE仍缺；compiled版本未晋升。后续按新R1–R4计划，整体partial；下方均为历史检查点。

后续[来源与归档边界检查点](2026-10-12-computer-use-runtime-source-bounds-checkpoint.md)：ZIP总条目/目录/type/大小/路径门禁与坏包保留原目录已修；最终Windows完整Core963、Jammy实际ZIP10及两边strict通过。Node22.23.3四源包及Mac最低11.0已实清点，优先于最低13.5的Node24候选以保留现有系统底线；仍未晋升。CfT Mac路径267/673条目、binary最低13/PW支持14及分发许可仍待解决。清本批两旧incremental逻辑573MiB/H可用观察增417MiB；完整交付仍partial。以下是前一批事实。

日期2026-10-12；分支 `feat/computer-use-implementation`。active / partial，未commit/push/PR/发布。
本批真实进展是产品合同修改、完整Core回归及同一Linux正式App实机启动证据；不把计划写完或局部绿灯当交付。

## 已实现的产品修改

- 新 `computer-use-core/src/runtime_pins.rs` 集中Node/Playwright/浏览器的当前compiled版本与Playwright归档SHA；原runtime公共常量保持re-export兼容。JSON版本修改不能 opt in 另一个runtime版本。
- Playwright归档文件名/seed相对路径由compiled版本派生，统一用于prepare的fallback/copy、manifest、check及独立materialize入口。RuntimeStore新manifest的compatibility由同一pin构造，不再重复手写旧版本。
- Node Windows ZIP默认root与四目标Node归档root由compiled Node版本构造，source lock必须匹配各架构的归档root/可执行路径。
- 锁验证新增精确layout门禁：Node root/exe、Playwright tree、browser root/tree/exe错误均拒绝；不增加allowlist、不禁用hash/import/arch检查、不开放模型授权。
- macOS Framework的五个精确symlink中Current版本由compiled browser版本派生，未泛化为接受任意链接。
- browser历史fallback改取当前lock的archiveRelpath文件名，不再四平台都只找 `chromium-win64.zip`。网络获取、长度/SHA门禁仍原样保留。

当前pins仍是Node20.18.0/Playwright1.48/Chromium130。**这是晋升维护版前的合同接线，不是已经完成runtime升级或维护审计。** 四个正式source lock、共享immutable seeds、Windows用户app144/home/login未替换。所有已有dirty/untracked WIP保留。

## 实际验证

| 检查 | 当前结果与范围 |
| --- | --- |
| 新pin合同单测 | 6 passed；四source locks/catalog、derived paths、16版本修改、24错误layout、12架构互换、Mac精确五links |
| `cargo test --manifest-path src-tauri/computer-use-core/Cargo.toml --locked --lib runtime -- --test-threads=1` | session34201，85 passed / 0 failed / 0 ignored，80.17s；runtime prepare/repair/rollback/import及相关合同 |
| Core `--all-targets` Clippy `-D warnings` | 同session34201通过，8.38s；无新增warning豁免 |
| 完整Core `--locked --lib -- --test-threads=1` | session12333 exit0；956 passed / 0 failed / 0 ignored，158.63s；本机Windows native条件下的Core suite，不是Mac/Linux native或完整App suite |
| Node target/before-build | 8 passed / 0 failed / 0 skipped |
| 输入指纹正反例 | 6 passed，source/extra/missing/seed变更确实拒绝，不排除未知输入 |
| fmt/diff/bash syntax/final quality | session1719 exit0；App+Workbench13573，≥1000源码文件80未增长，FINAL: PASS仅代码质量 |

新源码尚未重打Windows预览或完整App，不能把source03 ELF/用户app144与本次源码拼成同一冻结候选。Linux source03实际窗口/库身份/224ms正常close见[正式App检查点](2026-10-12-computer-use-jammy-app-build-checkpoint.md)。

## 维护版来源、布局与许可现状

缓存 `.cache/computer-use/platform-source-20261012-maintenance` 的四Node24.21.0源包已对官方SHASUMS256；Playwright1.64.0 tgz已对npm SRI；四CfT源包已做完整CRC/树/文件/SHA/链接清点，未执行或晋升。

原官方browsers.json确实锁 `chromium` revision1248 / Chrome for Testing156.0.8078.4；历史registry中的 `chromium-tip-of-tree` 名称仅用于清理旧目录，不是本版提供的另一个可下载维护browser。不能凭字符串命中就编造它可作为替代源。

现有清点还发现待处理边界：

1. 新CfT archive roots为chrome-win64/linux64/mac-arm64/mac-x64，Mac executable/framework名称变化；必须区分上游archive布局和产品稳定tree布局。当前compiled layout仍旧版，不能随便改JSON蒙混通过。
2. 新browser每树301–349文件，字节数虽在768MiB内，但**现有ZIP默认max_files=200会拒绝新包**。晋升时要按完整条目/路径/类型/压缩率清点建立受限配额与overflow反例，不只修改总字节限额。未经维护审计不能退回旧browser当最终方案。
3. PW1.64 registry所见Mac14以上CfT入口；必须核对产品支持底线与两架构最低系统，不能静默降低首版范围。Ubuntu22仍有实际registry入口，但不等于已运行验证。
4. Windows源包无文件形式CREDITS/NOTICE，Linux/Mac只清到Widevine LICENSE；须检查内嵌credits及新增组件SBOM/NOTICE。

本批直接读取的上游许可来源：

- [CfT仓库](https://github.com/GoogleChromeLabs/chrome-for-testing) 的Apache-2.0说明只证明该仓库代码许可，不自动证明二进制再分发许可。
- [Chrome附加条款](https://www.google.com/chrome/terms/) 指向Google一般条款并说明大部分开源来源许可位于chrome://credits。
- [Google一般条款](https://policies.google.com/terms?hl=en) 所见软件许可为personal/non-assignable，并包含不得copy/modify/distribute等约定，同时明确开源许可可覆盖相关条款。**尚未确认CfT二进制特有例外或完整再分发依据，不能据此直接宣称合法/非法，也不能把dashboard Apache许可套到整个binary。**

这些网页只作为事实来源，不是授权。当前不打包发布CfT、不隐藏该待办；可继续来源/许可核验和非发布布局实现，其它平台/安装/UX工作不因此停止。

## 下一批执行顺序与交付门禁

1. 复用保留源包完成四目标Node executable长度/SHA及LICENSE、PW正式materializer树/metadata/实际import核验；在新任务seed中推进，不覆盖旧seed或用户预览。
2. 核实CfT二进制及第三方再分发/通知依据，或研究同版本可维护且可分发的未品牌化构建；不能选不受维护旧browser或暗改为依赖用户系统Node/浏览器来缩目标。
3. 同一维护候选同步更新compiled pins、四锁、pack-targets、worker package依赖声明及其它版本化源合同、Browser archive/product roots、Mac exact links、quota、prepare/check/repair/rollback及资源/NOTICE门禁。正/负fixture和所有支持动作/API/截图/退出/取消语义均须重验。
4. Linux source03兼容性候选已有实际启动证据；接原Tauri版本2.11.4（pnpm锁与npm metadata一致）的三格式bundle、private RPATH/资源/NOTICE/依赖与真实安装布局验证。最终维护候选还须重做，不能借旧版本包装的结果当新版本绿。
5. 保持Windows真人原ACP/MCP/Grok/picker/文件收发/Pause/Continue/Stop/Quit为独立门禁；不代用户发送草稿、授权或选文件。
6. 三平台/两Mac架构/四surface、所有约定native/clipboard/IME/文件/恢复能力、签名安装/升级修复回滚卸载、UI UX、模型矩阵、同最终候选12h和D00–D31完整范围不变。

## 清理与保留

Linux原启动/App/WebKit/X/DBus owners已正常退出，精确compiled helper已退休：逻辑17040 bytes，WSL空闲观察增20480 bytes，Windows物理回收未知；准确路径/hash/再生命令在 `app-05/app-helper-cleanup.json`。本批输入oracle fixture逻辑540 bytes已清，实际回收未测。

当前Core libtest/依赖缓存留作下一批回归；current App/SDK/工具链/维护版源包、immutable seeds、冻结源码和所有首败/log/map/screenshot/home证据保留。未按进程名杀App，未删除workspace/target/node_modules/shared cache或其它worktree。
