# CU 维护浏览器：默认许可规则与媒体来源

2026-10-13；`feat/computer-use-implementation`；active / partial。
上一批是实际progress：12固定外部源/引用闭包已采集和复验；本批新增静态policy模型、媒体7源和输入完整性修复，不重述旧测试冒充开发。
本批无产品 pins/四锁/seed/App144晋升，无commit/push/PR/release，无GUI/代理/模型任务/自动批准/文件选择。App11+/受管Mac14+和全部原交付范围不变。

## 固定官方规则，未执行源码

完整读取固定157 `tools/licenses/licenses.py` 后，新的 `inspect-chromium-license-policy.py` 只用受限AST取16个policy字段和固定PRUNE update，不exec/eval/import上游源码、DEPS或hooks。
源SHA256 `b312c62523e955b1893923e8b45c13092eb58a6df22504c4e045205bf1904063`；源码hash变更直接拒绝。
模型按full Git索引虚拟目录发现、PRUNE/附加路径JSON、Rust版本目录、SPECIAL_CASES、依赖分隔符、字段别名/默认Shipped及COPYING fallback处理。源内容保持OID data，不materialize源码路径或执行完整generator。

不是“每个README都必须发行”，也不是“所有raw引用都已核验”。同一checkout的桌面win/mac/linux在该CLI没有额外OS过滤（只对iOS设置默认exclude）；实际平台checkout/构建输入仍不同且未解析。
模型明确比上游的exists检查严格：许可证必须是已复验的regular bytes；source symlink/目录/有tree却无原文不能满足。**模型不是官方generator运行或真实binary SBOM**，`licenseCoverage:not_proven`。

第一轮unit有3个真实errors：本机Python **3.8.6**没有`ast.unparse`。未进入policy扫描、未产生报告。已用只识别Name/Attribute的静态dotted-name函数修复，动态factory/import调用不解析、不执行；原失败保留在工具流，不冒充本地raw log或关断言。

## 首轮policy原结果

原 session71387 exit0，`license-policy-01.json`：855候选目录、759 metadata entries、141目录/格式错误；468个精确`Shipped: yes`、287个`no`、4个其它值；60个yes entries有许可/mandatory错误。
4项原值分别为Kotlin stdlib `Yes`、rapidhash `Yes`、WebGL conformance `No`、Windows App SDK headers `Yes`。上游使用区分大小写比较；没有擅自归一化或把`No`静默丢掉。
root SHA256 `e52a963e23afb0e36943e27045cb3893af21c902be00ae147fa637f4bd165aca`。
这些是默认扫描的metadata计数，不是实际shipping二进制组件计数；未采Git checkout、Android生成许可、测试目录、平台/SDK等错误都留在完整报告中。

## 七个额外公共媒体pins

扩展collector为两个独立、编译内置的`core`/`media`组，任意dict/错误组或组件在path/network前拒绝；新组仍逐件对齐同一固定parent catalog/gitlink，不接受latest/镜像/凭据URL。
`external-license-03` 原 session66295 **exit0**：

- dav1d `99f3354ed0b9b4bf3c7c9347318dbcc522c94af5`。
- libaom `8fbd65b2f354e1d7059f8b1899f3f0373d1b3e2a`。
- libjpeg-turbo `640f254ad0fa03f6b1f29f89b7dd9366f2f6e533`。
- libvpx `0a6f769e44989222d99ded46daa1e9763637cc19`。
- libwebm `6184f4484a826724b5293837134ab9492261b941`。
- libwebp `b43b2caa710c0c997c066cb32c7fea1391fad70a`。
- libyuv `eeb06a6451ba2e21f8ff272c7b42fc62b9faecc5`。

**28 selected paths / 28仓库内去重blob合计 / 87814 bytes**，主引用/追加路径0缺失；包括libaom/libvpx/libwebm/libwebp实际PATENTS/许可文本，不因文件名非LICENSE漏采。
原root receipt SHA256 `b4fc80ba79996402e0b185077fa0fa615156492b230fe408fd8fd18c9a2186aa`，准确repo URL/对象及collector快照均保留。只是选定原文齐，不证明全源/实际decoder启用或发行义务已履行。

