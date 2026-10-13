# CU 维护浏览器替代来源与完整 Credits 候选

后续 [Linux155完整包与静态ABI](2026-10-13-computer-use-linux155-source-checkpoint.md)已完成：673文件/8.28MiB Credits候选、9个ELF符号检查；native/分发仍未通过。下方Linux“未下载”及25测试是本检查点当时状态，不覆盖后续39测试结果。

2026-10-13；`feat/computer-use-implementation`；active / partial。
本批只改来源审查工具/测试/文档，没有修改产品 pins、四锁、seed、worker、App144 或用户数据；未提交、push、PR、发布或执行新增浏览器。
用户确认的 **受管浏览器 Mac14+ / App11+** 边界保持；Mac native 仍 not_run。

## 新的实际发现

此前同 commit 官方 Chromium157 快照的四份 Credits 均是401-byte sample，仍不允许发行。
这批核验了 `ungoogled-software` 三个平台的公开 release/固定源码 tag，并实际下载与只读检查 Windows155 替代包。
这只是来源提案，不是已批准的新产品依赖。

| 提案 | 实际查询到的 release | 固定平台仓库 commit | 本批范围 |
| --- | --- | --- | --- |
| Windows x64 | 155.0.8059.39-1.1 | `989b4382364baaddb8b3180e1eb1030ca96a7273` | 完整ZIP/hash/CRC/树/架构/embedded Credits实查；未运行 |
| Linux x64 | 155.0.8059.39-1 | `025d99db1673a21992534551598b73ca79360762` | release/assets/hash和构建参数只读查询；二进制未下载/未验 |
| Mac ARM与Intel | 154.0.8037.97-1.1 | `f7ba75f94442abda7ac3ea81790c217f8636d3ba` | 两DMG release/assets/hash与参数查询；未下载/未验 |

这些版本不同，不能拼成“同候选四架构通过”，也不能仅凭 latest/tag/non-prerelease 判定安全补丁已完整。
查询中 Chromiumdash 与 CfT channel/version也不同；最终版本需核对固定branch、日期、补丁，而不是自动挑一个字符串最大的 latest。

## Windows源包的可重复身份

