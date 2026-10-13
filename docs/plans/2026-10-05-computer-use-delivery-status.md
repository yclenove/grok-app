# Computer Use 当前可用性与交付顺序

最新[原生CI修复](2026-10-13-computer-use-ci-fixes-checkpoint.md)第七批：两边main已合入，最新fetch无新增。`c8217d08`远端Mac、production Jammy、frontend与双架构SDK通过；Windows plugin auth超时、X11可访问性D-Bus崩溃、ARM64产品target仍开放。worker启动输出/有界诊断/原owners修复已在Windows完整App2407项与Linux独立startup模块4项通过，待新远端验证。产品pins/seed/App144保持，完整R1–R4仍active/partial，App11+/CU受管Mac14+不变。

当前进度：fork/源仓库已合入并推送 `8107bb07`，完整本地回归通过，见[同步检查点](2026-10-13-computer-use-upstream-sync-checkpoint.md)。当前推进 [Linux SDK 双架构](2026-10-13-computer-use-linux-sdk-targets-checkpoint.md)，保留 x64/Jammy 并接 ARM64/Noble 的 SDK/ABI/loader 与原生 CI；ARM64 浏览器资源仍开放。完整 NOTICE/媒体/源提供、维护运行时晋升及原 R1–R4 全范围仍 active/partial。App144 真人任务仍未发送，App11+/CU受管Mac14+及原用户home保持；以下为历史批次。

2026-10-13 最新[复制API通知与省略记录](2026-10-13-computer-use-notice-supplements-checkpoint.md)：38完整头文件/四manifest/原版权块已验，39880-byte partial补充绑定全部实际声明与完整许可；17省略记录分为10配置真实许可、3复制API/registry provenance未证、4 source/shipping义务未解决。411工具tests/0skip通过不作完整通知或产品验收；196目录问题/copied-generated/源提供/安全策略与原R1–R4全部门禁仍开放。App11+/CU受管Mac14+和产品旧runtime/pins/四锁/seed/App144/home保持。

2026-10-13 最新[默认通知候选与完整 ANGLE GN 来源](2026-10-13-computer-use-notice-candidates-checkpoint.md)：495有效候选全文缺口1→0，WebGL准确源/四manifest已验；独立预览5453606 bytes只作partial source evidence。17省略记录/196目录问题/5异常值仍开放。完整162 ANGLE GN源/七CPU字面引用和399工具tests通过，不作二进制排除或用户包验收。下一步审阅省略/目录/copied-generated源，完成完整四平台NOTICE/SBOM/源提供/安全策略，再最终维护pack和原R2–R4；App11+/CU受管Mac14+、产品旧运行时及完整范围active/partial保持。

补充[ANGLE CPU候选消费源](2026-10-13-computer-use-cpu-consumer-source-checkpoint.md)：两准确源25913 bytes/四manifest/独立offline复验和381工具tests通过；独立Android条件与这两处无CPU引用已记录，全部CPU消费图/二进制排除仍未证明。前批完整SPDX全文/fragment再验通过，维护运行时尚未进入用户包；原完整R1–R4、App11+/受管Mac14+与旧产品保持。

2026-10-13 [Khronos 完整许可正文](2026-10-13-computer-use-khronos-notice-checkpoint.md)：两固定SPDX全文11358 bytes、七声明绑定/一原内嵌permission与八原版权声明已复验，380工具tests/0skip通过。只是八文件片段，原7554paths/422 raw gaps/17 yes错/196目录错/5异常值保持。其它copied/generated源、CPU真实消费路径、四平台完整NOTICE/SBOM/源提供/安全策略及原R2–R4仍开放；App11+/受管Mac14+、产品旧运行时/pins/四锁/seed/App144/home未晋升，整体active/partial。

2026-10-13 [ANGLE / OpenScreen 集成](2026-10-13-computer-use-integration-checkpoint.md)：20件固定config/header、Perfetto准确DEPS/Gitlink/四manifest及其4件许可输入已验，16源码规则、DEPS export/helper AST与四原GN/custom vars确认四条实际路径映射。最终04/05独立同SHA、357审查工具tests/0skip通过；7541paths/1251refs/829resolved/422 raw gaps，17 yes错/196目录错/5异常值保留，Perfetto新六目录缺项不隐藏。完整NOTICE/R1与原R2–R4全部active/partial，产品/pins/四锁/seed/App144/home未晋升；App11+/受管Mac14+保持。格式/hash/清理收尾见本批checkpoint。

2026-10-13 [Crashpad 配置与 lss 来源](2026-10-13-computer-use-crashpad-checkpoint.md)：根 `.gn` 的 Chromium default、四实际GN参数、external lss/zlib分支与真实路径许可已验；7518paths/422 raw gaps/17 yes错，190目录错/5异常值保留。319审查工具tests/0skip和独立重算通过，原standalone metadata缺项不排除，不是完整NOTICE/产品验收。R1仍红、原R2–R4全部active/partial；App11+/受管Mac14+及产品/pins/四锁/seed/App144/home不晋升。

2026-10-13 [固定生成依赖闭包](2026-10-13-computer-use-node-generated-checkpoint.md)：准确GCS generation/11.49MB/SHA与recipe AST目录布局、6544 tar entries/234原文paths及四actual checkout同-object段已验。7273→7507 paths，439→422 raw gaps，19→17 yes错；190目录错/5异常值保留。Lit archive/lock3.3.0与README3.2.1差异不抹平；原目录/lockv3假设首败保留，复用唯一包修正后296工具tests/0skip与独立重算通过。完整NOTICE/R1仍红、原R2–R4全部active/partial，App11+/受管Mac14+和产品/pins/四锁/seed/App144/home不变；删除0/实际回收0，无提交发布。

2026-10-13 [四平台来源/生成输入](2026-10-13-computer-use-platform-coverage-checkpoint.md)：主树+63来源与原实际manifest逐项对齐，准确匹配59/62/56/56、身份冲突0，未采实际Git来源98/107/96/96；所有439 raw gaps/19 yes错/190目录错/5异常值原样保留。八件准确Crashpad/node源271849 bytes已采，发现Lit锁3.3.0与README3.2.1差异并定位固定GCS对象；未执行GN/包管理器或伪造路径。258工具tests/0skip通过，不是完整NOTICE或产品验收；R1仍红、原R2–R4完整active/partial，App11+/受管Mac14+与产品/pins/四锁/seed/App144/home保持。删除0/实际回收0，无提交发布。

2026-10-13 [实际recipe/helper/config](2026-10-13-computer-use-recipe-provenance-checkpoint.md)：原recipe实际ZIP整包SHA、两helper/Git bytes、四原log布局/DEPS fallback AST及四config/Git blobs/32 bootstrap keys已核验，236工具tests/0skip通过。完整NOTICE/R1仍红，63源439 raw gaps/19 yes错/190目录错/5异常值保持；不是远端文件测量或产品验收。产品/pins/四锁/seed/App144/home及原R2–R4、App11+/受管Mac14+active/partial不变，无提交发布；删除0/实际回收0，source ZIP/证据/当前preview保留。

