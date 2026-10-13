# CU 维护浏览器：固定源码与许可输入检查点

2026-10-13；`feat/computer-use-implementation`；active / partial。
只改 data-only 审查工具和文档；不执行上游代码，不改产品 pins、四锁、seed 或用户 App144，不提交/发布。
受管浏览器 macOS14+、App11+边界与完整交付范围保持。

## 对上一批 GN 要求的纠正

实际读取 Chromium157 commit `f52796fc20c6e7d618374bb8bf096e7b2155e14c` 的 `components/resources/BUILD.gn` 和 `tools/licenses/licenses.py` 后确认：

- `generate_about_credits = is_official_build`；未启用时复制 sample。
- 正常 Credits action 使用 `credits --target-os=$target_os --depfile ... output`，没有 `--gn-target` / `--gn-out-dir`。
- 默认模式扫描 checkout 的第三方 metadata；GN 是另一种精确依赖模式，不是生成完整浏览器 Credits 的必需前置条件。
- `_DiscoverMetadatas` 可记录/跳过错误，`credits` 仍返回0；只看 generator exit0不能证明完整覆盖。后续必须显式启用警告/扫描检查并保存缺失清单。

因此修正上一批“必须实际 GN output/target”的过强表述；仍必须核验同源 DEPS、外部源、平台条件、depot_tools allowlist、special cases / extra paths 与随包二进制覆盖，不能拼几个 LICENSE 冒充完整通知。本批尚未执行 generator。

## 主仓库固定树与原文

任务目录 `tools/computer-use-probe/.run/maintained-notice-source-20261013-01`：

- 官方固定 commit 的 bare partial Git 仓库，无 checkout；原 fetch 终态 exit0。
- 根 tree `6d3d6ca9ea18a088adc33dfcdddab14fc3ac361d`。
- 完整主仓库 NUL 索引 **509552条 / 66848162 bytes**，SHA256 `ace014ca44c2c2b0c3946c759211b8ba1301b11f4141e59e62a794d2f36603b8`。
- 六份小型官方源文件按 `.source-data` 保存、hash 核验；不执行上游 Python / DEPS / hooks。
- 官方157 full/lite source tar及hash的四个 URL 均404；没有下载2GB源归档。固定 Git 树路线已实际可用。

首轮 `license-tree-01` / 原 session71882 exit1，5157个 blob 已保留；真实非shipping Ninja metadata 的 `License File` 为 HTTPS URL。原解析器误当本机路径拒绝。
已改为保存合法远程引用为 data、`sourcePath:null`，不访问该URL或把它当许可原文；盘符、反斜线、越界仍拒绝。原失败 receipt 和 `collector-01.py.source-data` 保留，不覆写01。

新 `license-tree-02` / 原 session77309 exit0：

- **5833 selected paths / 5196 unique blobs / 7430772 bytes**；每件验证真实 Git blob SHA1和SHA256。
- **1101原始 License File 引用，455缺失/远程引用**；其中含非shipping/Android/optional项，不能称455个实际随包缺失组件。
- receipt SHA256 `73f557ce384aa699d6748c0f72a8ca3a91f851360922210fc79c35cb4d745a51`。
- `collectionComplete:true`仅指选定主仓库原文采集；外部源未齐、`licenseCoverage:not_proven`、`promoted:false`。

## DEPS/gitlinks 对齐

`deps-pin-inventory-04.json` SHA256 `6d0e36265008ac4e5d4dd0c3569b5421a14fc16d3b82c10af7cd3608a356ed4e`：
**273 Git declarations / 157 non-Git declarations / 0 unresolved / 0 mismatches / 0 uncatalogued gitlinks**。
通过受限 AST 读取常量、Var/Str、字符串加法及简单 URL 占位符，不 eval/exec；条件保留原文、不猜选中。129个 Git 声明无条件，剩余含平台/private/internal项。
固定 URL/revision 已核对，仍不是实际 checkout、递归来源或二进制 SBOM。早期01/02 unresolved结果保留，未覆写成通过。

## 本批回归与所有权

`test-chromium-license-tree.py` **13 passed**，`test-chromium-deps-pins.py` **9 passed**，均0skip；在续行时实际重验。
data-only工具拒绝覆盖证据和 linked/reparse parents；tree128MiB、blob4MiB、total128MiB、selected12000预算，无 source-path materialization。
本批没有新增构建/运行browser副本可清；bare/source/首败/成功receipt有用，保持。删除0，实际回收0；不删seed/home/cache。
续行 CIM 实查唯一App144 PID59356、EXE路径与创建时间UTC2026-10-12T15:43:07匹配；没有 GUI/代发/批准/选文件/更新或退出预览。查询时C约383.5GB/H约207.2GB可用，后续大下载前重查。

## 下一步

先按最终catalog的准确公共固定pins补 ANGLE/BoringSSL/FFmpeg/FreeType/HarfBuzz/ICU/PDFium/Skia/SwiftShader/WebRTC/V8 metadata/许可原文与递归DEPS，逐件记录跨仓库/非Git/平台缺口，不自动下载private项。
再补默认Credits输入及显式coverage检查；同时处理源提供/LGPL/media、安全/稳定更新策略。官方157四包仍只有401-byte sample Credits，不因主树/采集器exit0晋升。
之后继续[R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)：生产归档/四锁/prepare-check-repair-rollback、同最终候选四架构native、安装签名/更新卸载、四surface/UI UX/真人模型ACP-MCP/picker与12h/D00–D31全部开放。