- release [155.0.8059.39-1.1](https://github.com/ungoogled-software/ungoogled-chromium-windows/releases/tag/155.0.8059.39-1.1)，asset631109002，ZIP199985084 bytes。
- archive SHA256 `7ab1bac653c531ea06cd4f08a59120e129d53f73f2cf7a00ae52a43276849f11`，与 release API提供的digest实际匹配；这不是独立发布者签名或可复现构建证明。
- archive root `ungoogled-chromium_155.0.8059.39-1.1_windows_x64`。
- 全ZIP75普通文件/440890306 bytes，单文件最大301012480 bytes，最长路径91字符，压缩比2.204616，无links。当前200条目/400MiB单文件/512MiB总树额度都能容纳；仅这一提案，不推导其它平台。
- tree SHA256 `be521a1edcc71e2a79b157ae0fb50a0161c0647b775198e2ae384c95140fc13b`；全部条目读完校验CRC，没有只读目录。
- chrome.exe/chrome.dll/chrome_elf.dll均为PE x64头；只是binary metadata，不是运行验收。
- 没有独立LICENSE/CREDITS/NOTICE文件；在53个`.pak`中找到完整Credits候选。
- resources.pak / resource40001：Brotli153456 bytes → **8969504 bytes（约8.55MiB）**，SHA256 `4d90fba751a16438bf5f13a1023ee85471acfc43b1e6a1fcb9b7492d4562d3bb`。
- markup中770个product title/759个不同title（含重复组件）；含Chromium、V8、BoringSSL、ICU、FFmpeg、SwiftShader、WebKit与patchset声明。标题数量或copyright/license计数均不证明真实二进制依赖覆盖或完整SBOM。
- 完整原样HTML仅保存在task-local evidence，**未渲染/执行**。

工具：`tools/computer-use-probe/inspect-ungoogled-chromium-sources.py`。
源包：`.cache/computer-use/ungoogled-20261013-win155/Win_x64.zip`。
最终原样HTML及完整per-pack SHA/树/架构/provenance报告：
`tools/computer-use-probe/.run/maintained-browser-20261013-01/credits-win-02/`。

## 为什么仍不能晋升

1. [固定Windows flags](https://raw.githubusercontent.com/ungoogled-software/ungoogled-chromium-windows/989b4382364baaddb8b3180e1eb1030ca96a7273/flags.windows.gn)实际为`is_official_build=true`，但同时`proprietary_codecs=true`、`ffmpeg_branding="Chrome"`、`enable_swiftshader=false`。Mac/Linux参数也启用专有媒体编解码。Credits候选不能替代相应许可、源提供、再分发及可能的专利义务审查；也不能私自去掉媒体能力来缩小原产品范围。
2. [package.py](https://raw.githubusercontent.com/ungoogled-software/ungoogled-chromium-windows/989b4382364baaddb8b3180e1eb1030ca96a7273/package.py)通过Chromium FILES.cfg产出包；patchset gitlink实际固定到`ffeb45a2bb88bc56a1dfed31410aeb10e9efcd5a`，其版本文件155.0.8059.39，仍须核验全部patches、DEPS、CIPD/generated inputs和实际随包组件。
3. Chromium155.0.8059.39 tag实际commit `3ff7ac5a9224be9156d7f8703a06e22890aafd34`。官方lite源包generation1791333474509881、2021197344 bytes、发布SHA256 `d450182ea38a6485febfdfdad6f3a3f1d5d3408cc368329b94502c629ef8ec7e`。本批只读到官方metadata/hash文件，**没有下载或重现这份约1.88GiB源码**；报告明确downloaded/reproduced=false，不将官方源码hash当作已验二进制输入。
4. 公开源码tag/asset digest不等于二进制来源认证、可复现构建或安全签名通过。安全更新策略、四架构相同版本、Linux glibc2.35、Mac原生14+、软件渲染和PW1.64完整行为都未验。
5. 源检查器的exit0仅表示找到完整Credits候选。report的redistribution/native/security/otherTargets分别保持not_proven/not_run/not_audited，不让来源检查生成runtimePrepared/manifest/授权。

## 本批工具回归

- 原固定快照CLI仍只接受原四hash/root；复用内部显式pin的只读检查函数没有新增CLI覆盖、修改原锁或支持latest。
- 新proposal pin要求严格SHA256、安全平台label/root；ZIP路径新增NUL/colon拒绝，保留原budget与链接限制。
- DataPack/credits18个测试、proposal contract7个测试通过（0skip）。原proposal来源不符、size/复读hash/architecture错误均拒绝。
- 修改后的检查器重读原四官方157快照：每平台231packs，仍只有401-byte sample；**实际进程exit2**，门禁failed_missing_full_notices。外层测试仅确认该预期拒绝，没有将原通知门禁刷绿。
- `git diff --check`与FINAL质量门禁通过；App+Workbench13573/≥1000源码文件80未增长。未改产品代码，所以没有重复此前980 Core/477前端作为本批新验收。

## 下一批继续R1，不直接跳R2

1. 比较固定155 source/patchset/DEPS/完整Credits与实际Windows文件；检查第三方许可证、NOTICE与源提供义务，完成可追溯输入和真正的SBOM/更新策略。不要以页面字数或generator exit0充当完整覆盖证明。
2. 查Mac两架构可匹配的受维护版本。DMG需要native只读取证、布局、Framework links与签名身份；不能在Windows凭release URL算native passed。
3. Linux源包按固定hash/整个tar类型与CRC/架构/DSO ABI/同version读取；不能用AppImage metadata替代Jammy原生import/browser。
4. 若第三方方案不能满足原范围、来源/许可/更新要求，保留官方快照红灯并推进固定来源的受控构建/完整notices，不混用旧版credits或静默回退EOL。
5. 只有选定完整四平台来源后，继续原[R2–R4](2026-10-13-computer-use-maintained-runtime-next.md)：quota/pins/四锁/prepare/check/repair/rollback、四架构native动作、安装签名与真人链、同最终候选12h/D00–D31。

## 预览和清理

本批只读复查唯一用户预览app144/PID59356、原EXE路径与创建时间仍匹配；没有启动另一版、操作UI、代发模型任务、批准权限或选择文件。home/909字未发草稿保持；fixtures仍停。
两个原audit和全部本批tests均同步退出后，仅删除第一份与最终第二份SHA/长度完全相同的解码HTML副本；第一份report保留，并由retirement receipt映射到完整canonical bytes。
逻辑删除8969504 bytes；H即时空闲观察增加8970240 bytes（约8.55MiB），不是独占物理回收证明。清理脚本补齐直接父目录的reparse检查，随后完整复查两目录到盘根，均无reparse。
精确路径、保留对象和再生方法：`.run/maintained-browser-20261013-01/duplicate-credits-retirement.json`。
源ZIP/原报告/最终HTML/官方快照/seed/current build/全部用户数据与未知归属cache保留。没有停止其他进程。
