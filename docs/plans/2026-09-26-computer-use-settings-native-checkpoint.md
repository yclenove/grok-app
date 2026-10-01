# Computer Use：R15 原生设置复查与集成修复

日期：2026-09-26，本机 Asia/Shanghai。完整目标仍为最终版 Computer Use，
包含 UI/UX 整体重设计；不是完成设置页就宣布全部完成。未 commit/push/PR。

## 原生证据及其边界

独立冻结源码位于 `H:/aicoding/cu-native-r15-20260926/source`。
3460 个文件逐项 SHA256 核对；独立应用标识、全新 App/agent home，CU 保持关闭，
没有复制用户账户、会话、授权或密钥。现有用户 App 未替换或关闭。

`pnpm exec tauri build --debug --no-bundle --config src-tauri/tauri.review-r15.conf.json`
实际 exit 0：前端构建、准备/导入检查、资源审计和 Rust 原生构建均完成。
首轮 pnpm 参数转发错误的 `native-build.log` 保留；成功的是 `native-build-r2.log`。
资源审计 444 files、0 hits。exe SHA256：
`4399F1C57ECA8F4830C87E542DA0BCBA6B1E5E4B317D9718C9968EE244E5F610`。

确实通过原生 WebView2 窗口完成了：跳过导览、进入设置→扩展→使用电脑、
展开维护、打开操作记录清理确认框、Tab 焦点在框内循环、Escape 关闭并返回触发按钮、
窗口从 1202×802 调整到 894×802、切换浅色后重新打开 CU 设置。
AX 的 focused_element 只报告文档根；焦点环是截图观察，不能冒充精确 AX 焦点证据。
第一次直接拖右下角未改变尺寸，后续使用原生 Size 操作成功；不把首次无效动作写成成功。

未执行启用 CU、目标授权、确认删除、修复/安装、发送模型任务或登录。
新 App home 内缺少私有运行时的警告是真实状态，不能误写成打包资源缺失。
这不是签名安装版、真实控制、权限弹窗、OS 缩放或跨平台验收。

证据根目录：`tools/computer-use-probe/.run/ui-native-r15-20260926/`。
`native-launch.json` 记录原始 PID 44264、精确 exe、时间、独立 home；
`native-observations.json` 保存去掉图像载荷的原生观察记录。
使用窗口前仍需重新核验归属，旧 PID 本身不构成操作权限。

## 发现与实际修复

1. **双重卡片**：真实 ExtensionsPanel 外壳和 CU 组件各有 settings-card，
   单组件夹具此前漏掉这一集成问题。CU 外壳现在仅承担布局，组件保留自己的卡片；
   MCP 等相邻 tab 的原样式不变。新增测试渲染真正的 ExtensionsPanel，而非复制外壳。
2. **操作记录文案**：将“清除 traces”改为“清除操作记录”，15 个 catalog 同步，
   按钮与确认框共享同一 key。逐语言测试取消后不调用任何删除接口。
   不因此宣称所有历史 CU 文案已经完成人工翻译复核。
3. **读取卡死**：设置/运行时读取增加 10 秒只读展示期限。
   未读全状态前不开启切换或运行时修复；用户显式重试，过期成功/失败均不能覆盖新读取。
   StrictMode cleanup 退役自身读取，unmount 清除自身计时器。
   期限不作用于修复/删除等 mutation，不释放未完成动作的锁，不自动重放动作。
4. **确认框透字**：编译截图显示大字号维护文字从半透明弹窗底部透出。
   CU 维护确认框改用既有 `--bg-elevated` 实心材质，仍使用 GlassModal 的焦点与遮罩机制，
   不修改全局弹窗皮肤。浏览器验收新增实心 alpha=255、opacity=1 断言。

## 回归记录

- 双卡片/文案红测试：`ui-polish-red.log`，17 failed / 6 passed。
- 第一轮修复：`ui-polish-green.log`，65/65。
- 只读超时红测试：`ui-settings-loading-red.log`，7 failed / 2 passed；
  修复后 `ui-settings-loading-green.log`，74/74（包括 9 个读取/生命周期场景）。
- 当前 UI/domain/相邻功能/catalog 联合回归：`ui-polish-final-unit.log`，321/321、30 files。
- 第一轮类型检查因测试错误使用 `getByRole` 的 `exact` 参数失败，日志保留；
  删除测试中的无效参数，之后 `ui-polish-typecheck-r2.log` 和
  `ui-polish-fixture-typecheck-r2.log` 均 exit 0。
- 第一轮编译浏览器夹具：`ui-polish-browser.log`，83/83、terminal exit 0；
  artifact `ui-redesign-2026-09-26T03-30-40-838Z`。
  覆盖真实 ExtensionsPanel 外壳、窄中文、浅色英文/德文、200% Tamil 和取消确认。
  此轮早于读取期限与实心弹窗修复，不是最后源码的整体验收。
- 最后源码：`ui-polish-final-typecheck.log`、`ui-polish-final-fixture-typecheck.log`、
  `ui-polish-final-lint.log` 全部 exit 0。
- `ui-polish-final-browser.log`：**85/85，terminal exit 0**，artifact
  `ui-redesign-2026-09-26T03-40-45-971Z`。包含 6 个真实扩展页外壳场景，
  其中 2 个使用实际 10 秒计时，验证设置/运行时读取超时、手动重试和旧回包不能提前解锁。
  所有页面无 page error，实心材质断言通过；最终浅色、窄中文、200% Tamil 确认框、
  中文超时截图已视觉复查，未出现前轮的透字。
- `ui-polish-source-receipt.json` 记录 25 项源文件/15 个语言 catalog 的 SHA256。
  所有这些输入的修改时间早于该浏览器编译；旧 R15 原生 exe SHA256 仍完全相同。

两轮 UI probe 前后原 seed digest 均一致：
`c4430eb59a12744c3a7e8bea7334bc9a4a70bef40d99e4f9ed65fa9384bc88b2`。
所有浏览器夹具使用固定 Chromium 的独立可写副本、mock Host IPC；不等于原生控制。

**时间边界**：R15 原生 exe 早于本文件列出的修复。不能把旧 exe 的实机输入证据
算成修复后的原生验收；下一轮需要冻结新源码、独立构建并重验。旧源码与原生 exe 保持原样。

## 未关闭的最终版要求

Windows 原生退出长尾/发布拒绝访问、macOS AX 与签名资源交付、X11 完整原生动作、
GNOME Wayland Portal/PipeWire/libei、WebView 物理取消与关闭性能、正式 Chrome/Edge 扩展、
全平台安装/更新/回滚、UI 权限与缩放/跨平台验收、真实 Grok E4、冻结 12 小时 active soak
继续按原范围推进。局部绿色回归不代替这些缺失证据，目标保持 active。
