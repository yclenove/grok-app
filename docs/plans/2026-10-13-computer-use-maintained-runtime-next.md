# CU 维护版运行时：下一批落地顺序

最新[原生CI修复](2026-10-13-computer-use-ci-fixes-checkpoint.md)第七批已接worker端口前及时drain、有界诊断和原owners保留，Windows完整App2407项与Linux独立startup模块4项通过。`c8217d08`远端Mac、production Jammy、frontend与双架构SDK成功，原八项Windows managed启动已通过；继续定位Windows plugin auth超时、X11可访问性D-Bus崩溃，并接运行时CPU/header校验、五target维护合同/ARM64产品集成及原R1–R4完整验收。ARM64产品prepare仍失败，维护browser诊断成功不替代产品验收。用户App144与home、App11+/CU受管Mac14+保持；新启动修复待远端复验。

最新同步已完成：[fork / 源仓库同步检查点](2026-10-13-computer-use-upstream-sync-checkpoint.md)，
提交 `8107bb07` 已推送。当前处理 [Linux SDK 双架构检查点](2026-10-13-computer-use-linux-sdk-targets-checkpoint.md)：
明确 x64/Jammy 与 ARM64/Noble 的独立 SDK/ABI/loader 合同，两架构原生 CI
SDK job均已成功。后续[CI修复](2026-10-13-computer-use-ci-fixes-checkpoint.md)
处理Rust1.99 lint与Jammy输入夹具；ARM64浏览器资源仍开放。原 R1–R4、全部平台/surface/安装/真人/12h 范围保持；
下方 fetch-only、未提交和旧哈希属于历史批次，不作为当前下一步状态。

当前R1：[SDK / FFmpeg与同步预检](2026-10-13-computer-use-sdk-ffmpeg-source-checkpoint.md)。准确Linux SDK整包/四许可和FFmpeg声明已回读，SDK尚未接统一候选，四平台记录参数默认走FFmpeg静态库分支，源码提供与完整NOTICE仍开放；原候选07/08和18原错误保持。fork/源仓库已fetch、未合入，下一批先保存CU源码快照、处理冲突和回归，再继续原R1–R4全部范围。App11+/CU受管Mac14+、旧产品保持，以下为原各批证据。

R1最新[SwiftShader MD5/libbacktrace特殊通知](2026-10-13-computer-use-swiftshader-copied-checkpoint.md)：13准确源/88774 bytes/四manifest与raw batches已验；五MD5完整原许可和两backtrace inline permission绑定，独立01/02的10197-byte片段与整个报告一致，437工具tests/0skip通过。Boost按需下载pins、libbacktrace Gitlink及CMake debug分支已查清源码来源，未证明binary排除或完整NOTICE；原八目录错误/其它缺口保持。继续其余copied/generated来源/ANGLE义务、完整四平台NOTICE/SBOM/源码提供/LGPL-media/稳定安全策略，再最终维护pack及原R2–R4全部门禁；App11+/CU受管Mac14+与旧产品保持，整体active/partial。

R1最新[SwiftShader复制源码通知](2026-10-13-computer-use-swiftshader-notice-checkpoint.md)：全部28 GN/五C++样本、四manifest与raw batches已验；11原通知、完整NCSA/LLVM exception保持真实版权，独立01/02的91194-byte片段与整个inventory一致，427工具tests/0skip通过。八原目录错误未抹平，六partial绑定、Boost/libbacktrace与LLVM alternate/copied源仍未齐。下一步准确MD5等源/四原ANGLE义务及完整四平台NOTICE/SBOM/源提供/稳定安全策略，再最终维护pack与原R2–R4全部门禁；App11+/CU受管Mac14+和旧产品保持，整体active/partial。

