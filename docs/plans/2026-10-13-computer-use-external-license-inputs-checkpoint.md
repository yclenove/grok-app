# CU 维护浏览器：核心外部许可输入与引用闭包

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[主仓库固定源](2026-10-13-computer-use-pinned-license-source-checkpoint.md)。本批仍仅 data-only 审查工具/文档；不改产品 pins、四锁、seed、App144，不提交/push/PR/发布，不操作 GUI/代理/用户授权。
App11+/受管Mac14+和完整四目标、四surface、安装/UI UX/model/picker/最终12h/D00–D31范围保持。

## 原始采集与改进，不覆写证据

第一代 `external-license-01` 原 session52996 **exit0**，11个公共核心依赖固定 commit 的 bare tree/metadata/许可原文已采集，不执行 checkout、DEPS、hooks 或上游代码。
但名称规则不足：FreeType 根 `LICENSE.TXT` 指向 `docs/FTL.TXT`，文件名不以 LICENSE/COPYING开头，第一代没有采集。`collectionComplete:true`只代表该代选定输入，不代表完整覆盖。
原第一代目录/receipt与 collector快照保留；快照 SHA256 `5937eba47d33acfe599562d0e41707fafae43ef8c95700465c6c297a11d25c9c`。

原 owners terminal 后，新 `external-license-02`：复用01的准确 bare object caches，不覆盖01原始receipt/源文件；补主仓库metadata引用和读过的FreeType根许可明确引用，再追加官方 `depot_tools` 的固定许可校验模块输入。
12个来源均来自已复验的 `deps-pin-inventory-04.json`，受管Mac14+边界没有变化。只允许静态公共HTTPS源与固定40位revision，不猜latest/镜像、不拉private/internal项。

## 第二代实际结果

第二代原命令 **exit0**；**456 selected paths / 440仓库内去重blob合计 / 1584907 bytes**。
每件真实Git blob SHA1和SHA256验证，完整tree索引与root SHA保存；所有源内容以OID命名`.source-data`，不按上游path在本机checkout。
12个组件：ANGLE、BoringSSL、FFmpeg、FreeType、HarfBuzz、ICU、PDFium、Skia、SwiftShader、WebRTC、V8、depot_tools。

- 11个核心revision见固定parent catalog；新增depot_tools `21ae2d24f8ee0ca3459bda416129d7791c4313f1`。
- 已核验这12个来源中的主metadata引用和明确追加路径，`missingParentOrExtraPaths:0`；不是所有273个Git声明或所有shipping组件已齐。
- 组件自身metadata有 **35原始未解析引用**，包含跨仓库/未采子依赖；同样不是35个shipping缺口。
- ANGLE树有79个gitlinks，FreeType1个，SwiftShader9个；其它本批来源0。仅列出原pin，不自动递归/解释checkout条件或当实际构建依赖。
- 五份核心根DEPS（ANGLE/PDFium/Skia/WebRTC/V8）原文已保存，但递归checkout条件仍未解析/收齐。
- `depot_tools/metadata/`源码仅保存为data，包含固定`metadata/fields/custom/license_allowlist.py`；没有导入或执行。

root receipt SHA256 `233be34e7e29176acff240deaa36b3d91aa05f2b2d2b054277a1394748fb12fc`。
第二代collector快照 SHA256 `2bc36c8ebd679c58276c25f7ce220e7bb8af4e39da6b5d2624919469645f352e`。

## 真实特殊许可输入

FreeType从1件提升为8件，包含`docs/FTL.TXT`、GPL替代文本、BDF/PCF README、fthash源内声明与zlib.h；这里只收原文，不自动替项目选择GPL路径。
FFmpeg的固定`README.chromium`声明`Shipped: yes`、`License File: CREDITS.chromium`，实际47966-byte原文已经按引用补采。
其metadata声明BSD/ISC/LGPL2.1/MIT；源树包含GPL与LGPL文本，不等于这份browser已启用GPL。编译配置/媒体组件、源提供与LGPL分发义务仍未核验，不能从“Linux没有独立ffmpeg库”推定满足。

