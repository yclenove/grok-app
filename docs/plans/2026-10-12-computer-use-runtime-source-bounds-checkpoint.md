# 维护运行时来源与归档边界检查点

2026-10-13 最新[系统边界与维护来源检查点](2026-10-13-computer-use-managed-browser-boundary-checkpoint.md)：用户已确认受管Mac14+/App11+，原生只读版本门禁与十五语言错误已接；Windows/Jammy完整Core均980、前端477/strict通过，Mac native仍not_run。上一批Node22/PW1.64实际Win/Linux source import已收尾，但未形成完整pack。四同commit官方未品牌化快照实查仅401-byte sample Credits，完整通知门禁失败，不能发布；现有445-file seed无变化。下一步依新落地顺序，完整交付仍partial。以下963/975及旧PID均为历史。

2026-10-12；`feat/computer-use-implementation`；active / partial，未提交、推送、PR或发布。
本批有产品修改与真实 Windows/Linux 回归，不把源包清点当运行时升级完成。

## 产品修改

- ZIP新增总条目限额（Chromium1024、Node10000），目录与链接也计数；原文件数/字节/路径/压缩率与SHA门禁保留。
- 目录与root必须是真正无payload的目录；目录重复/大小写别名、特殊mode、空路径段拒绝。未选文件仍须完整CRC读取、精确大小核对并消耗全部配额。
- 选中文件读取限制在声明长度加一个校验字节内；保留Unix executable bits，不转移原entry的所有权。
- ZIP完全校验成功之前不删除原destination；归档拒绝保留原有效内容，临时staging收尾。此处仅服务owned staging，不代替最终seed publication/rollback journal。
- 没有提高现有browser的200文件/240路径限制，也没有改变compiled版本、四锁、权限、模型工具或immutable seed。

## 验证与原失败

| 门禁 | 实际结果与范围 |
| --- | --- |
| 最终Windows完整Core | session83896，963 passed / 0 failed / 0 ignored，150.30s；包含最终10项ZIP回归与全部runtime测试 |
| 最终WindowsCore all-targets strict | exit0，3.99s，`-D warnings`；没有新增warning豁免 |
| Jammy/glibc2.35当前Core | session84863 exit0；真实Rust1.98编译、ZIP10 passed / 0 failed / 0 ignored，all-targets strict通过；不是Linux完整Core或App验证 |
| 输入/源包oracles | Python11 passed；Node22/24四架构、四browser实际完整清点通过；未执行任何新增runtime |
| target/before-build | Node8 passed / 0 failed / 0 skipped |
| 原source03 App内容/seed | 独立checker再次通过；没有修改或复用其target作为本批Core候选 |

首次Windows全Core963/strict通过之后又审查补了Unix借用修正，以上是修正后的重跑，不拼接前后候选。最终 `archive_zip.rs` SHA256 `2ca9ec04ee4b3709b5ac621f66adf9cc98b8cccaf567ee79ead7b833ec9a4f6c`；新tests SHA256 `98523c01630da9625aa6b41b037810d958999ef334df428842ab264c128a16c7`。

Jammy首次 standalone snapshot01 的 `--locked` 实际exit101：复制整个workspace lock到独立包改变了解析图；没有去掉 `--locked` 刷绿。原snapshot/log保留。第二次新snapshot02保留真实workspace manifests、path crates与patch graph，同一原Cargo.lock下offline编译/断言/strict通过；前后冻结输入SHA复验通过。不是修改旧source03。

## 来源与兼容性结论

实际报告在 `tools/computer-use-probe/.run/runtime-source-inventory-20261012-01/`，包含Node24、Node22、browser全树/metadata及首次诊断失败；Jammy两个原log/input清单分别在 `.run/runtime-archive-jammy-20261012-01/02/`。

1. Node24.21.0四源包全清点：Windows2459条目、Unix各5888；原LICENSE在Windows为CRLF、Unix为LF，完整内容归一化比较相同。保留各源包原LICENSE字节/hash，未改通知。两Mac二进制实际最低13.5，官方[固定tag BUILDING](https://github.com/nodejs/node/blob/v24.21.0/BUILDING.md)也确认；不符合当前App发行配置11.0。
2. 因此另行获取仍维护的Node22.23.3四官方源包并核对SHASUMS256：Windows2540条目、Unix各5866；原二进制/hash/License及全树类型/限额已验。**两Mac实际Mach-O最低11.0**，与[固定tag官方说明](https://github.com/nodejs/node/blob/v22.23.3/BUILDING.md)一致。所见官方schedule的维护结束为2027-04-30；PW1.64声明Node>=20。Node22是保留现有OS底线的优先维护候选，不退回EOL Node20，不依赖系统Node；真实import/晋升仍待做。
3. Browser四包305/309/673/673条目；文件305/301/349/349，最大路径78/80/267/265，最大单文件约303MB，树约383–458MB。两Mac含324目录与5links。新增总条目限额覆盖实际清点，但原文件/路径quota、new roots/framework/五精确links仍须随最终维护候选迁移。
4. CfT两Mac `Info.plist`与可执行文件/Framework实际最低都是13.0；PW1.64官方registry所见支持入口为Mac14以上。**metadata不是13/14实机运行证明，也不能静默把App最低11.0改高。** 维护browser的分发/支持底线仍开放，未打包CfT或改为用系统browser缩范围。
5. 上游MEMBER在[许可issue21](https://github.com/GoogleChromeLabs/chrome-for-testing/issues/21#issuecomment-1594319082)明确CfT binary适用Chrome条款，仓库Apache只用于仓库代码。另一用户的“不能再分发”评论不是上游授权或法律结论；当前仍未取得完整binary再分发依据/第三方NOTICE，不能套旧BSD-3/CREDITS通知发布。继续评估可维护、可分发的未品牌化来源。

Node22初次curl五件下载全部connection-aborted/exit1，且目录无文件；原handles终态后，用现有Windows HTTP client下载成功。没有调整代理、VPN、WSL网络或关闭TLS/哈希校验；不把这个工具网络失败误记产品runtime失败。

## 清理与后续

原Windows编译/test owners与Jammy session均已终态。只删除本批两个精确退休incremental generations：逻辑600867549 bytes（约573MiB）；H盘即时可用观察增加436842496 bytes（约417MiB），不是把逻辑大小冒充物理回收，可能受hardlinks/并发活动影响。精确路径、保留的新代次及再生命令在 `cache-retirement.json`；来源、当前Core binary/依赖、两个Jammy原snapshot/log与所有failure evidence保留。没有删除workspace、target整体、node_modules、shared cache或其它worktree，没停进程。

当前用户app144/PID19768/home/login与未发送草稿保留；本批源码未重打进该包。当前compiled仍Node20.18/PW1.48/Chromium130，**不能据本批绿灯发布维护版或称整套CU交付**。

下一步先做Node22正式production extractor/实际import与PW1.64严格materializer接线，在独立新候选中完成；再落实维护browser合法来源/系统底线及完整四锁、布局/限额/NOTICE/SBOM、prepare/repair/rollback/动作/截图/生命周期。Linux三格式安装、Windows真人原ACP/MCP/model/picker、全部三平台/两Mac架构/四surface/输入与文件能力/UI UX/model矩阵/同最终候选12h/D00–D31全部范围不变。