R1最新[复制API通知与省略记录](2026-10-13-computer-use-notice-supplements-checkpoint.md)：38完整API头文件/原版权/四manifest已验，39880-byte通知补充仍partial/not for distribution；411工具tests/0skip通过。17省略记录10真实配置许可已绑定、3复制API通知已补但registry provenance未证、4 source/shipping义务未解决；196目录问题/原421 raw gaps保持。下一批真实copied-generated输入/四义务与完整四平台NOTICE/SBOM/源提供/稳定安全策略，再最终维护pins/四锁/production pack和原R2–R4全部门禁；App11+/CU受管Mac14+与旧产品保持。

R1最新[默认通知候选与完整 ANGLE GN 来源](2026-10-13-computer-use-notice-candidates-checkpoint.md)：495有效候选全文缺口1→0，WebGL准确原文/四manifest已验，独立02/03 partial preview为5453606 bytes；17省略记录/196目录问题/5异常值保持。全部162 ANGLE GN源1018314 bytes/七CPU字面引用与399工具tests通过，不证明binary排除或完整通知。下一步省略记录/目录/copied-generated源与完整四平台NOTICE/SBOM/源提供/安全策略，再最终维护pack及原R2–R4全部门禁；App11+/CU受管Mac14+与产品旧运行时保持。

补充[ANGLE CPU候选消费源](2026-10-13-computer-use-cpu-consumer-source-checkpoint.md)：两准确GN/C++源25913 bytes、四原manifest与独立offline复验通过，累计381工具tests/0skip；原DEPS此项是独立Android条件，两候选处无CPU引用，不据此排除完整通知或证明全部消费者。完整CPU图/其余copied-generated源/字段/四平台NOTICE仍开放，原R1–R4及App11+/受管Mac14+、产品旧运行时保持。

R1最新[Khronos 完整许可正文](2026-10-13-computer-use-khronos-notice-checkpoint.md)：固定SPDX两正文11358 bytes、七声明绑定/一完整内嵌permission和八原版权声明已复验，380工具tests/0skip通过。只是八文件通知片段，原7554paths/422 raw gaps/17 yes错/196目录错/5异常值保持。下一步其它copied/generated源、ANGLE CPU真实消费路径和字段问题，再完整四平台NOTICE/SBOM/源提供/LGPL-media/稳定安全策略。原R1–R4全部active/partial，App11+/受管Mac14+和产品旧运行时/pins/四锁/seed/App144/home未晋升。

R1最新[ANGLE / OpenScreen 集成](2026-10-13-computer-use-integration-checkpoint.md)：准确generated flag导出/helper AST、16源码规则与四原GN/custom vars对应，四条真实许可路径映射/Perfetto实际四manifest pin已验。最终04/05独立同SHA、357审查工具tests/0skip通过；7541paths/422 raw gaps/17 yes错、196目录错/5异常值均保留。下一步ANGLE cpu_features/LLVM/registries、OpenScreen libc++/LLVM及其它生成输入，再完整四平台NOTICE/SBOM/源提供/LGPL-media/稳定安全策略。R1仍红、原R2–R4全部active/partial；App11+/受管Mac14+、产品/pins/四锁/seed/App144/home未晋升。

R1最新[Crashpad 配置与 lss 来源](2026-10-13-computer-use-crashpad-checkpoint.md)：根 `.gn`/四actual GN参数、external lss/zlib分支与准确路径许可已验，独立重算和319审查工具tests/0skip通过。7518paths/422 raw gaps/17 yes错、190目录错/5异常值原样保留，原standalone缺项不排除。下一步ANGLE/OpenScreen准确配置/真实路径与其它生成输入，再完整四平台NOTICE/SBOM/源提供/LGPL-media/稳定安全策略。R1仍红与原R2–R4全部active/partial；App11+/受管Mac14+及产品/pins/四锁/seed/App144/home不晋升。

