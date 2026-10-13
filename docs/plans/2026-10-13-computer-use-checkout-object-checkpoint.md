# CU 维护浏览器：实际 checkout 与固定 GCS 对象

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[四平台 archive / GN 检查点](2026-10-13-computer-use-archive-build-checkpoint.md)。
用户确认的兼容边界保持：App macOS 11+，CU 受管浏览器 macOS 14+。
本批不提交、推送、提 PR、发布或操作 GUI；不发模型任务、不代批权限、不选择用户文件。
产品 pins、四锁、seed、App144 和用户 home 不晋升；完整 R1–R4 / D00–D31 范围不缩减。

## 1. 同次构建的实际 checkout

`collect-chromium-archive-logs.py --profile checkout` 新增固定的三个同 build 日志：
`bot_update/json.output`、`bot_update/stdout`、`bootstrapped_properties/properties`。
原 core profile 和 16 件封存日志不改变；请求路径仍是编译白名单，禁止重定向。
四平台的 manifest / bootstrap JSON 必须完整且非空。仅 checkout stdout 允许空文件，
或保留最多 4 MiB 前缀并显式记录 `truncated:true`，不能冒充完整日志。

原 `archive-checkout-logs-03` collector 实际 exit0：12 件 / 5116718 bytes。
SHA256：`c3ad23bae80560a7249d0295df6c765bca0d88951b7b52c0b6077fa9145e8048`。
Windows stdout 超出预算，保留 4194304-byte 前缀，`fullLogsComplete:false`；
四平台 manifest / bootstrap 均完整。01 / 02 原预算拒绝和部分证据保留，没有扩大额度。

`inspect-chromium-archive-checkouts.py` 再验原 build / log receipt、准确 URL / hash / 内容类型，
将实际仓库路径、URL、revision 与已收集的 57 个 source pins 和递归目录逐项比较。
原 `archive-checkouts-01.json` 实际 exit0；SHA256：
`e1349b8e7d96d23ac15c0d3d64085dabb7dc801a83bf36d27f242454b460b5ae`。

| 平台 | 实际 manifest 仓库 | 57 源准确匹配 | 准确路径未出现 | 身份不同 |
| --- | ---: | ---: | ---: | ---: |
| Windows x64 | 157 | 52 | 5 | 0 |
| Linux x64 | 169 | 55 | 2 | 0 |
| Mac Intel | 152 | 49 | 8 | 0 |
| Mac ARM | 152 | 49 | 8 | 0 |

Windows 未出现 fontconfig 与四个 Wayland 源；Linux 未出现两项 Microsoft 源；
两 Mac 未出现上述 Windows 五项、两 Microsoft 源及 OpenXR。
这些只是准确 manifest entry 不在清单，**不是 NOT_SHIPPED 或许可证豁免**。
不把 standalone LLVM / quiche / googleurl 等条件依赖别名到 root 的其它版本。

四平台的 ANGLE 递归目录均为 3 匹配 / 77 未出现 / 0 身份差异；
OpenScreen 均为 3 匹配 / 22 未出现 / 0 身份差异。条件原文只记录、不求值。
实际匹配路径是 ANGLE 下的 glmark2/src、rapidjson/src、VK-GL-CTS/src，
以及 OpenScreen 下的 buildtools、tinycbor/src、simple_dns/src。
准确 pins 仍来自原 sealed 目录，不猜缓存位置或改用最新 ref。

四个 bootstrap 均记录 recipe `chromium`、config revision
`82064764761481b897d400961ee71601b5d796e0` 和各自 builder 的 properties.json 路径。
原 recipe bundle CIPD instance 为 `2QjLPDj2oNMKlnOBPW4LhuQHkh6Mc52_OS_yRNlo_RwC`，
package 为 `infra/recipe_bundles/chromium.googlesource.com/chromium/tools/build`。
`requested_version:refs/heads/main` 保留为原始请求，不当作不可变实际版本。
recipe 源仍未收集，执行中的 gclient helper revision 仍未验证。
Linux stdout 显示使用 CIPD depot_tools 路径，而不是仅凭 checkout 内 depot_tools。
日志声明 `.DEPS.git`，固定主 Git 树只有 `DEPS`；实际 helper 的 fallback / 读取机制仍须查明。
禁止执行收集日志中的命令，或将已采 checkout helper 当作实际执行 helper 证明。

## 2. 原四 ZIP 与固定 GCS generation 的 bytes 关联

新增 `collect-chromium-snapshot-objects.py`：仅对四个编译对象及原 generation 做
有界 64 KiB 只读 GCS JSON metadata GET，不下载、解压或执行 ZIP，不用凭据。
原 ZIP 以 streaming SHA256 / MD5 / CRC32C 校验，单件 512 MiB 上限。
同时检查原路径与打开 handle 的 dev / inode / size / mtime，
各 API 的完整身份含 ctime 分别做前后复查。
Windows CPython 的 path-stat / fstat ctime 语义差别不被当成变更，
但同一 API 内身份变更仍拒绝，且新增反例断言。

