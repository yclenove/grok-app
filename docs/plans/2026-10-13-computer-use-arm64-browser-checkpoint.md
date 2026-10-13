# Computer Use：维护版 Linux ARM64 浏览器候选

2026-10-13，当前阶段是固定来源核验和原生诊断；产品 compiled pins/锁/seed
仍是原版本。没有把候选浏览器或 Node 重打进用户 App144。

## 已取得的真实来源

Playwright1.64.0原 `browsers.json` 的浏览器版本为 **156.0.8078.4**、revision
1248；实际 registry 给 Ubuntu22/24/26和Debian12/13 ARM64 使用
`linux-arm64/chrome-linux-arm64.zip`。不是开发快照157、x64包或系统浏览器。

| 来源 | 实际核验 |
| --- | --- |
| Node22.23.3 Linux ARM64 | 官方SHASUMS256一致，归档56,655,163 bytes；SHA256 `5ced2d48d1d7198739b7f86804de0171aefb6823b684b12341d3321afc3cb0b2`；完整原LICENSE与所有归档项已核验 |
| CfT156 Linux ARM64 | 固定GCS generation `1790729770695645`；199,750,508 bytes；官方MD5一致；SHA256 `910a12fff6170202195f72ef3d353801c7063d301ab80b1f51b77d791fda60ce` |
| CfT完整文件树 | 301文件/306项/418,439,600 bytes，treeSHA `3a03998c5a13331748583eb9d9c8689186c648ef5ec2adb5e05e3dbef41083f1`；无links；chrome原生header为AArch64 |
| 静态ELF/ABI | Node、chrome、crashpad、sandbox和四DSO全部AArch64；可执行loader为`ld-linux-aarch64.so.1`；所见最高GLIBC Node2.28/browser2.25，不代替Noble原生启动 |
| 内嵌Credits | 223个pak实际解析；5,421,636-byte完整候选，SHA `496f23ae4815bc7183266fe92958a4bfeb694813e997da31a6ab465d5eaf1023`，不是401-byte sample；完整许可/媒体/再分发范围仍须核对 |

官方Chromium snapshots对象前缀中没有当前 `Linux_Arm64`；所见
`Linux_ARM_Cross-Compile/LAST_CHANGE` 为270195，最终对象创建于2014-05-13，
不能作为维护版ARM64替代。没有下载/运行该旧来源。

## 实现和验证

只读native header审查加入ARM64 ELF，CfT source inspector按每个固定target
核对实际native architecture，拒绝跨架构/PE代Linux；通用ZIP树审查与各
source wrapper的独立身份合同保持。维护诊断stager加入固定ARM64三件来源；
完整SHA/npm SRI/类型/配额/架构核验后才写新task-owned目录，不修改产品seed。

新的 `linux-arm64-maintained-browser` CI job 使用原生Ubuntu24.04 ARM64。
在私有诊断中实启包内Node22/PW1.64/CfT156，再跑现有五suite的28项操作、
观察、上传、截图/drag、wait和退出检查。runner要求28pass/0skip；版本、
架构、OS、ABI与原生worker都必须匹配。只上传JSON/log/TAP，不发布来源
binary；诊断不是安装/App/沙箱策略或完整产品验收。

本机已完成独立staging/完整五平台CfT来源清点与62项来源工具回归；
x64 WSL真实拒绝ARM64 runner，未执行ARM binary或冒充原生通过。
新ARM64 CI实际结果须以后续run身份回读为准。

证据：`tools/computer-use-probe/.run/ci-followup-20261013-01/` 的来源receipt、
五平台inventory、ARM64 ABI和原错误log；固定GCS metadata在前批SDK证据根。
当前独立ARM64stage `maintained-native-arm64-20261013-01` 保留供后续核验。

## 继续完成

先回读原生诊断结果；然后决定有完整来源与通知的最终维护浏览器合同，
补齐产品ARM64 target/compiled pins/锁/layout/repair/rollback和发行资源。
仍须完成Mac/全Linux/Windows安装态、四surface、真人模型/ACP/MCP/picker、
UI UX以及同最终候选12h；App11+/CU受管Mac14+和原交付范围保持。