R1最新[固定生成依赖闭包](2026-10-13-computer-use-node-generated-checkpoint.md)：准确GCS对象/generation/SHA、原recipe AST布局和四实际checkout同-object段已验，7507paths/422 raw gaps/17 yes错；190目录错/5异常值保留、296工具tests/0skip通过。下一步Linux实际lss pin/header/license、Crashpad准确GN映射与ANGLE/OpenScreen/其它生成输入，再完整NOTICE/SBOM/源提供/LGPL-media/稳定安全策略。产品/pins/四锁/seed/App144/home未晋升，App11+/受管Mac14+与原R2–R4全部active/partial保持；删除0/实际回收0。

R1最新[四平台来源/生成输入](2026-10-13-computer-use-platform-coverage-checkpoint.md)：主树+63源与原manifest准确匹配59/62/56/56、冲突0，原439 raw gaps/19 yes错/190目录错/5异常值全保留。八准确Crashpad/node源已采，258工具tests/0skip通过；下一步校验固定GCS node_modules对象/hash/有界tar与实际版本许可证、Crashpad准确GN分支/lss来源，再完整NOTICE/SBOM、源提供/LGPL-media/稳定安全策略。R1仍红，原R2–R4全部门禁active/partial；App11+/受管Mac14+及产品/pins/四锁/seed/App144/home保持，删除0/实际回收0。

以[最新检查点](2026-10-13-computer-use-managed-browser-boundary-checkpoint.md)为事实依据；不重复旧975/963或把用户App144当最新源码构建。完整目标保持active，未授权本批提交/发布。

R1最新[实际recipe/helper/config](2026-10-13-computer-use-recipe-provenance-checkpoint.md)：原不可变recipe整包6214369 bytes/SHA已读，包内21ae helper与独立Git bytes一致，四原log布局和精确AST确认DEPS fallback机制；四config8465 bytes/Git blobs及32 bootstrap keys零差异。236工具tests/0skip通过；不是远端文件测量/条件求值或完整NOTICE。63源439 raw gaps/19 yes错/190目录错/5异常值保留，下一步实际平台来源/policy coverage、生成输入/完整通知/源提供/LGPL-media/稳定安全策略；原R2–R4与App11+/受管Mac14+、产品/pins/四锁/seed/App144保持，删除0/实际回收0。

R1最新[六个实际递归仓库](2026-10-13-computer-use-checked-recursive-checkpoint.md)：四平台manifest均准确匹配的六源已采，63源7273路径/439 raw gaps；rapidjson一项关闭，19 yes错/5异常值保留，OpenScreen buildtools三LLVM目录使目录错增至190，不据缺项或同名主源刷绿。206工具tests/0skip与独立重算通过，完整NOTICE仍红，产品/pins/四锁/seed/App144不晋升。下一批准确config/recipe、执行CIPD helper及DEPS fallback，再真实平台coverage/完整通知/源提供/LGPL-media/稳定安全策略；原R2–R4完整范围与App11+/受管Mac14+保持，删除0/实际回收0。

R1最新[实际checkout/固定GCS对象](2026-10-13-computer-use-checkout-object-checkpoint.md)：四完整manifest/bootstrap与有界stdout共12件/5116718 bytes已封存（Windows stdout仅4MiB前缀）；57来源逐项对齐，四manifest157/169/152/152仓库且身份差异0。ANGLE/OpenScreen每平台各3实际匹配，缺项不当NOT_SHIPPED。原四ZIP共988113804 bytes的SHA256/MD5/CRC32C与固定GCS generation匹配；Windows原创建时间晚upload结束24.062ms保留，非publisher build认证。192显式tests/0skip通过，原19 yes错/187目录错/5异常值不降，完整NOTICE/R1红，产品/pins/四锁/seed/App144不晋升。下一批固定recipe/config、实际CIPD helper/DEPS fallback与六个实际递归源，再完整通知/源提供/稳定安全策略；原R2–R4全部范围与App11+/受管Mac14+保持，删除0/实际回收0。

