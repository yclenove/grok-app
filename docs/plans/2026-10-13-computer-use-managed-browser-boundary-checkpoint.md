# CU 受管浏览器系统边界与维护来源检查点

2026-10-13；`feat/computer-use-implementation`；active / partial。
本批未提交、push、PR、发布或替换用户预览；完整交付目标不关闭。

## 用户确认与实际实现

用户明确允许：**CU 受管浏览器要求 macOS 14+，App 本身保留 macOS 11+**。

- 新 `src-tauri/computer-use-core/src/managed_browser_platform.rs` 在受管浏览器的 `product_runtime_paths()` 边界读取原生 `kern.osproductversion`。使用有界只读 sysctl，不调用 shell、不接受环境变量/模型版本覆盖、不启动浏览器或请求权限。
- macOS 11–13 明确拒绝受管浏览器；版本读取失败或格式不明确也拒绝。Windows/Linux 不继承此限制。MCP/Node bootstrap、repair、App WebView、已有浏览器扩展和桌面入口未被全局设成 14+。
- App release `minimumSystemVersion: "11.0"` 保持原样；新增回归约束。该约束不冒充旧系统上的 App 实机兼容验证。
- 两种受管错误有独立稳定分类，任务卡/面板使用十五语言说明；不是“重装/修复”或泛化权限错误。不自动重试、修复、批准任务，不凭本地回执显示执行授权。
- 宽回归发现已注册的 `computer_upload` 遗漏前端工具常量；补齐 TS/Rust/MCP/golden 清单一致性，并保留断言。没有新增权限或放宽文件读取。

**尚未在 ARM 或原生 Intel Mac 上运行 sysctl、低版本拒绝或 14+ 正向启动。Mac native = not_run。** Windows/Linux 上的版本正反例不能算 Mac 实机通过。当前 app144 尚不包含本批修改。

## 当前候选验证

| 门禁 | 实际结果 / 范围 |
| --- | --- |
| Windows 当前完整 Core | 原 session99307 exit0；980 passed / 0 failed / 0 ignored，195.02s |
| Windows Core all-targets strict / CLI | 原 session92964 exit0；Clippy `-D warnings`，CLI 3 passed |
| Jammy 当前完整 Core | 原 session20394 exit0；glibc2.35 / Rust1.98，980 passed / 0 failed / 0 ignored，245.19s |
| Jammy strict / CLI / 当前 source import | 同一冻结 `/work/core-runtime-bounds-05`；all-targets strict、CLI3、Node20.18/PW1.48 import passed；原输入前后 SHA 不变 |
| CU 前端全部目录 + i18n | 原 session79425 exit0；34 files / 477 passed，无 skipped |
| Typecheck / scoped ESLint | 原 session63165 exit0；另补查工具清单与 Node wrapper lint exit0 |
| Node wrapper/target/before-build | 14 passed / 0 skipped；source audit 不替代 bundle prepare/check |
| Node MCP golden | 11 passed / 0 skipped，上传 schema/Host-only 边界保留 |
| Python 来源 / DataPack | 22 + 16 passed；源检查器不启动新增二进制、不渲染 HTML |
| fmt / diff / FINAL quality | passed；App+Workbench13573，≥1000 源文件80未增长。FINAL: PASS 只指代码质量 |

原宽前端回归为476 passed / 1 failed（TS缺上传清单），不是首次就绿；修复后477全通过。
原 WSL 内联 launcher 因参数转义在 mkdir 阶段失败，未生成 native candidate；随后使用直接 argv 的原 session20394 新冻结候选，不改旧快照/锁来刷绿。旧 Jammy03 九项失败仍保留。

证据：`.run/runtime-platform-jammy-20261013-05/jammy-core-archive.log`、`core-inputs.sha256`；
`.run/runtime-sources-20261012-01/platform-source-and-seed-hashes-20261013.json` 与 `code-quality-platform-20261013.json`。
以上 `.run/` 均在 `tools/computer-use-probe/` 下，原 tool handles/首败输出保留。

## 上一批 Node22 / PW1.64 来源核验已收尾

永久 `cu-prepare-runtime --verify-sources` 共用 production Node extractor、PW materializer、原生 import；不会生成/发布 browser、worker、seed 或 manifest，也不授予任务权限。Node wrapper 新增严格三种互斥模式、未知/重复参数拒绝与四 target triple 归一化。

- Windows 原冻结来源候选：Node22.23.3 / PW1.64，实际 import passed、`runtimePrepared:false`。
- Jammy 原 session41973 已 exit0：`/work/core-maintained-sources-01` 实际 build44.11s、bin strict16.66s、同版本 native import passed；冻结来源/cache SHA 不变。helper SHA `46b51031773e02753cdbcae8408ad0cda99b1546c6575134d2b287df0f18c533`。
- 两 Mac source materialize 成功，但 `importProbe:not_run_cross_target`。不能记成 Mac native import。
- PW1.64 materialized tree115件、13655528 bytes、SHA `47db3fc7767d6e50666eba83142e5aa782394125516a9392904c3d8bc6516f20`。
- 这组 **source-pair 候选不是本批当前 Core 冻结候选，也不是完整运行时 pack**。不能拼接两组证据宣称维护版已交付。
- 原“445 seed 文件全变了”诊断是假阳性：PowerShell JSON 的 DateTime 自动转换/小数秒字面格式导致字符串比较失败。独立 UTC ticks/长度/SHA 清点无变化；未恢复或编辑 seed。原错误保留。本批再验445文件，0变化/0新增。