## 输入完整性修复与新联合复验

以前root receipt的hash不能单独防止改子清单删selected paths；现在对原terminal collector的 **19份完整component receipts加独立固定SHA256 seals**，保护“列了哪些输入”，再核验full tree与每件Git/sha256。
共享loader只允许固定02/03与显式bool选媒体。已验证payload保留为不可变bytes，policy直接用该snapshot，不再第二次读disk造成验证/解析之间变化。

原 session77533 exit0，按新source group/seals生成全新报告，不覆写01：

- `combined-license-inputs-02.json`：**6317 selected paths / 按path 12410191 bytes / 1201 raw refs / 724已解析 / 477未解析**；477含476未采tree路径与1远程引用，没有本批已索引regular文件却漏原文的引用。
- 原主455 raw缺失现解决11项；19个parent Git源已收，剩余254个Git声明/157 non-Git及递归/条件/二进制覆盖仍开放。
- `license-policy-02.json`：**869候选目录 / 761 entries / 153目录错误 / 470 yes / 287 no / 4其它 / 56 yes entries带错误**。
- libaom、libvpx、libwebm、libwebp四个metadata缺许可关闭（60→56）；dav1d/JPEG/YUV许可也按固定输入核验。新来源暴露更多真实嵌套目录，所以目录错误141→153，不静默剪树或将其刷绿。

combined SHA256 `b0d34a0d51b138ea692c5c76800f5c82c2b4b76a2cc39c49725cb132a437726b`；policy SHA256 `9e9f5734e0fde7ffef6aa58634de178052cf01d1a706e471688cbcbeef947a3d`。
注意“470 yes”既不是470个binary依赖，也不证明四架构采用同样组件。`sourceFunctionsExecuted:false`、`generatorExecuted:false`、`checkoutCoverage:partial`、`promoted:false`保持。

## 本批最终回归与产物

显式 **75 passed / 0skip**：tree13、DEPS9、external22、closure11、policy20。含Python3.8静态名字、无动态执行、真实分隔/别名、COPYING/部分缺失、未知Shipped、symlink拒绝、附加路径循环/越界、media组/PATENTS、完整seals与原bytes快照。
十份本地Pythonsyntax、11份工具/测试/新检查点的LF/末尾换行/零trailing whitespace、`git diff --check`与FINAL质量本批真实exit0/PASS，App+Workbench13573、≥1000源码80未增长。三份其它WIP的CRLF提示保留，不改写；不是产品/浏览器/App/native/model验收，不复记前批Core/前端。

没有新构建/浏览器执行副本/重复发行包可清，删除0/实际回收0；新bare/source、旧首败/成功报告/源快照都是后续输入与证据，保留。旧不确定cache/owner暂缓，无无关process被停止。
最后CIM只见原用户App144 PID59356、路径和创建时间UTC2026-10-12T15:43:07.976117匹配；没有视觉验收/新包替换/代发批准。C可用383491698688、H207149780992 bytes；下一批大构建前重查。

## 下一批不能缩小范围

1. 对56个yes错误、153目录错误、4个非标准Shipped及477 raw引用按准确来源/生成步骤/平台条件审查；优先QUICHE、GPU/系统运行库等公共项，保留测试/Android/SDK/private边界，不要求下载全部无关平台，也不擅自忽略可能shipping项。
2. 核验官方binary实际build输入、全同源通知/源提供/LGPL媒体和稳定安全更新策略；静态policy及collector exit0不能晋升。目前官方157四包仍401-byte sample Credits，官方generator尚未跑，R1红。
3. 可在不选择/晋升最终来源的前提下并行推进R2**通用有界归档/配额合同**；必须同步prepare/store/repair/rollback/manifest/bundle audit，保留所有异常与旧有效目录，不只改一个数。最终pins/四锁/seed仍等来源/NOTICE决策。
4. R2最终候选与[R3–R4](2026-10-13-computer-use-maintained-runtime-next.md)之后仍须四架构native、全部surface/UI UX/安装签名更新卸载/真人模型ACP-MCP/picker与同最终候选12h/D00–D31。目标继续active / partial。