2026-10-13 [六个实际递归仓库](2026-10-13-computer-use-checked-recursive-checkpoint.md)：按四份实际manifest补六固定源/36文件，合并63源7273路径/439 raw gaps；rapidjson一项关闭，但19 yes错/5异常值及新增后的190目录错保留。206工具tests/0skip、原01/新02重算SHA一致，只证明来源工具，不是完整NOTICE或CU交付。完整R1–R4 active/partial，产品/pins/四锁/seed/App144/home不晋升；App11+/受管Mac14+不变，无提交发布，删除0/实际回收0，live preview/不确定owner不动。

2026-10-13 [实际checkout/固定GCS对象](2026-10-13-computer-use-checkout-object-checkpoint.md)：四完整manifest/bootstrap与有界stdout12件已封存；57来源/实际递归pins对齐，不把未出现当许可证豁免。原四ZIP的SHA256/MD5/CRC32C与固定GCS generation一致，Windows24.062ms差异保留、不作publisher build认证。192显式工具tests/0skip通过，完整NOTICE/R1仍红，原19 yes错/187目录错/5异常值不降；recipe/config已识别但实际CIPD helper/fallback待查。App11+/受管Mac14+、产品/pins/四锁/seed/App144/home与全部R2–R4范围active/partial不变，无提交发布；删除0/实际回收0，source/SDK/证据/live preview保留。

2026-10-13 [四平台实际archive/GN参数](2026-10-13-computer-use-archive-build-checkpoint.md)：四完整GetBuild与16同build白名单日志已封存，同commit/position/四核心pins和准确ZIP上传step/path SUCCESS实核，actual写入args与同源默认确认sample机制。169工具tests通过；gclient custom vars不等于实际checkout/有效递归继承，publisher路径不等于GCS generation/下载bytes关联，完整通知/R1仍红、原19 yes错/187目录错/5异常值不降。App11+/CU受管Mac14+、产品/pins/四锁/seed/App144/home及原R2–R4全部范围active/partial保持，未提交发布；删除0/实际回收0，source/CI/依赖/全部证据保留。

2026-10-13 [gclient机制/公开build入口](2026-10-13-computer-use-gclient-build-evidence-checkpoint.md)：准确helper/AST只证明源码变量优先级；OpenScreen25 Git/7 non-Git对齐，原ANGLE核心数据保持。4页361条固定commit read-only CI已封存，四archive IDs已定位；LinuxZIP上传路径诊断可读，但完整raw/四目标/generation关联未齐。140工具tests通过不替代完整NOTICE/App/native；原19 yes错/187目录错/5异常值仍保留，R1红，产品/pins/四锁/seed/App144不晋升，App11+/CU受管Mac14+与原全部R2–R4active/partial保持；删除0/实际回收0，source/evidence/home保留。

2026-10-13 [公共浏览器源/ANGLE递归目录](2026-10-13-computer-use-browser-recursion-checkpoint.md)：再10固定源/876 paths，57源7237 paths/440 raw gaps；19 yes带错/187目录错/5异常值保留，原12错误关闭/新6递归错误暴露。ANGLE80 Git/441 non-Git目录准确对齐但未证明继承或实际binary；113工具tests/0skip通过不是CU交付。R1完整NOTICE/源提供/安全策略仍红，产品/pins/四锁/seed/App144不晋升，App11+/受管Mac14+及原R2–R4全部范围active/partial保持；删除0/实际回收0，source/evidence/home保留。

2026-10-13 [LLVM/Windows支持源](2026-10-13-computer-use-support-license-checkpoint.md)：再11准确源/16 paths，47源6361 paths/439 raw gaps；25 yes带错/165目录错/4异常字段保留，14许可缺口关闭，99显式tests/0skip通过。Not Included不当NOT_SHIPPED，所有来源计数不等于binary数量或完整NOTICE通过；R1仍红，不晋升产品/pins/四锁/seed/App144。App11+/受管Mac14+、原R2–R4全部范围和active/partial保持；本批无可退休build/执行副本，删除0/实际回收0，source/evidence/home保留。

2026-10-13 [公共运行源与缺口分类](2026-10-13-computer-use-runtime-license-classification-checkpoint.md)：新增17准确公共源/28 paths，36源合并6345 paths/1201 raw refs/459 unresolved；默认policy39 yes带错/154目录错/4异常值保留，17许可缺口关闭。94显式tests/0skip通过，0tests discovery不算验收；来源分类不是binary数量或完整通知通过。R2上一批22源码SHA不变、收尾质量再次通过；产品/pins/seed/App144不晋升，App11+/受管Mac14+和原R1–R4完整范围保持active/partial。本批无新可退休构建/执行副本，删除0/实际回收0，source/evidence/home保留。

2026-10-13 [通用有界运行时合同](2026-10-13-computer-use-runtime-contract-checkpoint.md)：ZIP/TAR/树/复制/prepare/manifest/repair/rollback/audit共享固定额度，坏包/超额保留旧目录和journal；Windows/Jammy06最终各Core1000/CLI3/strict、Node35、Linux冻结SHA与质量通过。四现有源容量适配，共享seed445指纹不变；清两旧incremental逻辑575.3MiB/H可用观察增455.0MiB，current候选/source/全部失败证据保留。不是runtime晋升、Mac/native browser/model或整套CU验收；来源/完整通知、最终四锁/新candidate/R3–R4仍开放，App11+/受管Mac14+、App144/home与完整active/partial保持。

2026-10-13 [默认policy/媒体来源](2026-10-13-computer-use-license-policy-media-checkpoint.md)：静态规则与媒体7额外pins实际采集，19源6317路径/1201 raw引用/477未解析；policy470 yes/56有错/153目录错/4非标准字段保留，不能当binary数量。四个媒体许可缺口关闭，75测试/质量通过，完整component seals/不可变输入已接。只是审查工具，产品/pins/seed/App144不晋升；完整通知/四架构/安装/真人/最终12h仍开放，active/partial。下一批正确来源分类与通用R2合同可并行，不缩原范围。

2026-10-13 [核心外部许可输入/闭包](2026-10-13-computer-use-external-license-inputs-checkpoint.md)：12个固定公共来源456路径/440仓库内blob实际采集，49测试通过；合并重验6289路径/1199原始引用，714解析到verified bytes、485未解析（不是shipping计数）。FreeType FTL/FFmpeg CREDITS等已补，递归/平台/non-Git/完整通知/源义务仍未完成；产品/pins/seed/App144不晋升。下一步官方parser/prune/OS分类与运行组件来源，继续R1–R4全部原范围，整体active/partial。