R1最新[四平台实际archive/GN参数](2026-10-13-computer-use-archive-build-checkpoint.md)：四完整GetBuild/1260152 bytes与16同build白名单日志/22343 bytes已封存，同commit/position、ANGLE/SwiftShader/V8/WebRTC pins及准确ZIP上传step/path SUCCESS均已验。四actual GN写入参数没有official/credits override，同源默认Git对象/hash确认sample机制；gclient仅两remoteexec custom vars实读，未证明checkout/有效递归继承或GCS generation/ZIP bytes关联。169显式tests通过，原57源/19 yes错/187目录错/5异常值保持，完整通知/R1红，不晋升产品/pins/四锁/seed/App144。下一批优先同build manifest/实际DEPS/recipe/config与GCS对象精确关联，再补完整NOTICE/源提供/稳定安全策略；原R2–R4全部范围active/partial，App11+/CU受管Mac14+不变，删除0/实际回收0。

R1最新[gclient机制/公开build入口](2026-10-13-computer-use-gclient-build-evidence-checkpoint.md)：4固定helper原文404852 bytes与准确AST/boolean示例已核验，source-only precedence不证明实际builder；OpenScreen25 Git/7 non-Git零不匹配，ANGLE02原核心数据不变。固定commit read-only CI查询4页361条已封存，定位四个archive IDs；Linux诊断明确同position ZIP上传URL，但大输出截断不冒充完整raw/四平台或generation关联。140显式tests通过，原57源/19 yes错误/187目录错/5异常值不降、R1仍红；下一步封存四个bounded GetBuild记录，按真实gclient/GN/recipe和GCS hash/generation验证通知覆盖。App11+/CU受管Mac14+、原R2–R4全部范围active/partial，产品/pins/四锁/seed/App144不晋升，本批删除0/实际回收0。

R1最新[浏览器公共源/ANGLE递归目录](2026-10-13-computer-use-browser-recursion-checkpoint.md)：再10固定源/876 paths，57源7237 paths/1249 refs/440 raw gaps；policy19 yes带错/187目录错/5异常值保留，原12错误关闭/新6递归错误暴露。ANGLE80 Git/441 non-Git声明准确对齐，但继承/实际builder与binary未证明；113显式tests/0skip通过，不作NOTICE通过或晋升产品。下一步优先准确gclient继承与OpenScreen/ANGLE公共递归输入、生成/平台/完整NOTICE/源提供/安全策略；App11+/受管Mac14+、原R2–R4全部范围active/partial保持，本批删除0/实际回收0。

R1最新[LLVM/Windows支持源](2026-10-13-computer-use-support-license-checkpoint.md)：再11固定公共源/16 paths，47源6361 paths/1201 raw refs/439 unresolved；policy25 yes带错/165目录错/4异常字段保留。14许可缺口关闭，99显式tests/0skip通过；三项ANGLE的Not Included单独分类但不当NOT_SHIPPED，parent literal与standalone条件不替代真实builder继承/checkout。继续公共浏览器源、生成/递归/平台与完整NOTICE/源提供/安全策略；R1仍红，产品/pins/四锁/seed/App144不晋升，原R2–R4全部范围保持。

R1最新[公共运行源/缺口分类](2026-10-13-computer-use-runtime-license-classification-checkpoint.md)：额外17固定公共源/28 paths已采，准确Linux条件不执行，36源合并6345 paths/1201 raw refs/459 unresolved；默认policy39 yes带错/154目录错/4异常字段全部保留。17许可缺口关闭，94显式tests/0skip通过；通配discovery的0tests不算验收。459按来源分为370当前树未索引、53未采父Git、16递归、19non-Git、1remote，不是binary计数。继续公共LLVM/ANGLE递归/Windows运行源与生成步骤/实际平台；完整通知/源提供/安全策略仍红，不晋升产品/pins/seed/App144，原全部R2–R4范围不变。

