# CU 维护版运行时：Windows / Jammy 内部原生诊断

2026-10-13；`feat/computer-use-implementation`；active / partial。
用户再次确认受管浏览器 macOS 14+、App 本身保留 macOS 11+。既有系统边界接线保持，Mac 两架构原生验收仍 `not_run`。
本批没有修改产品 compiled pins、四份 source locks、pack-targets 或 seed，没有更新用户 App144，也没有 commit/push/PR/release。

## 同来源候选与实际边界

这是独立 **INTERNAL compatibility diagnostic**，不是 runtime prepare/check、最终发行候选或完整 App 验收。
使用官方未品牌化开发 Chromium157，不是上一批审查的 ungoogled155。

- Node **22.23.3**、Playwright **1.64.0**、Chromium **157.0.8101.0**。
- Chromium commit `f52796fc20c6e7d618374bb8bf096e7b2155e14c`，position `1716663`。
- 私有 Node 执行实际版本查询、加载私有 PW package path，启动实际浏览器读取 version、独立 HTML 后置条件并 await close；报告 `promoted:false`。
- staging 工具先核验固定来源，再有界读取写入全新 `.run` 子目录；不会 extractall、覆盖既有运行目录、调用系统浏览器或读取用户账号。
- 两个 stage 冻结相同 worker/fixture/test **63 件**，SHA 与当时源码一致，旧 node_modules/cache 不进入冻结。
- PW 树115件 / 13655528 bytes，SHA `47db3fc7767d6e50666eba83142e5aa782394125516a9392904c3d8bc6516f20`。
- Windows browser 树258件 / 859583753 bytes，SHA `5f49d409b215d62e32529f1b8462f879591913310b5d2f5e73db6c75aa51c8df`。
- Linux browser 树478件 / 709988858 bytes，SHA `e57057a6769171e9eb9609b737890c86cf8b11a0e31c47374dce7f6f70fe374c`。

每个 stage 的完整输入身份保留在 `tools/computer-use-probe/.run/maintained-native-20261013-{windows,linux}-01/diagnostic-inputs.json`。
四份官方157包的完整第三方通知仍失败（只有401-byte sample Credits），不能因本次 native 通过允许发行；产品仍为旧 Node20/PW1.48/Chromium130。
固定源 license generator 的输入仍待核验；仅拼 LICENSE 或使用另版/另一平台 Credits 不能复现真实通知。本批没有下载完整 Chromium/DEPS 或运行通知 generator。
后续[固定源码核验](2026-10-13-computer-use-pinned-license-source-checkpoint.md)纠正了早先过强的 GN 前置要求：正常浏览器 Credits 默认扫描 checkout，GN output/target只用于可选精确依赖模式。完整同源 metadata/DEPS/depot_tools与显式coverage门禁仍必需。

## Windows 实际结果

原版本 probe exit0，实际 Node/PW/browser 身份、独立 HTML 和 `browserClosed:true` 通过。
原测试 session69602 exit0：**28 passed / 0 failed / 0 skipped / 0 cancelled**，64673.9748 ms。
五份冻结测试为 `typed-act-http`、`observation-http`、`observation-mutation-http`、`worker-error-http`、`cleanup-http`。
涉及中文 typed actions、截图/ref 退休、上传、scroll/element 与 pixel drag、wait/取消/导航 unknown、pause/resume fence、不同 profile 实际 PID，以及关闭/停止/shutdown 原 context 的迟到收尾。

原 TAP 在工具输出流中，**未保存为本地 raw TAP 文件**；只补了明确注明手工记录的 `native-worker-tool-stream-summary.json`。不冒充原始日志或重跑所得证据。
Windows 本批最后 scoped CIM 查询原 stage 下0 live process。Node `EnvHttpProxyAgent` warning 不等于断言失败；没有修改代理，worker child-env 仍剥离代理与 API keys。

## Jammy 首次失败与新 generation 重试

私有 Ubuntu22 root：`/var/tmp/grok-cu-jammy-sdk-20261012-01`，实际 glibc2.35。

1. 原 session83625 exit1，candidate `/work/maintained-native-157-01`。浏览器 loader 缺 `libnspr4.so`，原 browser PID18 exit127；**28 测试未进入**。失败 identity、原 runner、inputs SHA 和 `jammy-evidence/native-worker.log` 保留，不覆写成通过。
2. 原 owners 已终态后，只在该任务私有 root 安装运行依赖。完整 `ldd` 表明缺 NSPR、NSS、NSSUTIL、SMIME、ALSA 五库。APT 模拟/实际均为0升级、6新增、0删除，准确版本和 before/after loader 保存在 `jammy-dependencies-01/install.log`。
3. 新 runner 严格校验两位 generation、固定 source/evidence/root 与目录身份，拒绝00、遍历、错配及既有输出。新 candidate `/work/maintained-native-157-02`、新 evidence `jammy-evidence-02`，不重跑01或覆盖 wx identity。
4. 原 session53413 **exit0**，实际 Node22/PW1.64/Chrome157 与 await close 通过；同五测试 **28 passed / 0 failed / 0 skipped / 0 cancelled**，55944.596583 ms。原 namespace/runner 收束，staged input SHA 复验通过。

