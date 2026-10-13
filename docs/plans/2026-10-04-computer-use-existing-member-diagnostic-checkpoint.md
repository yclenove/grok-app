# ExistingTab：多成员正向首败的精确诊断

状态active / partial — not releasable，单Codex writer；不启动Grok/子代理，不提交发布。
接[discovery检查点](2026-10-04-computer-use-task-consent-u04-existing-discovery-checkpoint.md)。
原dirty/untracked/fix.patch和全部failed roots保留；本批没有生产行为修复。

## 为什么先补诊断

source25/build26十场景第二轮9通过/1失败，首败位于B成员五动作，前10checks完成。
原detail残留A阶段的scroll-action，不能证明是B滚动、输入、回包或独立DOM断言失败。
没有读取原Cookie/Token、配对记录或浏览器数据库来猜测原因，也不靠复跑绿销项。

原memberActions仍沿原MCP/认证HTTP/production source MV3及原TaskGrant，保持每动作
fresh observe、原输入与相同独立DOM/幂等后置条件。新增固定member/action/phase，
有限outcome kind和replyError；输入只记录是否匹配，DOM数值只来自自建页面的clicks/
scrollTop并限0–10000。数组分别限32/16项，不记录原reply、身份、ref或错误payload。
重复点击的deepEqual仍保留；新有限kind断言是其原kind要求的提前定位，不增加派发。

## 当前候选与实际证据

source26/build27为未运行的初次诊断中间候选；之后补重复回包与独立DOM断言的精确phase。
当前source27仍3259文件，`source-files-27.sha256` SHA256
`fae05efa03e72f6aa953d0a52953d6405c0d095f20123aaac9f24cb27194f6dc`。
默认App build28 no-run实际exit0，原CI manifest已嵌入；Rust未改，当前executable仍
`src-tauri/target/debug/deps/grok_app_lib-63e3432e557be5b6.exe`，SHA256
`293ce35099670d5df5ac7af628a4630dc1e1288edc67f9a72942ac8ef2c22422`。
本批source脚本被原fixture实际读取，不能只凭复用Rust二进制宣称新脚本已验。

具名`existing-member-diagnostic-native-1/2/3`各实际1passed/exit0，25.90s/24.52s/24.52s，
各完整13checks，原App ACP/MCP/MV3/独立A↔B后置条件仍通过。只是未复现首败，不能
称生产根因已修复或将上批第二轮改成绿色。
Node177/MCP38、quality11/diff14及Node syntax实际exit0；App/Workbench仍13573行。
Core/前端/Rust strict未受改动影响，本批未全量重跑，不移用上批数字填本批门禁。
原10场景整套`existing-member-diagnostic-suite-1`实际10passed/0failed/exit0，178.39s、
完整67checks；带原完整前置撤销序列也未复现，不证明序列无关或原首败已修复。
不扩张时钟、不自动重放unknown、不删首败；本批没有产品修复可接受为根因关闭。

Host/Share手势、session与ACP peer仍synthetic；不是installed/真人Grok验收。
ExistingTab descriptor仍关闭，raw parser仍Desktop-only；全E5/U00–U09/D00–D31/
四surface/三平台双Mac架构/签名安装升级回滚/真实模型/最终12h均保留。

## 下一项与产物收尾

若再现，先凭固定phase/有限kind/原独立效果确认具体反例，再改生产实现并保留首败。
若未再现，不无限复跑堵住其它ready工作；原失败保持开放，诊断随正向矩阵继续。
下一ready项是原connection撤销在choose/reservation/publication切点的实际source popup
Forget→认证HTTP disconnect→原Host proof拒绝，随后profile/lease/replacement/Stop/Pause。
不能以直接改registry或test-only强制到期冒称实际用户连接/真实租约行为。

本批仅默认no-run缓存命中及日志，没有新编译site/重复包；成功的隔离browser profile与
可写runtime副本已由原owned资源合同退出后删除。整套owner终态后实际核对cargo/rustc/
App test owners均为0；未找到可确认废弃的完整old/current缓存对，因此本批未额外删除缓存。
唯一旧变体`grok_app_lib-28mpg1j4ghpco`仍含
`s-hmvxivh3e0-05sv6hk-4vexe5cqmydwi392kexc65pd9`与
`s-hmvys28aps-0y4sbl2-working`；后者是不完整working代次，保留调查，不能猜测为废弃。
current/seed/源码和全部failed profiles均保留。下一批仍以新source/artifact绑定的新清单
核对，不复用已执行身份，也不杀无关进程来制造可删除条件。
