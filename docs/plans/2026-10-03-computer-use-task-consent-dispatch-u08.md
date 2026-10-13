# Grok U08：当前候选门禁、协议与最终授权体验复审

状态：**draft / not completed**。Codex已接手；U01–U07结果/接口确认后继续本批。
单Codex writer修复并验证，不启动Grok/子代理；本稿不授权提交或正式发布，完整范围保留。

## 1. 候选与证据覆盖

确认HEAD/dirty inventory，包含全部新增产品、协议、locale和测试文件；记录候选指纹、
实际CLI/runtime/worker/extension版本。编译之前固定输入与命令；变更后重验受影响项。
逐U00–U07与S01–S35核实现/验证/evidenceLevel，未验/人工依赖不能因green CI变passed。
原D00–D31、全平台native/installed/model/soak/签名需求保留，本批成功不等于完整CU完成。

检查测试本身：合法请求、实际后置条件、真实先红、有界队列/资源/Stop与replacement。
constant true/零positive effects/非terminal EOF/缺事件/未知退出都不能报绿；保留首败。
准备状态不能提前Approved；全局bounds要可安全回收，不得实现成一生只能32个chat；
未消费有效票据不靠drop掉换有界。请求通道、任务权限、执行绑定/观察分别核合同。

## 2. 当前门禁命令，不混Cargo默认member与全平台

先全文读CONTRIBUTING与当前`.github/workflows/ci.yml`相关jobs，按真实features/target
执行。以下是2026-10-03读到的起点，不代替开工时核当前CI：

```text
git diff --check
pnpm deps:check
pnpm audit:prod
pnpm typecheck
pnpm lint
pnpm test
pnpm build:ui
pnpm check:computer-use
python scripts/check-code-quality-gates.py --mode final
python scripts/publish-website-downloads.py --self-test
node --test scripts/stage-windows-portable.test.mjs
node --test tools/computer-use-extension/*.test.mjs
node --test tools/computer-use-mcp/*.test.mjs
node --test tools/computer-use-browser/*.test.mjs
node --test tools/computer-use-probe/existing-mcp-live.test.mjs tools/computer-use-probe/webview-dom.test.mjs
```

src-tauri下：

```text
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo clippy --locked -p grok-computer-use-core --all-targets --all-features -- -D warnings
cargo test --locked -p grok-computer-use-core --all-features
cargo test --locked -p grok-computer-use-core --features test-support --test driver -- --test-threads=1
```

PowerShell/Node各命令如有glob兼容问题先列实际文件并传数组，不让0文件测试误通过。
Cargo workspace default-members是App，root cargo test并不覆盖全部helper；core/native/
App default与preview按真实cfg分开跑。重负载构建串行，已绿且候选未变不重复刷全套。

Windows App harness用当前CI同款cargo --no-run --message-format=json得到精确artifact，
mt.exe嵌windows-test-manifest.xml后直接运行同副本，记录前后hash；不得glob旧exe或
Cargo重链接抹掉manifest。不要复制CI的cargo clean到live工作区清理已有成果；需要
clean重建用owned snapshot/独立target。0xc0000139/0tests/ignored/filtered不算整套通过。
PATH处理只限该命令的进程环境，不能修改机器PATH/依赖；无须manifest的core不硬加。

新增任务授权oracle/protocol/golden/runner须接当前CI对应job，严格Rust/Node schema
一致、unknown fields与伪造身份拒绝；无白名单豁免产品权限测试。
Linux SDK/Wayland/X11、macOS双架构以及updater native tests按原D任务要求保留；
缺环境不伪装pass，不为了本批将CI改高系统底线或禁用required能力。

## 3. 格式、代码规范和债务

遵守AGENTS的pnpm-only/15locale/project Select-dialog-styles/settingsCatalog。
App.tsx+AppWorkbench combined只能不增长，不添加state/大feature块；新状态归领域模块。
源码LF、fmt与diff check；不要为了统一换行改无关文件，保留用户原dirty/untracked。
大文件按职责拆，不用forward-only拆分、allow dead_code、ignored安全测试、ts-nocheck/
any props或更改quality ceilings报绿。质量脚本按当前值验，不能抬13750/94/65之类上限。

查实际临时实现/TODO/placeholder/unused按钮/cfg探针生产旁路、absolute H盘依赖、
全局always-approve、多个权限真相、unbounded queue/thread与背景采集。
source test seam可留在明确test-support/cfg(test)，不能进入production工具面。
runtime/seed worker合同当前且immutable副本不被写；目标/所有者/取消字段不能被Host补默认。
mandatory债务修完并回归，不把“以后再改”写成当前功能完成。

## 4. 用户文档与准确状态

computer-use wiki里旧“先设置启用→面板选目标授权→MCP才注入”的描述按最终事实更新，
解释按需请求、首次启用一次决定、任务范围/增量、一次Continue、Stop与必要系统Share。
主用户说明简洁，技术trace/安装诊断另处；不能把平台实验描述成stock公共支持。
历史checkpoint不覆盖原文，必要更正追加链接；当前权威状态一份，不创建互相矛盾清单。
15locale完整自然短文案/placeholder同键，Intl格式；所有新增setting登记搜索/深链/锚点。
真实UI深浅/窄宽/长语言、keyboard/stale/Stop/preview与实际App证据按U03/U07复验。

## 5. 交审

交`2026-10-03-computer-use-task-consent-u08-checkpoint.md`、当前候选manifest与逐项矩阵：
commands/真实exit/count/ignored/filtered、source/runtime/oracle/test/binary hashes、首次
失败及修复、owned cleanup/terminal/join、平台/真人未验项、产品与mandatory债务结论。
依赖安全合规等既有合同继续执行，不加无关产品授权流程来迎合测试。
CI green仅E1/相应runtime证据，不代替U07人工/原生；如有required外部缺口结论partial。
Codex接收Windows候选门禁后派U09独立开发包；之后仍继续原D00–D31。

不commit/push/PR/merge/tag/release/reset/restore/clean/stash/rebase；不触凭据、共享~/.grok、
代理/VPN/用户profile/其它worktree与日常进程，不自动真实系统/App/CU/Share批准。
