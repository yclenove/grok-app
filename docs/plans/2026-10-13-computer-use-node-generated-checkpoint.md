# CU 维护浏览器：固定生成依赖许可证闭包

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[四平台来源/生成输入](2026-10-13-computer-use-platform-coverage-checkpoint.md)。
App macOS 11+ / CU 受管浏览器 macOS 14+、四平台/两 Mac 架构/四 surface 与全部原交付范围保持。
本批仅审查工具、公开来源数据、测试和文档；没有改变产品 pins、四锁、seed、App144/home，
没有提交、推送、提 PR、发布、操作 GUI、发模型任务、代批权限/选择用户文件或改代理。

## 1. 原 DEPS 的准确 GCS 生成对象

`collect-chromium-node-generated-licenses.py` 在网络前重新验证封存主树/DEPS catalog、
八件准确 Gitiles source/raw 字节与 SHA，再只读编译白名单的 metadata/archive URL。
固定 bucket `chromium-nodejs`，object `605b17899d6503df7f2ce96d5458976b36c5e456`，
generation `1791258586019361`，**11492257 bytes**，SHA256：
`f7d15a9ba38b018602496cde9b094c34c5876c75723d3a5497682fd2e71fcdf3`。
metadata 的准确 generation/size 和下载响应 generation header 都匹配原 DEPS；
streaming 整包 SHA256/size 通过才读 tar，禁止跳转、自动 Content-Encoding，单次 socket20秒/transfer60秒。
不走浮动对象，不安装 npm，不执行上游脚本，也不抓 README 所写的其它版本来凑许可证。

归档是**声明 checkout 目录内部的内容**，开头 `.` / `./keyv`，没有 `node_modules/` 外层。
实际 recipe 打包的 `gclient.py` SHA
`ac100091d3d3c02993cbcad11d13cd1e25ae8adb4346746bf85886dd96094913`
有精确 AST 证据：`GcsDependency.output_dir` 取根目录 + 声明 name，
`_MaybeExtractArtifact` 中 `tar.extractall(path=self.output_dir, filter=TarFilter)`。
本地工具按该布局把 archive-relative path 映射为 source-relative path；不运行 helper、不声称测量远端文件。

原 `01` archive 已校验成功，但工具错误要求带外层目录，检查 exit1。
修正前有界读取原包前八项仅用于诊断，未到 footer，不算完整归档通过。
`02` 复用原包后完整 tar 检查成功，但工具错误要求 lockfile v3；原封存 source 实为 v2，exit1 保留。
修正为**准确 v2** 后，`03` 实际 exit0 / `collectionComplete:true`；不删除断言、跳过 bad tar 或覆盖原输出。
只下载一次原归档；02/03 都重新校验 01 的原 metadata/size/SHA，不重复下载或复制大包。

## 2. 有界归档与实际许可证

新 `bounded_tar_gzip.py` 用有界 zlib 输出检查 single-member gzip CRC/ISIZE、截断、拼接和尾部，
共享 `bounded_tar_xz.py` 的 raw tar checksum/长名/末尾/padding/entry/file 验证。
原 XZ 默认仍拒绝 links；Gzip 可记录链接，不跟随、不提取，路径/target/root/重复/case collision/
父目录类型冲突另有明确检查。PAX/sparse/special 仍拒绝；不使用无界 tarfile 解压。
Gzip decoded 总限256MiB、50000 raw entries、16MiB 单文件；选定原文单件1MiB、总32MiB。

实际完整归档：**6544 entries = 1198 directories + 5346 regular files**，无链接；
全部 regular file payload 共 **58717571 bytes**，每件都 streaming SHA256 并读尽。
选定 **234 paths / 641781 bytes**：许可证原文、全部主 metadata 指向的17件许可证，以及两包版本 JSON。
只写 SHA 命名 `.source-data`，没有物化 `node_modules`，没有解析或执行包中 JS。
所有请求的原 license path 均已找到；同原文 hash 可共用数据 blob，但每个来源路径独立保留。

| 包 | 归档版本 | 原锁版本 | README.chromium | 实际许可 |
| --- | --- | --- | --- | --- |
| Lit | 3.3.0 | 3.3.0 | 3.2.1 | BSD-3-Clause，Google 2017 |
| @azure/msal-browser | 4.2.1 | 4.2.1 | 4.2.1 | MIT，Microsoft |