APT 固定安装：libnspr4 `2:4.35-0ubuntu0.22.04.1`；libnss3 `2:3.98-0ubuntu0.22.04.4`；libasound2/data `1.2.6.1-1ubuntu1.2`；alsa-topology-conf `1.2.5.1-2`；alsa-ucm-conf `1.2.6.3-1ubuntu1.12`。
APT 有 `/dev/pts` 日志终端和 `_apt` 下载 sandbox warning，实际 dpkg 六包均 `ii`，最终 `ldd` 没有 missing DSO；保留原警告，不称警告已修。宿主 WSL/Windows 包、代理和服务未更改。

这仍是 **root/headless 内部诊断**。不证明用户级 Chromium sandbox、X11/原生 Wayland、portal、普通 App、RPM/deb/AppImage 安装、真实下载/真人文件选择、模型/ACP/MCP 或最终12h通过。

## 真实依赖修复，而不是只补测试机

`src-tauri/tauri.conf.json`：deb 保留 WebKit/GTK，追加 `libnspr4`、`libnss3`、`libasound2 | libasound2t64`；后者覆盖 Jammy 与 t64 命名系统，不硬锁测试 root 的 patch version。
RPM 使用 Tauri 正式字段 `depends`，不是 `requires`：声明五个 `SONAME()(64bit)` capability，不硬编码发行版包名。

实际核对安装的 CLI2.11.4 对应 bundler2.9.4 源码：CLI 将自定义 depends 与 GTK/WebKit/tray 依赖追加合并；RPM writer 将每项传给 `Dependency::any`。这不是递归扫描 browser resource 自动推导，因此要显式声明。
Fedora43 公共 provider metadata 的实际 Provides 已确认：nspr（source nss）、nss、nss-util、alsa-lib。原 `/pkgs/nspr/nspr/...` 返回历史 overview，保留但不作为 provider 证据；正确 `/pkgs/nss/nspr/...` 已另存。
七份上游源码/metadata 的 URL、实际长度/hash 与能力映射在 `packaging-source-01/source-inventory.json`，原内容未执行。**生成包 metadata 与真实安装仍未验**；其它 DSO/动态组件/资源最终随包核对继续开放。

## 本批回归与质量

- staging boundary Python：9 passed / 0 skipped。
- target/package Node：9 passed / 0 failed / 0 skipped；新增依赖及平台资源不覆盖合同。
- Jammy pre-execution guards：14 passed / 0 skipped；非法 generation/路径、已有首败/成功 evidence、重复 install 均 exit2，零 staging/安装副作用。
- Bash syntax、Node syntax、定向 ESLint、`git diff --check` 和 FINAL 质量门禁通过。首次 ESLint 报缺 Node globals 与既有 regex-spaces；改显式 Node imports/等价 regex 后真实 exit0，未关闭规则。
- 未修改 Rust/前端业务逻辑，不将前批980 Core/477前端重复记作本批新通过。FINAL 只指代码质量；App+Workbench13573、≥1000源码80未增长。

## 产物与用户预览

原 owners 终态、目录/所有父级无 links/reparse、完整树SHA/文件数/字节与固定源一致后，仅退休两份 obsolete browser subtree，不递归删 candidate/root：

- Windows `maintained-native-20261013-windows-01/browser`：258件，逻辑859583753 bytes；H 即时可用观察增加859779072 bytes（约820MiB）。
- Jammy 首败 `.../work/maintained-native-157-01/browser`：478件，逻辑709988858 bytes；WSL可用观察增加710922240 bytes（约678MiB），**Windows VHDX物理回收未知**。

总逻辑1569572611 bytes（约1.46GiB）；观察空闲变化不是独占物理归因。精确 removals journal、cleanup receipt 和再生说明分别保存在 Windows stage 与 Linux 首败 evidence。只移除执行副本；固定 source ZIP、Node/PW、冻结 worker、原失败/成功证据保留。
保留当前 Linux02 和 H Linux staging 供后续诊断；当前包/seed/SDK/依赖与不确定归属 cache 不删。没有停止任何无关进程。

本批 CIM 再次核对唯一用户 App144 的 PID59356、实际 EXE 路径及创建时间（UTC2026-10-12T15:43:07.976117Z）匹配；没有操作 GUI、代发/批准任务、选择用户文件、更新或关闭预览。原隔离 home/login/历史/未发送草稿保持。

## 后续不缩水

按 [R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)继续。官方157完整通知与来源覆盖、安全/稳定更新策略仍红；第三方155还有真实安全/codec差异与两Mac同版本缺口。内部兼容通过不跨过 R1 或替代四目标最终候选。
最终来源选定后仍须生产归档/配额/compiled pins/四锁/prepare-check-repair-rollback、Mac ARM与原生Intel、Windows/Linux用户级原生动作及安装签名/更新卸载、所有四surface/UI UX/真实模型ACP-MCP/picker，最后同一最终候选12h与D00–D31。完整目标保持 active / partial。