2026-10-13 [固定源码/许可输入](2026-10-13-computer-use-pinned-license-source-checkpoint.md)：主仓库509552条固定树、5833路径/5196原文blob收齐，273 Git pins全部对齐，22测试通过；455原始缺失/远程引用不代表455个shipping组件。外部源/完整通知尚未齐，不晋升产品。纠正过强GN前置要求，默认Credits扫描checkout且exit0不能证明coverage；继续R1–R4全范围，App11+/受管Mac14+与active/partial不变。

2026-10-13 [维护版内部原生诊断](2026-10-13-computer-use-maintained-native-checkpoint.md)：官方157/Node22/PW1.64在Windows/Jammy各28项通过，Linux首败28未进入/新02成功均保留；Linux NSS/NSPR/ALSA包声明已补。只是worker/headless兼容，通知/生产pack/用户sandbox/安装/Mac原生/真人/最终12h未完成，不更新产品pins/seed/App144或宣称交付。Node9/Python9/guard14与质量通过，清退休两browser逻辑1.46GiB（H观察增约820MiB，WSL约678MiB而VHDX物理未知）；当前依赖/source/evidence/预览/home保持，完整active/partial。

2026-10-13 [Linux155完整来源/静态ABI与固定补丁](2026-10-13-computer-use-linux155-source-checkpoint.md)：全包673文件/797546240 bytes、8.28MiB Credits候选和9 ELF的18个readelf已验，GLIBC2.25不代表Jammy启动。后续三个固定仓库281 blob/6.13MiB已归档，54来源测试/质量通过；Safe Browsing/quarantine/MOTW/FedCM默认关闭等真实差异明确，不作为无差别替代。格式/配额、Mac同版本、codec/源覆盖/安全/native均待完成，产品/App144/pins/seed未晋升。临时ELF逻辑550MiB已清，源包/证据/预览/home保持；完整交付仍partial，下方为历史。

2026-10-13 [维护浏览器替代来源检查点](2026-10-13-computer-use-maintained-browser-notices-checkpoint.md)：Windows155固定提案实查75件/约420MiB树和8.55MiB完整Credits候选；Mac/Linux仍来源查询级，codec/源覆盖/安全/native保持未验，不晋升。来源工具25测试/质量通过；原官方四快照仍sample/exit2。产品/App144/pins/seed均未改，完整交付仍partial。

2026-10-13 最新[系统边界与维护来源检查点](2026-10-13-computer-use-managed-browser-boundary-checkpoint.md)：Win/Jammy Core各980、前端477与strict通过，受管Mac14+/App11+门禁和十五语言已接，但Mac native not_run、App144未重打。Node22/PW1.64 source-pair实际Win/Linux import完成，四官方未品牌化快照完整通知门禁失败（仅401-byte sample），compiled仍旧。用户preview PID59356/home/未发送草稿保持、fixtures停止；完整交付仍partial，按新R1–R4继续，以下为历史。

2026-10-05；工作区 `grok-app-computer-use`，分支 `feat/computer-use-implementation`。

## 当前结论

2026-10-12 后续[来源/归档边界](2026-10-12-computer-use-runtime-source-bounds-checkpoint.md)：最终WindowsCore963、Jammy ZIP10及两边all-targets strict通过，修坏包提前删除原目录与目录/quota/路径/type边界；Node22四源包/两Mac最低11.0已实清点，runtime未晋升。CfT分发和Mac最低13/PW支持14与App11底线冲突仍开放，未缩范围或依赖系统runtime。首败/原snapshot保持，清本批两旧cache逻辑573MiB/H可用观察增417MiB；app144/home/草稿保持。完整交付仍partial，以下为历史。

2026-10-12 最新[完整Jammy App续行](2026-10-12-computer-use-jammy-app-build-checkpoint.md)：03完整release/custom-protocol、release all-targets strict和导航9项通过，最高GLIBC2.34；原23735生成锁owner指纹exit1保留，独立内容/seed复验通过。相同正式ELF实际窗口首次设置页、私有PW身份与正常close224ms已验，原三owner全退出；不是installed/portal/模型CU或12h通过。新[runtime编译合同](2026-10-12-computer-use-runtime-pins-checkpoint.md)已实现并通过完整Core956/strict，旧版本和seed尚未晋升，CfT许可/系统底线/新布局/文件配额迁移及三格式安装仍缺。Node8/输入6/质量通过，远端CI未跑；app144首页/A/B/上传页已亲自核验，草稿未发。所有原交付范围仍partial，下方为历史。

2026-10-12 [维护版SDK与装载门禁](2026-10-12-computer-use-maintained-sdk-checkpoint.md)：Jammy实编PW1.6.9+Debian安全补丁/静态EI，16件runtime的许可/ABI/实际模块及8负例通过，最终48项回归0skip。Ubuntu22独立CI已写但远端未跑、release未接。接下来产品loader/完整App/三格式安装/portal，完整三平台四surface/UI UX/model/12h/D00–D31仍partial。当前app144与未发送草稿不变，清退休fault逻辑约31.9MiB，主机实际回收未知。

2026-10-12 [Ubuntu22私有SDK检查点](2026-10-12-computer-use-jammy-sdk-checkpoint.md)：独立Jammy/glibc2.35实际PW1.0.5/静态EI1.5编译、原/迁移目录C contexts通过；所见DSO最高GLIBC2.34。不是最终维护版本审计、完整App/portal/release/三格式安装通过，整体partial。app144/未发送草稿保持；清退休objects逻辑约8.9MiB，WSL空闲观察增约9.7MiB，主机VHDX回收未知。

2026-10-12 [普通app144真人任务就绪](2026-10-12-computer-use-ordinary-app144-next.md)：三内嵌页已实际打开，独立receiver初值0，session health正常；新任务草稿未发送，等待用户确认方便，未自动授权/选择文件。source145仅fixture EOF收尾与原child回归，Node9/lint/质量/Verify通过；用户产品包仍144且不需本批重建。完整范围仍partial。

2026-10-12 [source144真实journal异常组合](2026-10-12-computer-use-native-journal-faults-checkpoint.md)：九项原App/Host维护与文件后置条件通过，39 held owners最终退出；新App bounded hold正常unwind，不强杀。首败预期分类错误保留。清两重复EXE/helper输入逻辑190362639 bytes，空闲delta因报告统计失败缺失，不虚报。当前唯一app144/PID19768/home/login保持；真人CU/picker、D22其余和完整交付仍partial。

2026-10-12 [source144正常退出取证](2026-10-12-computer-use-normal-exit-checkpoint.md)：
idle/held/before-start三原生批次业务与原30秒gate通过，14/17/20 owners退休；历史138/139超时根因未证，不回填绿。
外部runner ExitTime null首败保留，取证改读原kernel FILETIME；fresh144定向153/三strict/质量通过。
app144普通fresh包/450件ZIP回读完成，已更新唯一预览PID19768并实看首页/关于版本/历史消息，历史连接“就绪”；原133及14 observed owners正常exit0，原home/login保留。真人CU模型/picker、D22其余故障组合及完整交付仍partial。
清3份退休测试EXE/H空闲观察增约254.6MiB，原失败证据/当前候选与用户包保留。

