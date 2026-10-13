# E4b.1：ExistingTab 原任务共享准备 UI

状态：**frontend verified / installation entry pending / full CU active / partial — not releasable**。
工作区 `H:/aicoding/grok-app-computer-use`，分支 `feat/computer-use-implementation`，
HEAD `8d8b71c262ab222748bca32412eea884d4b892e0`。单 Codex writer；原 dirty/untracked/
`fix.patch` 保留。本批没有提交、push、PR、发布，未启动 Grok/子代理/自动化。

## 实际实现与范围

1. Started/View 补严格 `desktop | existing-tabs` surface，增加 task-bound preparation/
   pairing-confirm API/DTO。只传原 session/full key/pickerId/nonce；renderer 不能指定
   endpoint。开始、准备和候选均核对原地址/surface/maxSelection；准备计数/状态/一次性
   challenge 有类型和一致性验证，拒绝远程地址、URL 凭据及过期 challenge。
2. `TaskExistingPreparation` 与 picker 验证抽独立 domain 模块，原 card/panel 继续共用
   `ComputerTaskActions`。ExistingTab 先读 preparation，有真实可用 Share 才 discover；
   Pair confirm、候选选择和最终批准分别需要明确用户动作。receipt 仍不是 grant，批准
   之后只等原 Host stream，不把回复 runId 变成执行权限。
3. 一个原 modal owner 最多有一个串行 metadata observer。无可用分享/空 receipt 时每秒
   检查；候选非空停止轮询，不刷新非空 IDs。读失败停止自动重试，提供显式检查按钮。
   Hide 撤销旧 visibility epoch，Close/Stop/Cancel/替换/最后 consumer 退出取消原 picker，
   迟到回复不能 discover、显示代码或恢复 grant。Showing 不退休原准备调用。
4. 原 GlassModal/按钮/候选布局复用；明确任务范围，配对码仅原 modal 且 App confirm 后
   可见，地址/码沿既有纯展示组件复制。提供分享说明、忙/不兼容/空/失败/到期状态和重试；
   Stop/Cancel 不等待 metadata。全部新增文案同步15locale，没有增长 App/Workbench。
5. **安装路径仍未交付**：现在的折叠说明诚实只支持源码版 Load unpacked，没有商店版、
   安装包扩展导出/获取按钮。本批不伪造可用安装入口，不开放 production ExistingTab
   descriptor。不是完整 E4b，也不是真人 App/MV3/Grok/新 TaskGrant 联合验收。

## 首败与验证

