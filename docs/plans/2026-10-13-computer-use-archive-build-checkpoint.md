# CU 维护浏览器：四平台实际归档与 GN 参数

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[gclient机制/公开build入口](2026-10-13-computer-use-gclient-build-evidence-checkpoint.md)。
App 仍 macOS11+，CU受管浏览器14+；原四架构、四surface和D00–D31范围不缩减。
本批仅来源审查工具、公开证据与文档；产品/pins/四锁/seed/App144/home没有晋升。
无提交/推送/PR/发布/GUI/模型任务/自动授权/用户文件选择，不改代理、不使用历史凭据。

## 1. 四条准确 GetBuild 原记录已封存

新增 `collect-chromium-archive-builds.py`，网络前复核独立封存的361条search receipt、
四份raw page/hash、完整inventory和目标存在性；不只相信查询tag。
RPC只允许四个编译ID与准确 `id,builder,status,input,output,steps,tags` mask。
4MiB/record、20s、跳转在请求另一个endpoint前拒绝；不调度/取消build或使用凭据。
坏record也先保存有界raw，迟败保存已完成项，不覆盖原证据。

`archive-build-records-01` 原工具实际exit0；**4 records /1260152 bytes**。
独立root SHA256：`851226dbba1f79047c0998234bb885feea6d96c65548540e1e15f93bff7476d4`。
四条input/output均准确匹配Chromium commit
`f52796fc20c6e7d618374bb8bf096e7b2155e14c`、position1716663，
ANGLE/SwiftShader/V8/WebRTC实际output pins与原收集来源一致。

| 目标 | builder | build ID | 实际成功上传的相对路径 |
| --- | --- | --- | --- |
| Windows x64 | win-archive-rel | 8668153607039536273 | Win_x64/1716663/chrome-win.zip |
| Linux x64 | linux-archive-rel | 8668152676980535313 | Linux_x64/1716663/chrome-linux.zip |
| Mac Intel | mac-archive-rel | 8668153412503290689 | Mac/1716663/chrome-mac.zip |
| Mac ARM | mac-arm64-archive-rel | 8668153878055557601 | Mac_Arm/1716663/chrome-mac.zip |

每条准确 `gsutil_urls` key/value和同名upload step SUCCESS都实核，bucket为
`chromium-browser-snapshots`。这是实际publisher路径证据，**不是GCS generation/
现存ZIP bytes关联证明**；四平台这两项继续false，不以SUCCESS当完整交付。

## 2. 同次构建的16份原日志

新增 `collect-chromium-archive-logs.py`，先复核上述root seal、raw hashes与重新
导出的summary；只读每个build准确的gclient config/config、generate_build_files/stdout、
read gn_logs.txt/gn_logs.txt和该ZIP upload/stdout，16条编译白名单路径。
不跟任意同build日志，所有请求必须与编译target/key/step/path完整一致。
4MiB/log、20s、NoRedirect；只接受UTF-8 text/plain，HTML/login/binary拒绝。
原日志全部是inert `.log-data`，绝不执行其中命令或访问其中临时文件路径。

`archive-build-logs-01` 原collector实际exit0；**16 logs /22343 bytes**。
root SHA256：`9056f1b81f8822cb6852308ec6b57bf8f5cc2660eac449bfd41b3f8797ec7e17`。
之后补强fetch的编译精确路径和迟到HTML保留，未重收/覆盖此01；inspector再次按
最终白名单逐项核对原16URL/bytes/hash/content type。

四个实际gclient config均只有 `src` solution；记录的deps_file为`.DEPS.git`，
custom_deps为空，custom_vars仅 `download_remoteexec_cfg:"True"` 与rbe_instance。
原字符串保留，不将"True"当bool；没有build_with_chromium override。
**不是实际checkout manifest、gclient运行helper版本或完整递归变量求值证明**，
不据此排除原19个yes metadata错误或把standalone pins直接别名成parent pins。

## 3. sample Credits 的实际参数链