R2通用[有界归档/树合同](2026-10-13-computer-use-runtime-contract-checkpoint.md)已接：Rust/Node同一JSON额度，Chromium2048条目/1024文件/600MiB单件/1GiB树、ZIP分块CRC/TAR raw扩展头/尾部、tree/copy/repair/rollback/audit与失败保留已统一。Windows/Jammy06最终各Core1000/CLI3/strict、Node35与Linux冻结SHA通过，原owners均终态；FINAL仅质量通过。四现有源容量全适配，共享seed445不变；最终版本/布局/五链接/四锁/NOTICE与新完整prepare仍等R1，不晋升App144/pins/seed，R3–R4所有原范围保持。清两旧incremental逻辑575.3MiB/H可用观察增455.0MiB，当前依赖/候选/source/全部证据与home保持。

R1最新[默认policy/媒体来源](2026-10-13-computer-use-license-policy-media-checkpoint.md)：新增静态官方规则模型与媒体7 pins/28原文路径，75测试/质量通过；完整component seals与不可变payload补强。19源联合6317路径/1201 raw refs/477未解析；默认policy470 yes/287 no/4其它/56 yes带错/153目录错，都是metadata而非binary计数。原60→56四个媒体许可缺口关闭，不改pins/seed/App144；R1完整通知仍红。下一批优先正确分类/真实公共运行源，同时可并行通用R2归档合同，但最终pins/四锁晋升仍等来源/NOTICE，原R3–R4/完整范围不变。

R1后续[核心外部输入/引用闭包](2026-10-13-computer-use-external-license-inputs-checkpoint.md)：12个准确公共pins的456路径/440仓库内原文blob已收，FreeType非标准文件名/FFmpeg CREDITS按原引用补齐。合并重验6289路径，1199原始引用/714解析/485未解析（不是shipping计数），49测试通过；仍有261个parent Git声明、157 non-Git及递归/条件/二进制覆盖，通知红，不晋升。下一批按官方parser/prune/OS规则分类缺口，优先真实公共运行组件，不为计数清零拉private/无关Android源。

R1最新[固定源码/许可输入](2026-10-13-computer-use-pinned-license-source-checkpoint.md)：主树509552条、5833选定路径/5196原文blob已采集，273 Git pins全部对齐；不是完整checkout或SBOM。455原始缺失/远程引用不等于455个shipping组件。22测试通过，首败保留；下一步补公共核心外部源。正常Credits不强制GN output/target，默认扫描checkout；generator exit0可漏错，必须显式coverage检查，不晋升产品或减少原范围。

后续[Windows/Jammy内部原生诊断](2026-10-13-computer-use-maintained-native-checkpoint.md)：同来源官方157 + Node22/PW1.64，各28项通过；Linux首败缺DSO/28未进入保留，新02在私有root补依赖后通过。deb/rpm声明已补、Node9/Python9/guard14/质量通过；不是生产prepare、用户级sandbox/installed/最终候选或完整通知通过。只清已退休Win与Linux首败browser逻辑1.46GiB，保留当前Linux02/source/Node/PW/失败证据/App144/home。继续R1，R3最终四架构仍要重新验收，原范围与Mac14+/App11+不变。

R1最新[Linux155完整包与静态ABI](2026-10-13-computer-use-linux155-source-checkpoint.md)：673文件/797546240-byte树、8.28MiB Credits候选已实读；9个ELF所需GLIBC最高2.25只是静态证据，native仍not_run。联合来源工具39测试/质量通过；Linux文件数/格式合同仍待迁移，Mac版本不匹配，codec/源覆盖/安全未完成，不晋升。上轮[Windows155提案](2026-10-13-computer-use-maintained-browser-notices-checkpoint.md)保持，原官方157四快照仍sample/exit2。下一批继续R1后再进入R2。

同检查点后续已收齐两平台及共同补丁281个Git对象，实际确认Safe Browsing关闭、下载quarantine/MOTW关闭、FedCM默认关闭和不同SwiftShader配置；存在真实安全/体验差异。它们不是普通Chromium无差别替代，不因完整Credits或collector exit0晋升。源对象不是完整上游/二进制来源认证；原生兼容诊断可另建明确内部候选，不修改产品seed/用户包或把诊断当分发通过。