证据根仍为 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`，旧文件不覆盖。

- `existing-ui-red-1.txt`：新 controller 10项反例最初 **1 passed/9 failed**；原实现没调
  preparation，未 Pair/Share 就 load，也缺 refresh/confirm/visibility 接线。
- 新 UI 测试初版缺 jsdom 注释/导入先后错误，属于测试配置错误，不当作生产断言证明。
  按既有 taskUiTestSupport 的注册顺序修正，没有修改授权合同。
- `existing-visibility-red-1.txt`：配置修正后真实 UI **1 passed/18 failed**。Showing 把
  正在返回的初始 metadata 当作退休，配对/准备状态不显示；只在 Hide 增 visibility epoch
  后，同 controller/UI **29 passed**。不能把显示和隐藏都算作退休。
- 定向 controller/API **42 passed**；UI19含15locale、原nonce确认、准备失败重试、Stop
  超越迟到准备、过期代码隐藏。定向集合重叠，不相加为总验证数。
- 前一候选全量：**705文件 /8062 passed /0 failed**，196.74秒，见
  `existing-ui-full-1.txt`。typecheck、fixture typecheck、全量 lint、quality、diff 通过。
- 第一次隔离 Chromium **115场景通过**（其中36为新增ExistingTab，全部为synthetic IPC），
  完整 UI build通过，但人工查看截图发现复制按钮被窄屏输入框挤出。新增内层body/
  复制按钮位置断言后第二次实际失败：300px时body宽202/scrollWidth236，按钮x251→285，
  超出modal右边276。`existing-ui-chromium-2.txt`和原failure保留，不改外壳断言冒充通过。
  修复共享字段的min-width/width后重跑最终候选，并将英文数量文案改为无单复数歧义的
  标签。最终同manifest4源码全量 **705文件/8062 passed/0 failed**，216.19秒，见
  `existing-ui-full-2.txt`；完整UI build/typecheck及全量lint通过，见build/lint-2。
  最终同source Chromium **115场景通过**（含36项新增ExistingTab、原79项保留），
  15locale/深浅色/300px窄屏/1.5x/键盘Escape与原trigger焦点/显式配对/空结果重试/
  忙和版本/失败重试/Stop及Cancel迟到准备都通过。新增复制按钮及内层body断言通过，
  英文浅色、中文深色截图再次人工复看，按钮在可视区域内；地址文本在input内可滚动/
  选中，复制字段保留完整值。见 `existing-ui-chromium-3.txt` 和
  `.run/task-consent-ui-2026-10-04T04-23-10-213Z/report.json`。
  这些仍是synthetic IPC，不替代原App/ACP/MV3或真人权限；Vite既有大chunk warning保留。
- 本批 Rust/extension/MCP执行逻辑未改，没有重跑它们，不将 E4a.1 的 Core1068/1084、
  默认App CU280/ACP177/routing38、Node28或strict算成本批实测。默认App旧 binary只属于
  上一后端候选，不宣称已经嵌入本批UI或安装态交付。

最终3177源 `source-files-4.sha256`：
`06a9dd6285a5f37b2e153fde9ff71a94475b12d22473a9ef7f41798a31d9a7b5`。
本批新增6个源文件；manifest3属于复制字段修复前候选，旧manifest2/3不覆盖。

## 下一步 E4b.2 → E5，不重做后端

1. 提供安装包内匹配的扩展获取/导出入口，使用固定 Host-owned bundled allowlist，用户
   明确点击才写独立App-owned目录；不下载未知zip、不覆盖不受管文件、不装入日常profile。
   正确展示扩展版本/安装ID和 Chrome/Edge 手动安装步骤，15locale/失败/取消路径齐全。
2. 补真实UI准备→Share撤销/空→重新Share→同picker retry，以及可见性/owner移交/键盘
   焦点/缩放联合验收；专门核对background metadata读取时disabled控件对键盘焦点及
   复制反馈的影响，不把synthetic快速回复当作真实延迟的UX证明。
   安装入口及这些路径完成后，才修改原 compiled descriptor 与测试；
   不改变默认feature、raw parser、模型授权面或既有权限要求。
3. E5用同最终source MV3 + authenticated HTTP + 原 App/ACP publication + **新 TaskGrant**
   验证。隔离UI mock、后端synthetic peer及 legacy `mcp-open` grant 都不能替代这条链。
4. 原自然关闭21530ms长尾及原root保持开放/保留；本批未改或重跑managed cleanup。
   U00–U09/D00–D31、四surface、Windows/macOS/Linux、全部输入/恢复、签名安装、真人
   Grok、同候选12h和完整UI/UX仍开放，不缩减范围或因局部绿标可交付。

## 构建卫生

本批无Rust重编译或新incremental代次。三轮隔离UI runner的原 browser/server owners
退出后，各自精确清理自己的 `site` / `browser-runtime`，共 **1118038337 bytes，约1.04GiB**。
三份 `cleanup.json`/`build-artifacts.json` 原样保留；已逐一读回 ownersClosed=true、两个
生成目录不存在。包括失败轮次也正常收尾，但失败json、日志、截图、源码hash没有删除。
删除未进回收站，按原 `pnpm exec node tools/computer-use-probe/ui-task-consent-acceptance.mjs`
可重新生成。没有清空target/node_modules/workspace，没有杀无关进程。
源码/依赖/immutable seed、最终当前 `dist`、默认App/Core缓存全部保留；3177源manifest4
清理后逐项复验不变。默认App artifact7仍为上一后端候选，回读hash仍是
`7eafc46682ca8678a97c48f493abf9198feade814844d0fb553d4fd6225f80f1`。
所有本批工具session均已终态；旧原自然长尾root未动，已执行清单不可重复使用。