Lit 的 README/lock/archive 差异保留，不改上游 metadata 或宣称三个都匹配。
`node-generated-license-03/node-generated-inventory.json` SHA256：
`96783deaa138a896b0d24d0fbc86d67834f6b85f4ff1775543616843d9640576`。
原01/02的失败 receipt、原包和 raw 均保留，03是后续独立审查的准确 seal。

## 3. 原四个实际构建与闭包重验

`inspect-chromium-node-generated-coverage.py` 重新读取原 archive/build/bootstrap/log seals、
63个公共/递归来源、生成 source receipt和原包；再完整读 tar，要求所有 entries/selected paths/
hash-named source bytes/version comparison 精确重现原03。
独立重验不创建缺失 blob；缺 bytes 直接拒绝，不把验证变成隐式补采。

四原 checkout stdout 都有准确同-object header与 Started/Finished 段：
Windows line30093、Linux1600、Mac Intel1746、Mac ARM1923。
Windows仍只有原4MiB前缀，完整段在前缀内，但整份 stdout **不**改称完整。
这增强 actual checkout 输入对应关系，不是测量远端解压 bytes 或 publisher/binary 认证。

生成索引用独立 `generated_archive_input` / `archive_regular_file` kind；
没有伪造 Git mode、gitObjectSha1 或将 non-Git data 当 Git 树。
默认 policy 用虚拟 regular source view读取准确 archive bytes，保留大小写和其它原错误。
source reference闭包同样重新计算，原四份历史 license receipts 不改：

- 已采 paths：7273 → **7507**；raw引用仍1250，resolved828 / unresolved**422**。
- **17**原始许可证引用实际闭合，均在 third_party/node/README.chromium；不是静默排除。
- yes metadata errors：19 → **17**，仅 Lit/MSAL 无原文的两错误实际关闭。
- directory errors **190** / nonstandard Shipped values **5** 保留。
- 完整 NOTICE/SBOM、其它源义务与 R1 仍红；`licenseCoverage:not_proven` / `promoted:false`。

原 `node-generated-coverage-01.json` 与独立重算 `02` SHA一致：
`0f9b45ff474988987b092b198f05e0bcff43153511f64af40117d5fd31bbe124`。
本批source通过不能晋升维护 runtime 或拿来替代产品/native/model/包验收。

## 4. 测试、质量与清理

显式 runner 原258项加 raw-XZ8/Gzip6/generated-node15/coverage9，实际**296 passed / 0 failed / 0 skipped**。
XZ默认links边界保持；新增损坏footer/CRC、条目/单件/总量、未消费entry、selected-link、root escape/
同名大小写/父类型冲突、已有source拒绝覆盖、原cache hash/迟败保留、version差异、生成kind/原log反例。
只执行受信本地工具测试，不执行 collected upstream bytes。
最终55 Python AST / 61严格格式 / 68 LF-EOF-hash、diff与实际质量门禁通过。
收尾 `node-generated-final-input-hashes-01.json` / `quality-node-generated-final-01.json` 是本批质量证据；
App+Workbench13573 / ≥1000源码80不得增长，原 R2 22 hashes 和四历史许可 receipts 不变。

原 collector/test/inspector owners 终态后检查本批清理：没有新 compiled site、运行时执行副本或重复大包。
删除0 / logical0 / actual reclaimed0；01唯一来源包、02失败证据/选定blobs、03 seal/source、
SDK/bare/依赖/seeds/current App/home及全部历史成功失败证据保留。
live user preview/不确定owner cache暂缓；空间观察不冒充实际回收量。

## 5. 下一批

1. 原平台清单表明 `third_party/lss` 的准确 pin
   `29164a80da4d41134950d76d55199ea33fbb9613` 仅 Linux manifest有exact entry；
   继续收准确公共源/license/header，再证明 Crashpad Chromium配置的分支和external mapping。
   不从Mac/Windows未出现推导免义务，也不套Crashpad standalone pin或造symlink。
2. ANGLE/OpenScreen standalone缺路径、生成/其它runtime输入和明确metadata异常继续如实核对，
   再完成四平台NOTICE/SBOM、源提供/LGPL-media与稳定安全更新策略。
3. 原[R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)全部继续：最终pins/四锁/
   production prepare/check/repair/rollback/bundle、四架构原生、三格式安装/签名/更新/卸载、
   四surface/UI UX、真人原ACP/MCP/Grok/model/picker、同最终candidate12h与D00–D31。
   active / partial，不将审查工具数量或metadata闭包当完整交付。