2026-10-12 [持久下载journal与实际App崩溃恢复](2026-10-12-computer-use-download-journal-checkpoint.md)：
139起派发前持久归属与显式维护已接生产；140限定清理错误分类，143 fresh153/三strict与140前端86通过。
142真实1MiB/held Wry/App force-kill、原browser live拒绝且hash不变、原owners退出后维护/重复维护通过。
138/139正常退出30秒gate再次失败，独立后查仅证实原owners后来缺失/事务0；根因开放，不回填通过。
四重复EXE已清/H空闲观察增约327.6MiB；当前preview仍app133，并非143源码构建。真人模型/picker/fresh包/其余原生故障组合与完整范围仍partial。

2026-10-12 [真实private-file联合收尾验收/source136](2026-10-12-computer-use-native-buffer-retirement-checkpoint.md)：
两真实1MiB下载在callback-first和8.013秒caller-unknown时均准确保留/退休原buffer；两同label新owner正常。
同构建五项before-start复查与原30秒owners gate通过，fresh136回归和三strict通过；首次source135退出超时仍有未确认根因。
三成功副本已清/H空闲观察增约254MiB，当前user app133/home/login不动；持久journal、真人新意图/picker和完整交付仍partial。

2026-10-12 [下载buffer联合收尾](2026-10-12-computer-use-native-buffer-retirement-checkpoint.md)：
source134真实red/135修复，136定向回归和三strict通过；原caller+callbacks一起结束才退休文件，重复回调/Drop不删除新占用。
实际五业务场景完成但原30秒browser-owner gate失败；后来自行退出不回填绿。独立error-unwind expected2及原owners退出通过。
持久crash journal与退出慢根因仍开放；用户app133主页/关于实看正常，不代表source135或CU全功能验收。
原三副本已删；最后报告统计失败有恢复记录，原磁盘净增量不可重建。完整范围仍partial，无发布。

2026-10-12 [授权等待期间状态查询修复](2026-10-12-computer-use-pending-status-checkpoint.md)：
实机原请求因未授权computer_status的自动reject使CLI取消，该Host请求随后正常过期，业务效果0。
冻结133区分原申请元数据与执行状态，950 Core/173 App回归/47 Node及strict通过；观察/输入gate未放宽。
修复133普通包/450件ZIP回读完成，唯一App10928实际主界面/关于页正常且不依赖开发服务；旧App/CLI/MCP/fixtures均退出。
尚未真人重验，不自动复活旧请求；登录/home/历史保留，正式descriptor gate保留，完整交付仍partial。
下方132“正在等待真人”是历史状态，已过期，不再是当前待批准任务。

2026-10-12 [普通入口灰度包与真人请求](2026-10-12-computer-use-gray-product-acceptance-checkpoint.md)：
source132完整450件包/ZIP与私有Node8项通过；唯一App51460走普通入口，mock/harness env unset。
实际Grok4.7/极高已申请app-webview，正在等待真人选择/批准；三页实际打开，批准前独立效果均0。
正向业务/真实文件picker/暂停恢复Stop尚未通过，正式descriptor gate保留；全范围仍partial。
旧App129正常关窗且原App/CLI/MCP owners缺失后替换；仅清旧114目录及115重复ZIP，home/失败证据保留。

2026-10-12 [上传授权正向回归与灰度门禁](2026-10-12-computer-use-upload-permission-checkpoint.md)：
fresh manifested App172 passed/0 failed/84 ignored-not-run，原 runner exit0；正向许可不选文件，Pause/Stop 退休。
app-webview 正式 descriptor 原 probe 门禁保留，须真人产品链后开放；app129 主界面正常不等于该任务入口可用。
原 ACP/model/真实 picker/其它平台/installed/12h/D00–D31 仍开放，整体 partial。

2026-10-12 [上传 MCP/native 增量](2026-10-12-computer-use-upload-mcp-checkpoint.md)：
computer_upload catalog/原上传等待已接，实际 App MCP/native 四场景、2 POST、原 owners 退出通过。
Host approval/file choice synthetic；原 ACP/model/真实 picker/真人和全交付仍 partial。
独立 app129 包/ZIP回读通过，实际无开发服务的首页/历史就绪/设置已观察；未发新模型任务或代批权限。
下方“computer_upload 未公开/原 MCP 未接/用户 app115”为历史记录。

2026-10-12 [上传原任务与共享 Host 文件入口](2026-10-12-computer-use-upload-task-checkpoint.md)：
原 TaskGrant/单次账本/不可变原 handle 内容、main choose/cancel IPC、ACP pin、十五语言任务卡/侧栏已接。
Core932/fresh manifested App201/前端450/真实渲染198通过；原生上传/回归及收尾见检查点实际候选。
真实 picker/模型 schema 与原 MCP 等待/真人/用户新包仍开放，computer_upload 未公开；完整交付仍 partial。
以下原下载检查点描述的“TaskGrant/UI 全未接”是历史状态，不覆盖此增量。

2026-10-12 [原 App 下载错误收尾](2026-10-12-computer-use-download-error-unwind-checkpoint.md)：
独立held错误负例正确exit2，24ms收尾、10原owners退出，同label新owner文件正常；正常下载2/上传8/隔离负例同候选通过。
fresh去重Rust154/Node27/三strict/质量80通过；清三重复EXE，H空闲观察增约270MiB。
原error unwind缺口关闭，按[上传完整接线](2026-10-12-computer-use-upload-integration-next.md)继续。
文件同意/TaskGrant/UI/ACP/MCP/model/fresh包未接；当前用户窗口app115，并非最新代码包；整体partial，原交付范围不变。
下方2026-10-11及以前为历史。

2026-10-11 [App WebView 上传基础与退出码](2026-10-11-computer-use-webview-upload-checkpoint.md)：
追加原下载关闭fresh01失败保留；fresh02下载两场景/上传8场景/exit2负例与去重Rust154/三strict通过。
原失败browser精确中止仅现场回收、不算成功；held断言失败unwind负例尚待验。
实际 App adapter 的 8 场景、同 SHA256 的隔离条件拒绝通过；normal exit0 / rejection exit2，原 owners 退出。
非零错误 generation 拒绝，fresh Rust154/Node27/三strict及质量通过。普通产品入口退出流程未更换。
文件选择/明确同意、Broker TaskGrant、UI/ACP/MCP/模型上传仍未接通，computer_upload 仍拒绝。
用户 App/安装包未更新；本批未观察到旧预览，所有跨平台/四surface/升级回滚/真人/长稳范围仍partial。
三份副本删除被执行策略拒绝，实际0回收、逻辑246,649,856 bytes暂缓；首败和当前候选保留。

