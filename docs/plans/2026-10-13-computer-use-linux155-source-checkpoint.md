# CU Linux155：完整源包、通知候选与静态 ABI 检查点

2026-10-13；`feat/computer-use-implementation`；active / partial。
这是 R1 来源审查增量，不是运行时晋升或浏览器实机验收。受管浏览器 macOS 14+、App macOS 11+ 的用户确认边界保持；没有修改产品 pins、四锁、seed、App144 或用户数据，也没有 commit/push/PR/release。

## 完整包实际读取

- 发布仓库 `ungoogled-software/ungoogled-chromium-portablelinux`，release `155.0.8059.39-1`；平台源码 commit `025d99db1673a21992534551598b73ca79360762`。
- 缓存 `.cache/computer-use/ungoogled-20261013-linux155/Linux_x64.tar.xz`，156893644 bytes；实际 SHA256 `acc30f037e0080a0bf03456f8cf8cda4aca55a778b17174d0a4aaa8e7749cb79` 与固定发布摘要匹配。
- archive root `ungoogled-chromium-155.0.8059.39-1-x86_64_linux`；675条目（673普通文件、2目录），全树797546240 bytes，单件最大531621048 bytes，最长路径82字符。
- 全树摘要 `4522f0be60fa18edcb1d56d8b3ebb32a3c0fde34fe6865890c26d27494a5d0c2`；所有有效 payload 流式读完，同时检查 XZ、tar header、双 endblock、padding 和尾随数据。9个ELF头均为 x86_64。
- 331个 `.pak` 实际读取；resource40001 的完整 Credits 候选8686083 bytes（约8.28MiB），SHA256 `8fe4d69cb2b007cc6726371a94b5d0c07f266424fc7ae483157cd00456ae811b`。没有独立 LICENSE/NOTICE 文件，内嵌文档仍需与实际源码/组件核对，不称许可证覆盖已通过。
- 原报告和未执行的原样 HTML：`tools/computer-use-probe/.run/maintained-browser-20261013-02/credits-linux-01/`。

## 静态 ABI，不是 Jammy 原生启动

从同一固定源包只复制9个ELF作为 readelf 数据输入，逐件 hash；WSL Debian Binutils2.44 执行18个只读 version-info/dynamic 命令，原命令全部 exit0。

| 文件 | 最高 required GLIBC |
| --- | --- |
| chrome、chromedriver | 2.25 |
| crashpad、SwiftShader | 2.17 |
| EGL、GLES、Vulkan | 2.16 |
| Qt5/Qt6 shim | 2.14 |

无非数字 GLIBC 需求，以上符号底线不超过 Jammy glibc2.35；这不证明 loader、NSS/Qt/其它依赖、sandbox、浏览器、PW1.64、安装或 portal 可用。浏览器没有执行，nativeAcceptance 保持 `not_run`。
完整18份 readelf 输出和 `abi-inventory.json` 保存在 `.run/maintained-browser-20261013-02/abi-01/`，9件输入身份在 `elf-inputs-01.json`。

## 归档合同发现

Linux包的673文件超过当前200门槛；最大单件/总树超过默认400MiB/512MiB，但数值落在生产600MiB/768MiB内。这不是允许直接使用生产 ZIP policy 的理由：输入是 tar.xz，必须有生产格式支持、完整配额、所有条目/链接/路径/部分失败保护和四平台一致合同。不能只抬一个阈值、删文件缩包或把只读检查器当 materializer。

Mac最新来源查询仍为 `154.0.8037.97-1.1`、平台commit `f7ba75f94442abda7ac3ea81790c217f8636d3ba`，两DMG尚未下载/native验。它不能与Win/Linux155拼成同版本四平台候选。
上游155 source commit `3ff7ac5a9224be9156d7f8703a06e22890aafd34` 和约1.88GiB源码tar metadata/hash已查，但源码未下载/重现。固定tag/公开digest不等于二进制来源认证、源覆盖、安全签名或再分发批准。codec、SBOM/源提供义务和更新审查继续开放。

## 工具与回归

- `bounded_tar_xz.py`：有界 LZMA 解码/128MiB memory/64KiB输出；regular/dir/有界GNU longname；拒绝 links、PAX/global/sparse/特殊类型、模糊NUL、大小/条目/metadata/尾随越界；不提取或执行。
- `inspect-ungoogled-linux-sources.py`：固定 SHA/size/root，全包/tree/native头/Credits读取；不生成 runtimePrepared、manifest 或授权。
- 新tar8、Linux6，加既有DataPack18、Windows7，联合39 passed/0skip。原两次完整Linux检查均 exit0，只表示找到通知候选。
- diff 和 FINAL 质量门禁通过；App+Workbench13573、≥1000源码80未增长。没有重复运行旧980 Core/477前端并把它们记为本批验收。

## 后续：固定平台源码与共同补丁实际归档

`inspect-ungoogled-provenance.py` 已完整保存三个固定仓库的281个普通源码对象，共6432224 bytes。实际 Git commit → tree → blob 身份、声明/实际长度及每件SHA256均核验；所有下载内容以 `.source-data` 存储，仓库文件名不成为本机可执行脚本或本地路径，没有执行任何上游脚本。