维护 source receipts：`.run/runtime-sources-20261012-01/maintained-sources-*.log`；
`.run/maintained-sources-jammy-20261012-01/native-maintained-sources.log`。

## 官方未品牌化 Chromium：实查发现完整通知缺失

四个官方 GCS 快照固定同一 position1716663 / commit `f52796fc20c6e7d618374bb8bf096e7b2155e14c`，源码版本157.0.8101.0；不是各平台 latest 拼接。GCS generation/长度/MD5、完整 ZIP SHA/CRC/树、四实际架构及 Mac executable/framework metadata 已验。它是开发快照，稳定性未认证，未执行或晋升。

| 源平台 | 全树文件 / bytes | 总条目 / 最长路径 |
| --- | --- | --- |
| Win_x64 | 258 / 859583753 | 258 / 53 |
| Linux_x64 | 478 / 709988858 | 483 / 78 |
| Mac_Arm | 352 / 382083296 | 683 / 193 |
| Mac Intel | 352 / 420425816 | 683 / 193 |

Mac均331目录、347普通文件、5个 Framework links；plist/exe/framework minimum13.0仅为metadata，产品支持仍按用户确认14+。所有ZIP均无独立 LICENSE/CREDITS/NOTICE 候选。

独立有界 DataPack v4/v5 检查器验证索引/sentinel/别名/ID/offset/总大小；gzip 校验 trailer、拒绝拼接/截断；按固定上游格式解析 Chromium Brotli 的2-byte magic +6-byte长度，限定逐块输出与总解压字节。较旧无 bounded API 的 Brotli 不回退 unbounded decompress。工具依赖是 probe-only `Brotli==1.2.0`，不进 App runtime。

**每个平台实读231个 `.pak`，均仅发现资源41071：Brotli204 bytes → 同一401-byte sample Credits，copyright/license mentions=0。** SHA `c2a338c4f1f5f5e19d3020a25f8dc47e394ba4fed1ec2d8dc5b2a915170e02bc`。门禁为 `failed_missing_full_notices`，不是合法/非法法律结论，更不是可以套旧版 CREDITS 发布。

完整 per-pack SHA/表/候选及四原样HTML保存在 `.run/runtime-sources-20261012-01/credits-03/`。原gzip-only无候选、Brotli初测失败和 Python3.8 evidence API失败均保留；修后实查清单正确，**没有把实际 notices red 改成 green**。

读取的固定 commit 上游依据：

- [根 LICENSE](https://chromium.googlesource.com/chromium/src/+/f52796fc20c6e7d618374bb8bf096e7b2155e14c/LICENSE)：允许满足条件的源/二进制再分发；并不自动覆盖所有第三方组件。
- [credits 构建开关](https://chromium.googlesource.com/chromium/src/+/f52796fc20c6e7d618374bb8bf096e7b2155e14c/components/resources/BUILD.gn)：`generate_about_credits=is_official_build`，关闭时使用 sample 文件。
- [sample](https://chromium.googlesource.com/chromium/src/+/f52796fc20c6e7d618374bb8bf096e7b2155e14c/components/resources/sample_credits.html)、[grdp](https://chromium.googlesource.com/chromium/src/+/f52796fc20c6e7d618374bb8bf096e7b2155e14c/components/resources/about_ui_resources.grdp)：sample建议开启 generate_about_credits；实际资源使用 Brotli。
- [DataPack 格式](https://chromium.googlesource.com/chromium/src/+/f52796fc20c6e7d618374bb8bf096e7b2155e14c/tools/grit/grit/format/data_pack.py)、[Brotli header](https://chromium.googlesource.com/chromium/src/+/f52796fc20c6e7d618374bb8bf096e7b2155e14c/ui/base/resource/resource_bundle.h)。检查器独立实现，不复制上游实现或省略其归属说明。

## 预览、清理与下一步

用户唯一已交付预览仍是 app144/PID59356、创建UTC2026-10-12T15:43:07.9761177Z、原隔离home/登录/历史/909字未发送草稿保留。上一批已亲眼看过 `tauri.localhost` 首页/关于版本；本批仅查原进程仍活，不假称重新做了UI验收。14895/14898 fixture已停止，旧草稿暂不能用于完整任务验收；没有代发/代批/选择文件。

原 Windows build/test 和原 Jammy session 均终态后，只退休两个已核实旧 incremental generations，保留同包的新代次：逻辑603271339 bytes（约575.3MiB），H即时空闲观察增加484687872 bytes（约462.2MiB）。不是将逻辑大小作为物理回收。精确路径/身份/再生命令在 `platform-cache-retirement-20261013.json`。其它归属/继任不明 cache暂缓；源包、旧失败、全部候选/SDK/工具链、immutable seeds/current build/依赖和用户home均保留，未停止其它进程。

当前 compiled 仍 Node20.18/PW1.48/Chromium130；维护源未进入 App144 或正式 pack。下一步按[维护版落地顺序](2026-10-13-computer-use-maintained-runtime-next.md)推进完整 NOTICE/来源、限定配额与四锁、原生四架构、生产 prepare/check/repair/rollback/浏览器行为及包。Windows真人原ACP/MCP/model/picker、三平台/两Mac架构/四surface/UI UX/签名安装更新卸载/模型矩阵/同最终候选12h/D00–D31仍全部开放；**整体不是交付完成**。