2026-10-08 [下载最终发布与开始前导航/关闭](2026-10-08-computer-use-download-publication-checkpoint.md)：
同一实际App构建14场景、fresh定向Rust144与三strict通过；取消/页面退休后的原staged文件最终提交拒绝且清理。
响应头前导航/关闭、五秒Close失败保留hidden原owner/显式重试、8001ms grace及264字符最终路径通过。
Host批准/pin仍synthetic、ACP/MCP/model=false，前端非fresh；未更新用户App、未提交发布，完整交付仍partial。
页面就绪首败和probe退出码传播继续调查；下一步upload、D22 buffer异常维护、fresh包/真人及完整跨平台范围。
重复EXE删除被自动审批拒绝，暂缓188,401,664 bytes、实际回收0；reports/home/失败现场保留。

2026-10-08 [开始前取消与长路径转存](2026-10-08-computer-use-download-before-start-checkpoint.md)：
私有 native buffer 后转存原授权目录，264字符 Broker 路径及37-byte/hash实机通过；未依赖固定盘符或8.3设置。
开始前控制三场景和原Close两场景通过，fresh Rust144及三strict通过；Host批准/pin synthetic，原ACP/MCP/model仍false。
当前用户窗口后端未更新；最终publication、upload、native buffer异常维护、fresh用户包/真人与完整交付仍partial。
新清三份EXE实际空闲观测增282,165,248 bytes；此前八份752,066,560 bytes，失败证据与home保留。

2026-10-08 [关闭失败与恢复UX检查点](2026-10-08-computer-use-browser-close-recovery-checkpoint.md)：
关闭失败现在可见、保留原owner且可显式重试；“稍后”只收提示，重新打开成功依赖真实关闭回执。
75前端、82实际渲染、140定向Rust及三strict通过；修复大字号按钮裁切/正文不可达与迟到布局继续初始化。
真实原App retained view hidden、五秒失败/原回调重试已有局部证据；渲染IPC与Host批准仍synthetic。
用户窗口已实看并刷新，但后端未更新；fresh包、原ACP/MCP/真实Grok、余下download/upload和全部跨平台/安装/12h范围保持。
本批临时site/runtime已清，逻辑1,490,693,053 bytes，H空闲观测增1,491,226,624 bytes；原失败证据保留。未提交发布。

2026-10-08 [原 App 下载关闭检查点](2026-10-08-computer-use-app-download-close-checkpoint.md)：
原 App main IPC/真实 Wry 两场景通过，修复 polyfill 手动保存框和隔离世界桥接冲突。
真实五秒失败保留原 view，同 label create 拒绝，原回调释放后显式重试成功；UI/keeper保持响应。
127 WebView + 9 Host + 4 手动下载回归、三组strict通过，质量80/80；原进程和10个浏览器owners正常退出。
仅debug实际App library fixture，Host approval/pin synthetic、ACP/MCP/model=false；用户预览未更新。
超时恢复UX、其余download边界、upload、真人/跨平台/安装/冻结12h仍开放；未提交发布。

2026-10-08 [共享下载关闭检查点](2026-10-08-computer-use-shared-download-close-checkpoint.md)：
共享 controller 的 Cancel→真实联合终态→Close 实机通过44ms，原浏览器/keeper及新owner仍可用。
异步 App close、Closed admission、observer/COM释放已接源码，136回归/三strict/质量80通过。
原App IPC/Wry慢下载及超时恢复UX未验，用户App未更新；upload/真人/跨平台/安装/同候选12h仍开放。
清3旧EXE逻辑185,889,280 bytes，删除时H空闲实际观测增185,892,864 bytes；未提交发布。

2026-10-07 [慢下载生命周期检查点](2026-10-07-computer-use-download-lifecycle-checkpoint.md)：
Pause/Stop/navigation/独立环境Close四项通过，Close修复实际browser exit后丢回调仍busy，
保留unknown且零假完成；133回归和三组strict通过，质量预算恢复80/80。
用户当前App未重建；shared单view关闭、原ACP/UI/真人、三平台/四surface与冻结12h等仍开放。
下方下载40/40、83>80与其它结果属于此前候选，不是当前全交付结论。

2026-10-07 [App WebView 下载诊断检查点](2026-10-07-computer-use-webview-download-checkpoint.md)：
原 Broker / TaskGrant 到真实 WebView2 和 staging 已接，连续下载及重放门禁有通过证据；
修正Windows fixture接受socket的非阻塞继承与native/Wry/caller提前退休后，固定候选40/40、
6个独立profile全部通过（真实browser exit），131项定向回归、三组strict及格式通过。
旧38/40等失败证据保留；慢下载控制生命周期、长稳与真人全链仍开放，最终大型文件预算实际83>80。
用户开发预览已实际截图确认正常主页，本批下载修复尚未打入fresh App包；清理被策略拒绝，实际回收0。

2026-10-07 [App WebView 导航检查点](2026-10-07-computer-use-app-webview-navigation-checkpoint.md)：
URL/reload/back/forward 已接原 Broker、TaskGrant 与真实 WebView2；success/unknown 重放零派发，
无 history 零派发，native document generation 与 Broker targetGeneration 独立推进。
107 项 WebView 回归、MCP 8 项、三组 strict Clippy、格式质量和完整真实 probe 通过。
macOS/Linux、upload/download staging、fresh 用户包与真人原 ACP/MCP/Grok 整链仍开放。

2026-10-07 [Windows 开发入口检查点](2026-10-07-computer-use-dev-origin-checkpoint.md)：
Vite/Tauri/媒体 CORS 已统一为 `127.0.0.1:1421`，修复 WebView2 的
`ERR_CONNECTION_REFUSED`；真实 Vite 冷启动和实际窗口已验，CI manifest 后链两项测试通过。
最后 App WebView 真人请求已过期且 A/B 零副作用，不能据此标记真人链或完整交付完成。

最新[source124任务转换检查点](2026-10-06-computer-use-app-webview-task-checkpoint.md)：
候选/新run/proof/TaskGrant、整组Stop、切回、后端恢复和桌面lease并存已接。
Core1121/1137、WebView86/runtime4/命令15/ACP42/approval8、四strict与质量通过。
真实默认Windows App两内嵌页各中文保存1/计数3、fresh恢复及Stop零新效果通过，App exit0且exact-home无残留。
实机Host approval/publication pin synthetic，ACP/MCP/model均false；入口仍隐藏，用户包app115。
接原ACP pin/统一UI与真人验收，四surface/三平台等完整交付范围保持；以下source122为历史。

最新[source122多绑定与原生最终提交基础](2026-10-06-computer-use-app-webview-authority-checkpoint.md)已验：
84项WebView、15命令、42ACP、Core1110/1126及四strict/质量通过；默认Windows真实双native页中文效果、
exact释放/转交/刷新拒绝与最终guard已验。Host bind仍synthetic；candidate到新run/proof的转换、
原TaskGrant/前端/真人Grok未接。原App已退出并核验exact-home无残留；当前用户包仍app115，无新待授权卡片。
全交付仍partial，未提交发布；以下source118为上一批证据。