## 主仓库+外部输入的独立复验

`inspect-chromium-license-input-gaps.py`不联网、不执行源码：重读两个固定root receipts、所有full-tree索引和 **6289个selected paths**，逐件校验Git blob与SHA256，重新从原metadata生成引用，不相信receipt的成功布尔值。
新 `combined-license-inputs-01.json` 原命令 **exit0**：

- **1199个原始 License File 引用，714个解析到verified source bytes，485个未解析**。
- 485中484是尚未采集checkout tree内的路径，1是远程引用；**0个“本批已收完整tree内存在但原文漏采”的引用**，0个指向已索引非regular文件的引用。
- 原主仓库455个raw缺失引用有3个现已解决：BoringSSL根LICENSE、FreeType FTL.TXT、HarfBuzz COPYING。
- 新增外部metadata也新增引用，因此不能把485与原455直接解释为退步/实际缺失组件数。未判断shipping、平台或构建实际选中状态。
- 仍有261个parent Git source declarations未收，以及157个parent non-Git声明；递归源/平台条件/二进制组件覆盖仍`not_proven`。

combined receipt SHA256 `2115c029d3c460854adac59145c39935241a1458e6d38278bbb8ff0bd6f4731e`。
485项完整清单及各来源SHA留在任务`.run`目录；抽查含大量Android生成许可、Node non-Git与ANGLE子依赖。不会为了消掉原始数量去下载无关平台/private包，也不会擅自忽略shipping项。

## 回归/质量与产物所有权

显式测试共 **49 passed / 0 skipped**：tree13、DEPS9、external19、closure8。
外部新测试覆盖主引用非标准文件名、父路径/跨仓库、symlink不materialize、固定catalog/缓存、两轮真实格式Git batch采集，以及末轮坏blob的partial receipt保留；不能把这些fixture测试当实际browser/App/native验收。
八份本地Python文件syntax检查通过；10份本批工具/测试/新检查点的LF、末尾换行、零trailing whitespace通过。`git diff --check`与FINAL质量门禁本批真实 **exit0 / PASS**，App+Workbench13573、≥1000源码80未增长，不沿用上一批结果。
diff输出三份其它既有WIP的CRLF→LF提示（release.yml、.gitignore、tauri.linux.conf.json）；不是diff检查失败，本批没有顺手改写它们。没有改Rust/前端，不重复记旧Core/前端数字为本批通过。

本批未构建软件/下载新browser/启动测试App，无可退休compiled site、runtime执行副本或重复发行包；删除0、实际回收0。
01 bare cache仍被02使用，主源/两代源原文/首败/成功receipt有用，保持；其它不确定cache/owner暂缓，不停止无关进程。末次查询C可用383475920896、H207206461440 bytes；后续大构建前重查。
App144仍由原隔离home承载，最后CIM身份检查PID59356/路径/创建时间匹配；本批没有视觉验收、发送/批准任务或选择文件，不自动重放909字草稿。

## 下一批顺序

1. 依据固定官方parser/prune/special cases与OS规则梳理raw缺口，先处理真实公共运行组件（媒体/WebP/AV1等）和递归同pin源；条件、非Git、private/Android/开发项分别记录，不把raw计数当shipping门禁。
2. 补全默认Credits的同源输入，在独立受控候选中显式检查metadata解析/缺失，再执行审过的官方generator；保留warnings、scan/credits与coverage分别结果，不能只看exit0。
3. 完成源提供/LGPL/media与稳定安全更新决策；官方157四包仍401-byte sample Credits，完整通知红，不晋升。选定来源后继续[R2–R4](2026-10-13-computer-use-maintained-runtime-next.md)，不拿这份源数据替代生产prepare或最终四架构/安装/真人/12h验收。
