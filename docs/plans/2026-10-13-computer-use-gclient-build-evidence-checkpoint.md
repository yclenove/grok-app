# CU 维护浏览器：gclient 继承机制与公开构建入口

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[浏览器来源/ANGLE目录](2026-10-13-computer-use-browser-recursion-checkpoint.md)。
上一批是实际工具接线/源采集/复验进展，不重复收集或冒充本批 Core/native/UI 验收。
本批继续 R1，App11+/CU受管Mac14+、四目标/所有surface/完整D计划不缩减。
产品版本/pins/四锁/seed/App144/home不晋升，无提交/推送/PR/发布/GUI/新模型任务/
自动授权/用户文件选择，不使用被取消任务的凭据，不改代理或运行用户账户操作。

## 1. 同 parent 的 gclient 实现和上游测试原文

新 `collect-chromium-gclient-sources.py` 从原 sealed depot_tools tree/bare 收集4个
准确 Git 对象：`gclient.py`、`gclient_eval.py`、`tests/gclient_test.py`、
`tests/gclient_eval_unittest.py`。源码与上游测试全部只作 inert `.source-data`，不运行。
目录 `gclient-source-01`，原工具实际 **exit0**，4 files / **404852 bytes**。
revision `21ae2d24f8ee0ca3459bda416129d7791c4313f1`，原完整 component seal、
full-tree hash、四path/mode/Git blob、bare/revision/root-tree全部复核，不取latest。
root SHA256 `7875895d63dc45e10361ca09506a9db07a9b25896509a3be8791dcdb005a61e3`。

新 `inspect-chromium-gclient-inheritance.py` 对同 sealed 原文作 AST 形状校验，
不是只搜注释或执行上游函数。实际 `gclient-inheritance-01.json` **exit0**，
`sourceShapesVerified:true`，SHA256：
`e76438ff8df393952ebf36edc7fa07586358e20a38bd3af57032053299aa0e7c`。

准确源码证据（行号和原文片段均在receipt中）：

- `Dependency.get_vars` 1924–1940：child DEPS vars → parent effective vars →
  builtin vars → custom vars，最后者优先；整个函数 AST 匹配。
- `merge_vars` 1954–1965：值覆盖与 ConstantString 类型保留，整个函数 AST 匹配。
- `ParseDepsFile` 1082–1089：父有效变量覆盖子仓库已声明变量；原 Parse 调用实传
  `self.get_vars()` / `self.get_builtin_vars()`，不猜调用参数。
- `gclient_eval.Exec` 545–560：先声明变量、再 builtin、再已声明键的 override，
  不通过 override 引入未声明新变量。

三个本地 boolean 示例只说明准确顺序（child False/parent True→True；custom False→False；
builtin False/custom True→True），不是实际DEPS条件执行或平台checkout。
`actualBuilderInheritanceVerified:false`、`builderCustomVarsKnown:false`、
`licenseErrorsExcluded:false`，不以源码机制证明实际builder用了它或直接排除许可。

## 2. OpenScreen 准确递归目录与 ANGLE 回归

递归 inspector 增加仅内置的 `--component openscreen`，不同 source group/revision/
DEPS blob/component seal/prefix/输出名分别精确绑定；未知组件在解析或读源前拒绝。
原 ANGLE 默认保留旧字段，新通用 component 字段不混淆两个仓库。

全新 `recursive-deps-openscreen-01.json` 实际 **exit0**：
**25 Git /7 non-Git /0 unresolved /0 mismatch /0 uncatalogued Gitlinks**，
SHA256 `c80c95ff85dbc6e091f806038df1bd2ed91ad67c46e9e1e6c4c3619457050608`。
DEPS blob `22bc9c6438d34f9ea6b97158bca166eb4b4279cf`，parent 明确recurses，
parent literal True /OpenScreen standalone False；源注释也明确父变量覆盖。
22项 Git 声明带准确 `not build_with_chromium`，另3项无此条件。
不将 standalone 的不同 LLVM/quiche/googleurl/perfetto pin 别名成 Chromium root 版本，
不据注释/默认值跳过25项或排除19个当前 yes metadata 错误。

全新 `recursive-deps-angle-02.json` 实际 **exit0**：仍80 Git/441 non-Git/零不匹配。
对原01逐项比较 Git/non-Git/unresolved/mismatch/uncatalogued/defaults/parentRecursedeps
完全一致；原证据不覆盖。新 SHA256：
`eca26cd95153d528f8fd4df2275b208c936a2490a75edb9b09aeb27833c2ec00`。
继承实际值、builder custom vars、source fetch、conditions/shipping全部未证明/不执行。

## 3. sample Credits 的源码分支与真实 CI 查询入口

实读原 sealed `components/resources/BUILD.gn`，Git blob
`fd2fb05219e396333661c94319545aa546191fb6`，SHA256
`1d06064122da27e753b0da6e3fce990ba6cc13f179fbf19a309420fe657cafb9`。
准确默认：`generate_about_credits = is_official_build`。
开启时运行 `licenses.py credits --target-os=$target_os ...`，关闭时复制 `sample_credits.html`。
这与四快照的401-byte sample结果一致，解释了可导致该结果的构建机制；
**并未由此确认四个平台的实际 args.gn 或证明通用生成器/解压器失败**。
原完整NOTICE门禁依旧红，不把示例页当完整许可或用错版本/平台通知代替。