[D12内嵌页面选择基础source118](2026-10-06-computer-use-app-webview-selection-checkpoint.md)已验：
旧native/document receipt在刷新或同label重建后拒绝，76项WebView/15项命令回归和两strict/格式质量通过。
默认Windows实际App内嵌页中文保存1/计数1通过并正常退出，但Host bind为synthetic，
新TaskGrant、多owner、picker/前端与其它平台仍未接。本批未打新portable包；当前用户包仍app115。
其新真人任务expired/零操作，source115实时恢复/第二Stop仍not_run；全交付范围保持。

[app114主动暂停/继续/停止与source115实时记录修复](2026-10-06-computer-use-live-recovery-checkpoint.md)：
真人批准后一次保存、Pause保持5、Continue新计数6–9、active Stop保持9，未重复批准/保存/重发。
原App/CLI/MCP/runtime/browser/fixture已退出。source115补实时stream与第二Stop终态去重，
223项前端/29项默认App定向回归通过；fresh app115实际构建/450件ZIP校验通过，真实新任务仍单独记录。

[source112准确收尾状态修复](2026-10-05-computer-use-cleanup-readback-checkpoint.md)已通过：
Core1104/1120、前端401、原native四项4/0、四strict与格式质量门禁。
原截图held时的Host cleanupPending现真实为true；一次继续/等待中Stop和卡片完成状态准确。
app113已以source112实际构建，450文件ZIP逐件校验通过；真人批准后的中文业务、
一次暂停/继续后的fresh观察与明确完成通过。三次计数都发生在暂停前；新业务操作/active Stop随后在app114通过。
[source114聊天恢复修复](2026-10-05-computer-use-transcript-recovery-checkpoint.md)通过201项相关回归与原journal只读投影，
fresh app114已构建/450文件ZIP逐件校验。修复旧取消标记被排在恢复完成答复之后的消息分组问题。
app113原App/CLI/MCP/worker/browser/fixture均已退出，home保留；source112清理17组旧cache，
逻辑9.41GiB、H空闲观察增5.60GiB，精确targets/证据见检查点。
下方source111及app109为已保存的历史验收；完整交付范围保持。

[source111原截图等待恢复验收](2026-10-05-computer-use-early-continue-capture-checkpoint.md)已通过两个独立native场景：
提前Continue共用一次恢复，原PNG结束后才续行/新动作；等待中Stop不恢复。
本批仅测试/probe改变，真实App仍为下述source108/app109；Host/ACP synthetic，不能替代真人恢复或其它平台交付。

[app109实际应用检查点](2026-10-05-computer-use-completion-app109-checkpoint.md)：
[明确任务完成收尾修复](2026-10-05-computer-use-explicit-task-completion-checkpoint.md)已进入source108的
实际Windows新包app109；新前端、应用、运行时构建和450文件解包校验均exit0。
新版真实Grok经真人Host批准自动续行，中文备注/恰好3次计数/一次明确完成全部通过；
18工具消息、0工具错误，卡片任务已完成，原browser退出。本批App/CLI/MCP/worker/fixture均已退出。
原登录/会话和当前app109保留；旧包与废弃缓存按精确清单收尾。
本批清2组过期编译代和app102旧包/ZIP，逻辑3.45GiB、H空闲实测增加3.38GiB；最新包、
依赖、seed及证据保留，未知working和失败现场延期。精确清单与重建步骤见上述检查点。

完整产品尚未可交付。此前完成的浏览器自动化、Core/ACP/MCP 测试不能证明
用户已经能在安装的 App 中交给真实 Grok 完成任务。下一里程碑是 Windows
开发试用包及其扩展验收；最新Windows受管浏览器的真实聊天/真人批准/中文业务任务已通过，
见下方检查点。四 surface、Windows/macOS/Linux、两种 Mac
架构、UI/UX、安装升级恢复与长稳等原首版范围保持。

历史app102应用来源为 source101：3319 文件，manifest SHA-256
`4e7129202901abeb00e4e06459a9b301df9174fa9621d0d6bf6829a63050af19`。
[首次聊天初始化修复](2026-10-05-computer-use-first-chat-bootstrap-checkpoint.md)
与 [Managed Browser 恢复修复](2026-10-05-computer-use-managed-task-recovery-checkpoint.md)
均已打入实际 Windows 应用 app102，构建、完整便携打包与逐文件解包校验均 exit0。
旧 artifact96 与 app-artifact-98 都是 lib 测试 EXE；不作为应用交付。

## 当前 Windows 试用包 app115

- ZIP：`dist-portable/Grok_CU_20261006_app115.zip`，208412480 bytes；
  SHA-256 `43c55c10839a49e7490ff55d9daf05fd994346ed7fc92e5841a5e7b8f790d522`。
- EXE：`dist-portable/Grok_CU_20261006_app115/Grok.exe`，92958208 bytes；
  SHA-256 `3ec07f73bf23ee6292346fb8ab9cc0eda18ba61282256362ee52ddf2c502cdf6`。
- source115：3327件，manifest `6a45dc4d933366943e196c68ccc0ec852e843e55f7bdf665e400b2102a7794c6`。
  fresh UI40.70s、Rust77s，450件ZIP逐文件校验通过；未签名本地测试包。
- 入口：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/delivery-app-115/launch-preview.ps1`。
  原独立home/登录/会话保留，拒绝重复预览。新任务实际expired/无授权、零网页副作用；
  App/CLI/MCP/fixture均已正常退出，source115恢复/第二Stop真实任务仍未验。
- 实际重开原app114会话，两答复、暂停标记、自动补记的host_exit标记按顺序显示；
  host_exit是旧漏记Stop遗留lease的启动恢复，不能作为app114正确记录Stop的证据。
- 旧app114重复ZIP已清，约199MiB；解包目录作为首败证据保留。
  原owners退出后另清6组旧cache：逻辑6.85GiB，H空闲观察增4.53GiB；最新缓存/包/源/证据不变。
  unknown working与失败现场保留，见最新检查点。下一独立批次是D12新任务内嵌浏览器接入。

## 历史 Windows 试用包 app114（已退出，ZIP已清理）

- ZIP：`dist-portable/Grok_CU_20261005_app114.zip`，208408329 bytes；
  SHA-256 `46826028539d2feeec061fbd4fa120c5a9ab3da7f2fb45f3bb0aef1ba1478757`。
- EXE：`dist-portable/Grok_CU_20261005_app114/Grok.exe`，92957696 bytes；
  SHA-256 `266d1799254958058b1692fd34fd662ee315ec29cde81c221529e3d919bdefa3`。
- source114：3326件，构建前后与包装后复验一致；fresh UI38.52s、Rust54.81s，450文件ZIP逐件校验通过。
- 入口：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/delivery-app-114/launch-preview.ps1`。
  复用原独立home、登录、会话和instance；新业务/主动Stop验收需要新任务真人批准。