| 仓库 | 实际 tree SHA1 | 文件 / bytes |
| --- | --- | --- |
| Windows | `ebf09b7a575992e56df7c7c7abd7f81921070d17` | 53 / 2583566 |
| portablelinux | `ba6e8d1d6ccee4bfbfbf9d59b6452f07b9a4658f` | 32 / 95530 |
| 共同补丁 | `94bdc2817f2534d19de78b9c13b30daf82d5460f` | 196 / 3753128 |

两平台 gitlink 确实共同固定 `ffeb45a2bb88bc56a1dfed31410aeb10e9efcd5a`。这只证明公开平台/补丁源对象，不证明发布二进制实际采用这些输入，更不是完整 Chromium/DEPS/CIPD/generated inputs 或可复现构建。
完整新receipt `tools/computer-use-probe/.run/maintained-provenance-20261013-02/provenance-inventory.json` 的 `collectionComplete:true` 仅表示上述三仓库收集完成；sourceReproduction/native 为 not_run，binaryAttestation/licenseCoverage 为 not_proven，securityAcceptance 为 not_audited，promoted:false。

实际审阅的差异与未关闭项：

1. 共同 `flags.gn` 明确 `safe_browsing_mode=0`。series 包含关闭 Safe Browsing、下载 quarantine/MOTW 和默认 FedCM 的补丁；下载补丁移除了原 quarantine 服务与 Windows Zone 标记路径。这些是安全/登录体验差异，不能把包当普通 Chromium 的无差别替代，也不能以 Broker staging 替代原浏览器保护的验收。未经明确设计与评估不晋升产品。
2. Windows `enable_swiftshader=false` 与 Linux包含SwiftShader的差异仍需原生图形/软件渲染验证；不能把一种平台成功推导到其它平台。
3. Win/Linux两份完整 Credits 的 ffmpeg 项实际写明 LGPL v2.1+；通知体积/源链接不替代特定编解码构建、对应源、链接/重新链接及源提供义务和适用专利的审查。共同 `add-credits.patch` 还把 reciprocal link 改为共同补丁仓库首页，该首页不是完整固定 Chromium 源和依赖的已验证镜像。
4. Linux构建 Dockerfile 使用滚动 `debian:trixie-slim`、apt升级、Node24仓库与 npm全局更新；这不等于所有工具链输入固定或可复现。Windows许可证补丁仅调整 GN路径，不是可推定的完整 license coverage 证据。源码对象已保存，实际重现、构建输入/SBOM与安全更新策略继续开放。

首轮来源收集原 session47856 因HTTPS握手超时 exit1，Windows/Linux源及partial receipt保留；共同补丁没有完成，不能记成功。仅对固定公开只读GET增加最多3次网络超时/指定5xx重试，不重试hash、路径、redirect、404或大小错误，不改代理。第二轮原 session18431 exit0、三仓库收齐、下载 owners全部join。终态错误会取消尚未开始的请求，已开始的原owners仍join。
一次 `unittest discover` 因连字符文件名运行0测试，不计通过；已改为逐一显式调用测试脚本。新增404 fixture 首次13 passed/1 error（Python3.8构造了不真实的无body HTTPError），补成实际body对象后保留“不重试404”断言，不修改生产拒绝条件。最终结果以本检查点收尾记录为准。
两次 source-data 与首败receipt是本批有用证据，保留；没有新增构建包或可退休浏览器进程，额外手工清理0，不重复删除上一批9 ELF。

最终显式联合回归：DataPack18 + Windows7 + tar8 + Linux6 + provenance15 = **54 passed / 0 failed / 0 skipped**；新增测试涵盖只读网络有界重试、404/redirect/大小不重试、固定Git身份、路径/文件预算、新输出/data-only路径及终态失败取消队列并退出原pool。源collect实取证使用收尾前的同来源校验逻辑；后续取消队列/去未用形参为工具收尾，不重写既有receipt或称二进制重现完成。

## 产物收尾

原下载、两个完整来源检查、stager、全部18个 readelf 和测试 owners 均已退出。仅删除9件临时 ELF 副本，不递归删目录；原输入目录现为空，源tar、Credits、全部报告/readelf/hash、seed/current App/source/user data保持。
逻辑移除576660288 bytes（约550MiB），H即时空闲观察增加576684032 bytes；这是共享盘空闲变化，不是独占物理归因。
准确receipt：`.run/maintained-browser-20261013-02/elf-input-retirement.json`；再生命令以 `elf-input-retirement-addendum.json` 的纠正说明为准：

```powershell
python tools/computer-use-probe/.run/maintained-browser-20261013-02/stage-linux-elf.py --output-name elf-input-regenerated-02 --receipt-name elf-inputs-regenerated-02.json
```

输出目录与receipt必须都是新路径，后续ABI也使用新目录，不覆盖原证据。未知归属cache和唯一用户app144未清理。原EXE/PID59356/创建身份在本批最后仍匹配；隔离home/login/历史/909字未发草稿保持，fixtures仍停，没有GUI操作、代发任务、批准或选择文件。

## 下一批

按 [R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)继续：先固定source/patchset/DEPS、通知与实际组件/源提供义务及安全审查；找到可匹配的两Mac来源后再迁移生产归档/四锁/compiled pins/prepare/check/repair/rollback。新完整候选仍需四架构原生动作、安装/签名/真人ACP/MCP/model/picker、四surface/UI UX和同最终候选12h/D00–D31，不 mark complete。