对固定 commit 公开 Buildbucket 查询首次 diagnostic FieldMask 请求实际 HTTP400，
去掉错误 mask 后正常；失败不改成“服务不可用”或删除证据。
新 `collect-chromium-public-builds.py` 只允许该固定 commit 的 read-only SearchBuilds RPC：
100件/页、4MiB/页、最多8页、20s网络超时、精确XSSI/JSON/项目/bucket/ID、
去重/循环分页/partial失败保存，原body不能调度或取消build，不用登录凭据。

`public-build-search-01` 原工具实际 **exit0**：4页（100/100/100/61）、
**361条**、194096原response bytes，搜索穷尽；四raw page/hash和完整inventory保留。
root SHA256 `a3473758611aa0ec24a75d7d5017421e29f17583ea21d0fb4ec6a1a90d322e9b`。
原361条可用 input Gitiles host/project/commit 另实核均准确匹配，0 mismatch；
source-aware validator与预请求redirect拒绝已补，不能仅信query tag或请求后才检查跳转。
这是固定commit-tag公开CI集合，不证明每项实际publisher或四个ZIP generation。

已找到四准确 archive builders（都是 SUCCESS，但不将成功状态代替完整交付）：

| 平台 | builder | build ID |
| --- | --- | --- |
| Windows x64 | win-archive-rel | 8668153607039536273 |
| Linux x64 | linux-archive-rel | 8668152676980535313 |
| macOS Intel | mac-archive-rel | 8668153412503290689 |
| macOS ARM | mac-arm64-archive-rel | 8668153878055557601 |

另直接只读 GetBuild 诊断确认：默认 mask 不返回 output/steps，不是它们不存在；
准确 `fields=id,builder,status,input,output,steps,tags` 可读实际数据。
Linux记录包含同commit/position和准确 `gsutil_urls`：
`gs://chromium-browser-snapshots/Linux_x64/1716663/chrome-linux.zip`，
以及准确ANGLE/SwiftShader/V8/WebRTC revisions，实际上传step SUCCESS。
这一诊断输出很大被截断，**不冒充完整raw记录已封存或四平台证据已齐**。
下一批按上表封存4条 bounded GetBuild raw与筛选摘要，核验publisher路径/实际gclient/GN
参数及recipe/config，不仅看SUCCESS或源码默认值。必要日志只读取明确同build的允许路径，
不跟随任意输入URL、不执行日志中的命令。GCS generation/ZIP hashes与真实build关联仍开放。

## 4. 验证、格式、资源与后续

显式 runner 最终原日志 `license-tests-inheritance-final-02.log`：
**140 passed /0 failed /0 skipped**，tree13/DEPS11/external31/closure15/policy20/
classifier15/recursive12/gclient-source5/inheritance8/public-builds10。
初轮138和补source-aware的139原日志分别保留，不覆盖或重复计入最终测试数量。
含静态机制/优先级改变拒绝、动态源码不执行、显式boolean示例边界、准确组件/跨名拒绝，
read-only fixed-commit query、bounded页/opaque token、原response保留与晚败/循环分页partial。
不是执行上游测试、真实gclient sync、完整App/native/model验收。

原联合 closure05/policy05/classification03 不重生成/不降错误计数：仍57源/440 raw gaps、
19 yes带错/187目录错/5异常值；新helper/CI/递归catalog不合并为“新增shipping组件”。
R2原22源码SHA全部复核不变。最终本批语法、owned LF/EOF、工具/新检查点零尾空白、
hash/diff/代码质量门禁另存，已有Markdown hardbreak保留；FINAL仅质量，不是完整CU完成。
最终实际exit0：21份工具AST语法/22份严格零尾空白（含新检查点）、28份LF/EOF/hash、
diff与质量门禁。App+Workbench13573/≥1000源码80不增长，其它3件WIP的CRLF提示保留，
不顺手重写。最终hash/quality各另存02/01证据，不覆盖旧首轮或把本批成功写成完整交付。
无新build/执行browser副本/重复包可退休，删除0/实际回收0；source/CI/bare/当前候选/
依赖/seed/home/旧首败证据保留，不确定cache暂缓，不停止任何无关owner。

先走上述真实4个archive记录与builder参数路径，再补准确公共运行输入/平台生成/
完整NOTICE/源提供/LGPL-media/稳定安全策略；不为raw count清零拉441个非Git或跳元数据错误。
R2最终维护来源/格式/布局/五Mac links/四锁/全新production prepare-check/repair-rollback-bundle，
R3四架构原生（Mac ARM和原生Intel），R4所有four surfaces/UI UX/安装签名更新修复回滚卸载/
真人原ACP-MCP/model/picker/同最终候选12h/D00–D31全部保持，
见[维护版计划](2026-10-13-computer-use-maintained-runtime-next.md)。