- 实际重开原恢复会话，已确认取消标记居于两答复之间、两侧19/3工具分离，原journal未改。
- 本地未签名测试包，完整产品仍未可交付；旧app109/app113及ZIP已清理，原home/证据保留。
  本批另清1组旧增量代，两项合计逻辑3.82GiB、H空闲观察增2.22GiB。

## 历史 Windows 试用包 app113（已退出，旧包已清理）

- ZIP：`dist-portable/Grok_CU_20261005_app113.zip`，208408116 bytes；
  SHA-256 `1fdf07e206f45dbf6d8ca1c317a833b61980a6c732b76b91089db0e3f1251560`。
- EXE：`dist-portable/Grok_CU_20261005_app113/Grok.exe`，92957696 bytes；
  SHA-256 `e293476852d50ec496980d39e19fdf5ab8a1194bb0f70ee9d2e94dcf836faa0b`。
- source112：3325件，构建前后与包装后复验一致。fresh UI39.30s、Rust1m03s，
  完整运行时/扩展与450文件ZIP逐件校验通过。未签名本地预览，不是正式发行。
- 入口：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/delivery-app-113/launch-preview.ps1`。
  复用原独立home、登录、会话和instance；VerifyOnly已通过，无重复预览。
- 本批真实Grok恢复的fresh观察与明确完成已验；恢复后新操作/active Stop、installed/其它平台仍开放。

## 历史 Windows 试用包 app109（已退出，旧包已清理）

- ZIP：`dist-portable/Grok_CU_20261005_app109.zip`，208405681 bytes；
  SHA-256 `af6310bd4bb0cf09e91854a091299daee83bb898725b99750a19d6d0388f76d0`。
- EXE：`dist-portable/Grok_CU_20261005_app109/Grok.exe`，92956672 bytes；
  SHA-256 `3ab2b4cf60539228a33dae1177291f8c410baf0ab3904426d1fbb825bf8c1e4d`。
- source108的3323件输入构建前后与包装后复验一致；清单450文件，ZIP逐件解包通过。
- 入口：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/delivery-app-109/launch-preview.ps1`。
  复用原独立home、登录与instance；closeToTray和keepTrayForSchedules均false，测试完成后PID16980已退出。
- 真实会话 `09d11384-001b-4879-b2a9-8efbb5fc9cc7` 的本机收尾任务已通过真人批准、自动续行、
  中文备注/3次计数、明确完成与卡片/原browser收尾；18工具、0工具错误。其它恢复/平台验收仍开放。

## 历史 Windows 试用包 app102（已退出，旧包已清理，原home/证据保留）

- ZIP：`dist-portable/Grok_CU_20261005_app102.zip`，208403177 bytes；
  SHA-256 `140c0c81f01dd1c7000b7416e5adeea9832e8461bd678ac0079b771fbb653ffd`。
- 主程序：`dist-portable/Grok_CU_20261005_app102/Grok.exe`，92934656 bytes；
  SHA-256 `5f44ffed1adf66923707deaa8c769085731ce23ca08cc25d0268d4b36561d5c8`。
- 450件清单文件，共533915062 bytes；清单 SHA-256
  `647478b353d7db19671bd802142d59a28ad6b9f19d776127867f73dda46ae3b6`。
- 入口：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/delivery-app-102/launch-after-app98.ps1`。
  复用原app98独立home和instance，拒绝已有任务预览。
  该home已由官方CLI重新登录成功，当前真实App已取得首次聊天的CU申请卡；
  真人批准后中文备注和三次计数已实际完成，见下方新检查点。
- 测试home设置 `closeToTray=false`、`keepTrayForSchedules=false`；关窗走实际退出流程。
  app102已通过独立入口启动，PID42300；真人批准及本机任务完成后已正常退出。

Rust应用构建exit0、1m44s；前端/扩展/worker未改，复用app98已编译前端，
164文件构建前后hash一致。默认Core1101项、App控制4项、native恢复1项（两个场景）、
Core/App默认/probe strict与格式质量通过；原门禁中的Host/ACP仍有synthetic。
本次独立的真实Windows受管浏览器业务任务已通过，其它平台/表面与恢复验收保持待办。
构建、source和包身份见 `delivery-app-102/artifact.json`；未签名、未发布。

[新登录与真实首次聊天检查点](2026-10-05-computer-use-real-login-cold-chat-checkpoint.md)：
新账号的真实CLI冷连接验证通过，自动准备private runtime、显式catalog更新0、
模型调用request工具。当前app102内的固定本机任务约5秒显示任务批准卡，
批准前页面未操作；真人批准后自动保存“中文验证 你好”，计数恰好增加三次至3，
20条工具消息、0工具错误，最终模型结果与fixture服务端一致。
完成后授权卡的状态文案未收尾已记录；本次暂停/恢复/Stop仍未验。
App/CLI/受管浏览器已退出，本批另外实测回收1.63GiB，当前包/登录/会话保留。

用户16:50发出的fixture任务保存在原App数据目录的既有会话中，实际工具搜索没有
`grok-computer-use`；新app98独立home没有这条聊天。当前只能证明实际使用会话缺少
CU工具与预览目录不一致，不能将其归因于模型不愿调用、CU开关或隔离home登录错误。
下一次必须核对实际应用/会话/catalog身份，不让用户在多个同名窗口间猜测。

[本批清理与预览收尾](2026-10-05-computer-use-preview-hygiene-checkpoint.md)：
app97/app98便携目录和ZIP、13组过期增量缓存已删除；逻辑10596489277 bytes，
H空闲实测增加8455868416 bytes（7.88GiB）。当前包/hash、源码、依赖、seed及失败证据保持。
app97/app98已退出；另退出9月30日构建的无任务旧NSIS测试App PID77028。
其它工作区验证进程未动，未知working和失败现场延期。只保留一个当前试用包。

## 历史 Windows 试用包 app98（已退出，旧包已清理）

- ZIP：`dist-portable/Grok_CU_20261005_app98.zip`，208400801 bytes（约199 MiB）；
  SHA-256 `e98be9bc156f6c788719335002994cec9bfda9bd1f01f2aa432f47e4b16a4d53`。
- 主程序：`dist-portable/Grok_CU_20261005_app98/Grok.exe`，92929024 bytes；
  SHA-256 `55fee7a3e8b7897139ee3e39c348e9d22e35d0e8855a81e0d825717b9d7047d2`。
- 包内450件清单文件，共533909429 bytes；清单 SHA-256
  `7d59e93a7f63b108eb55804455e57abbbf5c1998526eeea7d9e5d2bc5a717388`。
- 本地隔离启动入口：
  `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/delivery-app-98/launch-preview.ps1`。
- 启动时 PID39348，创建时间2026-10-05T08:48:22.3174009Z；窗口标题Grok、
  handle276566792、Responding=true；独立 instance `com.grokapp.desktop.cu-app98-preview`。
  当时设置回读 independent/zh/ask、CU默认off；随后已退出，日志shutdown=true。

source98 在构建前后与包装后复验一致；Rust应用构建阶段2m06s，前端1m03s。
seed与扩展身份保持；10项运行时测试、App默认/probe两套strict、格式与质量门禁通过。
版本标识为 `0.2.38-cu-dev.20261005.app98`，未签名，未发布。
新包包含冷启动运行时修复；真正冷聊天的 MCP 注入和真人批准后业务任务仍未验。
原fixture目前仍未保存/计数0，不能将能启动或lib测试写成实际CU任务完成。

初次预览的精简配置不符合完整设置schema，被App回退到默认值。确认该新窗口
没有直接agent子进程后，只关闭本批PID58360，修正完整生成配置并重新启动；
旧app97/fixture保留。最终独立模式已实际回读，首轮日志保留。

打包脚本已移除其UUID解包校验副本（逻辑534009986 bytes）；exact临时前缀残留0。
该自动清理未单独测量实际空间，不以同期磁盘变化归因。当时活跃owner阻止追加清理；
现在已完成上述103批精确清理，旧记录不代表当前仍运行。

下方 app97 为 source96历史构建，便携目录和ZIP已精确清理，原证据保留。
当前 `target/debug/grok-app.exe` 已被source101构建更新，不能拿该路径复验旧app97。

## 已完成与未完成

| 项目 | 当前证据与限制 |
| --- | --- |
| 已有浏览器观察/截图/操作生命周期 | source96 的 81 项自动化 native 联测通过；Host/ACP 手势仍含 synthetic，未取得真人安装使用整链 |
| 按任务申请、选择范围、批准及控制 UI | app109真实批准/自动续行/中文业务/明确完成及卡片收尾已通过；暂停/恢复/Stop其它场景待验 |
| 已有浏览器标签页的产品入口 | `task_view.rs::present_view` 未向 UI 提供 `existing-tabs`；UI 过滤可选范围，尚未开放 |
| Windows 桌面与 Managed Browser | Managed Browser真实中文备注/计数任务已通过；桌面真实业务任务与其它场景仍待验 |
| App WebView、macOS/Linux与完整输入恢复 | 按 remaining-plan 继续；不由浏览器测试代替各平台验收 |
| UI/UX、安装升级回滚、真实模型任务与 12h 长稳 | 尚未完整收束，正式版日期缺乏可靠依据 |

请求解析器 `task_consent/parse.rs` 已接受全部四种 surface。
旧文档中的“raw parser 仍 Desktop-only”具体指 `task_scope.rs::TaskScopeTarget::parse`
的原始 Host 目标选择入口，不表示模型的 `computer_request_access` 只接受 desktop。
不要为开放 UI 绕过绑定、批准或安装使用验收。

## 历史应用构建 app97 / source96

命令：

```powershell
$env:GROK_CU_BUNDLE_TARGET = 'x86_64-pc-windows-msvc'
pnpm exec tauri build --debug --no-bundle `
  --config src-tauri/tauri.windows.conf.json `
  --config src-tauri/tauri.dev.conf.json