原源目录为 `.cache/computer-use/platform-source-20261012-chromium-snapshot`。
原 metadata SHA256：`06e20000ac0d1a3ded11478afeae8a7a69e4e18979018e455c6bc830e8ad18a2`；
原 ZIP inventory SHA256：`3bafdb84f281b55dfca707500c6bfae285f6ab59fa7a08407619ced55c25e28b`。

| 平台 | 原 GCS generation | 原 ZIP bytes |
| --- | --- | ---: |
| Windows x64 | 1791806836355024 | 362195251 |
| Linux x64 | 1791802844815218 | 251048667 |
| Mac Intel | 1791802460664545 | 198203931 |
| Mac ARM | 1791801893976688 | 176665955 |

总计 988113804 bytes，未重复下载。`snapshot-object-records-02` 原 collector 实际 exit0，
inventory SHA256：`cf1414d15aac80815bf0b7e038b2e4860c45bd1bc5f0cd22e08fb68cbf22243c`。
四个 SHA256 与原 sealed ZIP receipt 一致，MD5 / CRC32C 与固定 GCS generation metadata 一致，
所以 `gcsGenerationBytesMatched:true` 已证明。

Windows 原 object timeCreated 为 `2026-10-12T12:07:16.44Z`，原上传 step 结束为
`2026-10-12T12:07:16.415938Z`，晚 **24.062 ms**；`createdWithinUploadStep:false` 保留。
其它三项在原窗口内。原时间字符串保留，比较精度显式为微秒；generation 不当时间戳。
GCS metadata 没有 publisher build ID，因此四项
`publisherGenerationCryptographicallyBound:false` 保持，时间对应不当 publisher 认证。

CRC32C 使用任务私有 `crc32c-sdk-01` 的 google-crc32c 1.7.1 cp312-win_amd64 wheel，
SHA256：`b7491bdc0c7564fcf48c0179d2048ab2f7c7ba36b84ccd3a3e1c3f7a72d3bba6`。
安装为 require-hashes / only-binary / no-deps / no-cache-dir / fresh target，
没有改共享 site 或 App 依赖；native implementation / 三 known vectors / 分块检查通过。
用 bundled Python 3.12 实际校验 ZIP，默认 Python 3.8 只运行本地 tests。
SDK 为后续重验依赖，应保留。首次01的 ctime 拒绝及原 metadata / 失败回执均保留。
Python 3.8 的不等宽小数 UTC 解析首败修正为严格有界解析，不删断言。

## 3. 本轮实际测试与质量

显式 runner 原日志 `license-tests-checkout-final-01.log`：
**192 passed / 0 failed / 0 skipped**，实际 exit0。
原 169 + checkout log 新 5 + object 11 + manifest 7；不是 product / runtime 验收。
日志 profile / truncated 前缀、固定 generation / streaming 三校验 / mutation /
UTC 24ms 差异、manifest / bootstrap / URL / revision / 条件不执行均有回归。
最初重复 bot_update step 的临时 fixture 已按实际单步多日志布局修正，未减少断言。
移除两个 tests 的无用 import；native CRC loader 的 Python 3.12 执行边界不降级。

收尾执行 owned Python AST、LF / EOF / 严格尾空白、hash / diff 与质量 final gate；
结果写入本 evidence root 的 `checkout-final-input-hashes-01.json` 和
`quality-checkout-final-01.json`。App + Workbench 13573 行，≥1000 行源码 80 件未增长。
质量通过不等于交付完成；不复记历史 Rust / native / UI tests 为本轮通过。
原 R2 的 22 个 source hashes 及三个许可证回执重新核验不变：
combined05 仍 57 源 / 7237 paths / 440 raw gaps，policy05 仍 19 yes 带错 /
187 目录错误 / 5 异常值。不能凭 manifest 缺项减少这些原错误。

## 4. 清理与后续

本批只新增小型证据 / 工具和私有 checksum SDK，没有编译 site、浏览器运行副本或重复包。
删除目标 0，logical removed 0，**实际回收 0**；当前 SDK / source / bare / 原失败证据均保留。
原 user preview PID59356 路径及创建身份已只读复查，仍为 app144；没有重开或停止它。
未知 owner cache 和该 live preview 暂缓；不可递归删除整个工作区 / target / shared cache。
清理、空闲空间与再生成说明写入 `checkout-cleanup-01.json`，空闲空间波动不冒充回收。

下一批按已证明的实际输入补固定 recipe/config 和执行 CIPD helper / `.DEPS.git` fallback，
再补六个实际递归仓库的准确 source / 许可证输入；未出现的项只留证据，不当豁免。
推进平台 license coverage、完整 NOTICE / SBOM、source 提供 / LGPL-media 和稳定安全更新策略。
之后才晋升维护 Node / PW / browser，完成四锁 / production prepare-check /
repair-rollback-bundle、四原生架构、安装 / 签名 / 更新 / 卸载及所有 surface / UI UX /
真人原 ACP-MCP-model-picker / 同一最终候选 12h / D00–D31。