## R1 来源与完整通知（优先处理，不暂停其它原范围）

1. 保留现有四个同commit官方未品牌化快照/hash；实查 sample Credits不能用于发行。根BSD许可与第三方通知覆盖分别核验。
2. 按同 pinned source + DEPS 收齐官方默认 license generator 的输入、实际平台构建条件及所有随包组件（SwiftShader/ANGLE/ICU/媒体等），生成并审阅完整四平台通知/SBOM。正常Credits默认扫描checkout；GN依赖扫描是可选模式，不额外要求构建浏览器才能生成。显式检查metadata解析/缺失/跳过错误，不能只看credits exit0。不得用随便一个新版本 credits、另一平台列表或手写几个依赖名代替。完整通知与实际二进制输入必须可追溯；任何 source reproduction/coverage 缺口保持红色。
3. 同时核验是否存在维护中、许可与完整通知明确、四架构可追溯的较稳定未品牌化来源。CfT如使用，必须另行解决binary条款依据，仓库Apache不算二进制授权；不靠用户系统浏览器或EOL版本绕过。
4. 明确开发快照的更新/安全/稳定性策略后才选最终browser；157快照配PW1.64的自定义路径不是上游兼容保证。候选测试通过也不自动允许发行。

## R2 有界归档与锁迁移

- 通用部分见上述合同：旧200文件和prepare覆盖已移除，所有路径有相同固定上限；边界/超一项/目录/未选项/CRC/溢出/部分失败/旧有效目录与journal保留反例已补。仍须按最终选定来源复验真实格式、全部条目/树/链接/路径/压缩率，不能删源文件缩包。
- 选定来源后继续真实production prepare/check、repair/rollback和bundle全资源回读；现有四157容量清点及当前旧seed Core通过不替代最终维护候选或来源/完整通知门禁。
- 完整同步compiled pins、四source locks、pack-targets、worker声明、Mac五精确links、资源/NOTICE/hash/import/architecture合同；JSON edit不能绕compiled pin。App最低11不改，受管Mac14 floor保留。
- 在全新任务候选中prepare/check，不覆盖共享seed或用户App/home；source-only receipt不能跳过bundle before-build。

## R3 同一完整候选四架构原生门禁

- Node22/PW1.64 source-pair Windows/Jammy import是已验的上一候选；新完整候选仍须重验。Mac ARM与原生Intel需原生import/浏览器，不把cross-target materialization或Rosetta当Intel native。
- Mac11/12/13拒绝应无worker/browser/profile/授权副作用；14+正常启动，版本查询失败诚实拒绝，其他surface/普通聊天/App11安装不被全局阻断。检查实际UI说明与原授权流程。
- Windows/Linux全部typed action、截图/refs/下载上传、pause/stop/continue/导航/关闭、原PID所有权/迟到回执/取消/退出及immutable runtime副作用重验。固定独立页面+文件后置条件，不仅tool ok。
- 原Core/完整App suite、strict、TS/Node golden和15语言UI，失败及原owners收束证据保留，不跳过/assertion豁免。

## R4 包与完整交付的剩余门禁

1. 最终维护候选的Windows NSIS/portable，Mac两架构签名/公证与原生安装，Linux AppImage/deb/rpm依赖/私有loader/真实安装/portal、repair/update/rollback/uninstall按原D计划验。
2. 普通入口的真人原ACP/MCP/Grok/model与picker/收发文件/Pause/Continue/Stop/Quit；恢复原任务fixture时明确当前owner和监听地址。用户亲自发送/批准/选文件，不自动重放旧909字草稿。
3. 四surface/IME/clipboard/文件/恢复全部能力、UI UX与真实模型矩阵完成后冻结唯一最终candidate做12h长稳和D00–D31。不得借旧Core、旧App144或SDK局部结果拼绿。
4. 每批检查磁盘并只清已终态原owners的确切废弃产物；保留current build/依赖/seed/source/失败证据与用户数据。逻辑大小和可用空间观察分开报。