```

证据：`tools/computer-use-probe/.run/codex-u04-existing-app-20261004/delivery-app-97/build.log`。
构建前 C/H 空闲分别约 246/304 GB。固定扩展 48 件、15 locale 校验通过；
runtime prepare/check 与 seed 审计通过，seed 445 件、440975863 bytes、零 links。
`pnpm build:ui` 已通过，Rust 应用编译 exit0（编译阶段 3m02s）。
3316 源文件在打包后复验一致；artifact96 测试 EXE hash 保持不变。

| 产物 | 路径 / SHA-256 |
| --- | --- |
| 历史 App 保留副本 | `dist-portable/Grok_CU_20261005_app97/Grok.exe`；`51eb4d5795a5ba8837b98e15f0ce6c3019d47d7a639e5496e1bad8f82ed7ebce` |
| 开发便携目录 | `dist-portable/Grok_CU_20261005_app97`，450 件清单文件，共 533846453 bytes；附包内清单 |
| 包内清单 | `0076c0b5c7d0f87d78b7949bc9cd860a900c7826210bbcc5893e59439b0e673b` |
| 开发 ZIP | `dist-portable/Grok_CU_20261005_app97.zip`，208395324 bytes（约 199 MiB）；`0446d3c8934845480eb21165fcfe66ccc7b05991a69b31310066c77c68f96d8d` |

版本标识 `0.2.38-cu-dev.20261005.app97` 是本地开发包标识，App semver 仍是
0.2.38。包未签名。本批仅打本地开发包，未触发正式发行流程。
stage 与 ZIP 解包后的逐文件校验均 exit0，原运行时与扩展身份保持一致。

隔离启动入口：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File `
  tools/computer-use-probe/.run/codex-u04-existing-app-20261004/delivery-app-97/launch-preview.ps1
```

本批实际启动 PID28196，创建时间 2026-10-05T08:01:32.3968214Z，
路径为上述便携目录的 `Grok.exe`，窗口标题 `Grok`、handle25166660、Responding=true。
使用独立 instance `com.grokapp.desktop.cu-app97-preview`、本批 `startup-home`，
当时设置readback为independent/zh/ask；现已退出，日志shutdown=true。
旧包已清理，独立home保留。
这些证据只证明当前包可启动、原生窗口响应；不证明前端全部交互或真实 Grok
Computer Use 任务已经通过验收。直接双击 EXE 会走默认 App 数据目录；本机预览用上述入口。

本批包的暂存与解包校验副本由原包装脚本收尾，事后 exact 前缀检查残留为0。
该副本逻辑大小 533947010 bytes；实际回收空间未单独测量，不用同期磁盘变化归因。
旧增量代只读检查无完整可删除配对，本批另行清理0；旧 working 代继续保留。
上述是历史打包时保留状态；103批后只保留app102和原home/依赖/seed/验证证据。

## 下一里程碑的验收

1. 实际应用构建成功，完整携带运行时；记录 EXE 与包的来源/hash，准备独立测试数据目录。
2. 在真实 App 中发送任务，Grok 请求后显示范围选择与批准入口；批准前不操作。
3. 原任务完成观察、输入和结果核验；暂停、提前一次 Continue 及等待中 Stop 可实际使用。
4. 已有浏览器扩展安装、真实 Share 与批准链验收后开放其产品入口。
5. 提供 Windows 开发试用包、简短步骤及具体已验/未验能力，再继续三平台正式交付。

每批结束执行 AGENTS 第 9 条：原 owner 退出后仅精确清理已验证的过期产物；
保留当前应用、seed、依赖及失败证据，活跃或未知 owner 的目录延期。