新增 `inspect-chromium-archive-inputs.py`，实际 `archive-inputs-01.json` exit0；SHA256：
`37917da5449b8db8430c031aa098c488b149c9f32452efea8e85120d16627589`。
四完整GN写入block仅作literal parser，拒绝重复/dynamic/import/interpolation，
实核原Done行和准确target_os/cpu；不执行GN或上游Python/DEPS。

四份记录均显式 `is_debug=false`、`is_component_build=false`、remoteexec/siso开启；
Windows/Linux/Intel/ARM平台分别win-x64/linux-x64/mac-x64/mac-arm64。
**均未显式设置is_official_build或generate_about_credits。**
同固定commit/tree的两个原文件独立校验bare/root tree/path/Git blob/SHA256：

- `build/config/BUILDCONFIG.gn` blob `a8dcb4d4675505227c2259a8851d9b2ee904faef`，
  SHA `1b33f01310e829a6f8b2e675e944fd32b35d14809571df6d1d86d1f6b051e6f7`；
  line136准确默认 `is_official_build=false`。
- `components/resources/BUILD.gn` blob `fd2fb05219e396333661c94319545aa546191fb6`，
  SHA `1d06064122da27e753b0da6e3fce990ba6cc13f179fbf19a309420fe657cafb9`；
  line19默认credits随official，line158/185–188关闭分支复制sample。

因此原四archive的**实际写入参数+同源默认声明**均通向sample机制，与已读401-byte
Credits一致。不是完整GN graph的重新求值、license generator完成或下载ZIP关联认证。
不要再只用“源码可能默认关闭”解释，也不要把该原因明确误写为通知已补齐。

## 4. 测试、质量、清理与下一批

显式runner最终原日志 `license-tests-archive-final-02.log`：
**169 passed /0 failed /0 skipped**；原140保持，新增archive15/log8/input6。
涵盖fixed mask/四目标/commit/position/pins/上传步、原封存search/raw再验、
同build精确日志/伪造路径/HTML/迟败保留、literal GN平台/不执行表达式与原custom字符串。
首轮15tests有1个fixture错误（exclusive write拒绝测试修改自己临时receipt），
在临时fixture中修正写入方式后全通过；没有删断言。初次diagnostic未指定Accept返回
protobuf及本机Python3.8无removeprefix的错误也保留在工具输出，不改称网络不可用。

本批实际代码质量preledger/final、Python语法、owned LF/EOF/hash/严格尾空白、
diff均复验；FINAL: PASS仅本批质量。App+Workbench13573/≥1000源码80不增长。
最终27 Python AST/28严格尾空白/34 LF与EOF文件通过；旧Markdown hardbreak及wiki
历史多个EOF换行保持，不对其它3件WIP的CRLF警告做无关重写。格式首轮在额外
要求旧wiki单EOF时拒绝（01空report保留）；收尾仍严格要求tools/新检查点单EOF，
旧文档只核其原LF/EOF，02/03重新保存，不将失败首轮记为通过。
R2原22源码SHA不变；原closure05/policy05/classification03不重算，仍57源/
440 raw gaps、19 yes带错/187目录错/5异常值，原完整NOTICE/R1门禁红。
无新编译site/运行browser副本/重复包可退休；删除0/logical0/实际回收0。
source/CI/bare/当前候选/依赖/seed/用户home/全部失败与成功证据保留；未知owner cache暂缓。

下一批：将GCS metadata/generation/CRC/MD5与已下载四ZIP精确关联，读取同build
bot_update manifest/实际DEPS和recipe/config版本，补真实递归/平台checkout证据。
在此基础上补完整同源NOTICE/SBOM、source提供/LGPL-media及稳定安全更新策略。
不为清零metadata直接排除递归项或拉441非Git源。然后按
[维护版R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)完成最终来源选择、
四锁/真实production prepare-check/repair-rollback-bundle、四原生架构与安装/签名/
更新修复回滚卸载、所有surface/UI UX/真人原ACP-MCP-model-picker/最终同候选12h/D00–D31。
