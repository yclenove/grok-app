# M0：Preview 2.0 与任务工作台设计

状态：可开发规格，未实现。对应[开发计划](../../plans/2026-09-05-browser-rearchitecture/m0-preview.md)。遵循[共用合同](00-contracts.md)和[总体设计](../2026-09-04-browser-rearchitecture-design.md)。

## 交付目标

在现有右侧工作台中交付稳定的浏览器任务组、完整手动导航和可靠原生 WebView 生命周期，为 Managed 接入提供 Host 领域模型。M0 完成后普通 URL/localhost 与现有 Design Mode 可连续使用；Managed 状态只显示未启用及真实原因，不提供伪造的 Agent 控制。

不在本包启动 Chromium，不开发 Chrome 扩展，不复制登录态，不改 CLI permission enum。M0 Runtime 可行性是独立并行工作包。

## 当前实现与迁移入口

| 文件 | 已存在行为 | 本包处理 |
| --- | --- | --- |
| `src/lib/sideWorkbench.ts` | browser 行只有 id/url/title/name，最多 24 个侧栏 tab | 增加可选 browserSessionId，旧数据通过显式迁移转换 |
| `src/hooks/useSideWorkbenchProjectIsolation.ts` | 以项目为 key 的内存 tab stash | 保留非浏览器行，浏览器引用改由 AppSession owner 选择 |
| `src/components/side-workbench/BrowserTab.tsx` | URL 草稿、SSH 准备、loading 和 Design Mode 本地状态 | 收缩为 BrowserWorkbench 的兼容装配入口 |
| `src/components/EmbeddedBrowser.tsx` | 1,070 行，含 create/close/bounds/cover/download/reload | 按生命周期与几何同步拆分，保留 Tauri 原生 surface |
| `src-tauri/src/side_browser_host.rs` | create/navigate/reload/url/eval/snapshot、page-load 回调、按 URL 存下载 | Preview adapter 接入；下载迁移唯一 transfer ID |
| `src-tauri/src/commands/terminal.rs` | `side_browser_*` 命令 | 兼容命令转发新 Preview service，新业务命令独立模块 |
| `src/lib/nativeWebviewCover.ts` | 计数型临时遮挡和浮动矩形排除 | 原样复用语义，覆盖释放与多菜单竞态 |

当前 `SideWorkbench.tsx` 是通用多类型工作台，不把它改成 Browser 专用壳。新浏览器任务组在 browser 类型页签内展示，file/review/terminal/skills/plan 保持各自路径。

## 组件结构

```text
SideWorkbench / SideTabBody
  -> BrowserTab compatibility entry
  -> BrowserProvider (current AppSession projection)
  -> BrowserWorkbench
       -> BrowserGroupStrip
       -> BrowserToolbar
       -> BrowserStateView
       -> PreviewSurface
            -> usePreviewLifecycle
            -> usePreviewBounds
       -> existing BrowserDesignModePanel
```

Provider 使用 `useSyncExternalStore` 订阅领域 store。Host 持有 Session/Tab 与真实导航；React 仅持有地址输入草稿、菜单、焦点和布局。新的 `src/lib/api/browser.ts` 提供类型化 invoke/event adapter，由既有 barrel 导出；不向 App.tsx/AppWorkbench.tsx 增加 `useState`。

`BrowserSessionProjection` 含 ownerAppSessionId、Session UUID、标题、当前 tab、tab 顺序、backend、ProfileRef、connection state、capabilities 与 revision。UI 无权直接改 Host owner。

## 用户工作流

1. 从普通链接、侧栏加号或现有 Browser 入口打开，Host 检查当前 AppSession 与 URL，创建或复用其 BrowserSession。
2. 当前组内打开标签，地址栏立刻显示导航草稿并进入请求态；最终地址、标题和历史可用性来自 Host callback。
3. 同一 AppSession 可创建多个组；切换组只切投影，后台存在的标签仍有明确 owner。
4. 隐藏右侧 pane 暂停绘制/尺寸监听，保持任务；显式“关闭任务组”进入 closing 并释放该组资源。
5. AppSession 归档时 suspend/fence；重新打开显示恢复上下文，不自动控制。删除先清理浏览器资源，再让会话删除完成。

用户自定义组名只影响展示，Host UUID 不变。相同 URL 可以拥有不同 tab；只在用户显式要求复用当前 tab 时复用，不能把 URL 去重当全局唯一性。

## 导航合同

| 操作 | 行为 | 特殊路径 |
| --- | --- | --- |
| 地址 Enter | 规范化 HTTP(S) URL，再请求导航 | IME composing/keyCode 229 时只提交候选字 |
| Enter 当前 committed URL | 真正 reload | 不重建 WebView、不清历史 |
| 刷新 | 刷新 committed URL | 草稿编辑中不导航到未提交草稿 |
| 停止 | 调用 backend stop，更新真实状态 | 不把“停止 loading 动画”视为停止请求 |
| 后退/前进 | 原生历史 API + 当前可用性 | 不可用时禁用，未知能力不假造历史 |
| 页面内部跳转/重定向 | Host 更新 URL/title/navRevision | 旧导航回调不得覆盖较新 committed 页面 |
| 新窗口/popup | 先创建 Host tab/owner 再加载 | 被拒绝时显示可解释状态，不能出现无 owner WebView |
| SSH localhost | 复用 `sshBrowserPrepare` | 失败时保留原地址与错误，不意外打开本机同端口 |

Tauri 通用层没有可靠 back/forward/stop/title 回调时，在 `browser/preview/platform/` 实现 WKWebView、WebView2 的原生 adapter。WebKitGTK 的既有预览保持可用；其新增行为只能在对应 Linux fixture 通过后声明支持。此处不使用页面 `history.length` 判断完整原生历史，也不开放任意 eval 做控制后门。

原生 hook 与 delegate 必须按已绑定 generation 注册/注销。UI 关闭、快速切换、StrictMode 重挂载采用既有延迟关闭去重语义，最终显式关闭仍由 Host 处理，不让 React cleanup 误杀另一个任务。

## 状态与交互

| 状态 | 内容/动作 | 焦点与键盘 |
| --- | --- | --- |
| empty | 当前任务无标签，可新建/输入地址 | 初始焦点地址栏 |
| starting/loading | 紧凑忙状态、停止按钮、真实 URL | 导航期间地址栏可编辑 |
| ready | tab、后退/前进、刷新、Design Mode | 支持 tab strip roving focus |
| error/offline/TLS | 本地化原因、重试、复制地址/外部打开 | 不自动接受 TLS 风险 |
| unsupported | 具体能力不可用、可用后端菜单 | 不出现无效果的启用按钮 |
| closing | 关闭按钮 busy，拒绝新导航 | 失败恢复可操作状态并提示 |
| detached | 不绘制原生 surface，保留 metadata | 回到 pane 后恢复尺寸再显示 |

按钮使用已有图标/tooltip，菜单使用 Select/ContextMenu。工具栏紧凑、可换行，390px 时缩减非核心入口至菜单；地址栏占剩余空间，长 URL 单行省略。状态文案只说明当前状态/失败原因，不在主界面展示实现教程。

焦点与快捷键接入现有 shortcut registry，沿用 Cmd/Ctrl+W 关闭当前 tab 的路径；输入框编辑时不抢全局快捷键。菜单打开申请 native-cover，关闭/异常卸载都释放；native-cover 引用数不可负，modal 必须实际盖住原生内容。

## 数据迁移

旧 SideTab browser 行新增可选 `browserSessionId`，非浏览器行序列化不变。Host 用 `(ownerAppSessionId, legacySideTabId, migrationVersion)` 生成幂等迁移记录，新 UUID 单独保存。第一次成功 checkpoint 后 UI 才替换引用；磁盘失败保留旧记录与可重试状态。

旧行没有可证明的 AppSession owner 时，由当前用户打开该行触发认领，不在后台随机分配。无项目会话使用 AppSession 身份下的 ephemeral partition；项目移动不改变 Profile ID。兼容旧版本所需原始字段留到 M1 稳定后的清理任务，不直接删除用户历史。

Preview WebView partition 是否能跨平台按 Session 隔离必须真实探测。系统 WebView 当前共享 Cookie 的事实不能因任务分组而宣称已隔离；UI 只显示已验证的 Preview partition 能力。Preview 与 Managed 的隔离由后端分开保证，绝不复制 Cookie。

## 生命周期与 AppSession

归档、批量归档、删除、fork、rewind 都经 `browser/lifecycle.rs` 的同一入口。删除不能只卸载 React。Fork 默认新空 BrowserSession，不继承 tabs/grants/lease；rewind 只提示外部网页状态不能倒回，不复活历史 binding。批量操作逐 owner 清理，部分失败显示确切失败组。

`src-tauri/src/commands/session_p1.rs` 当前 1,178 行；把本次需要修改的 rewind/fork/archive/delete 路由抽到 `commands/session_lifecycle.rs`，从 façade 重新导出，保持 invoke 名称不变。与 EmbeddedBrowser 拆分合计目标是至少减少两个千行文件，不上调质量预算。

删除协调不能只放在 Tauri commands：`ssh_remote/fs_sessions.rs` 的已导入会话删除、`automation_runner.rs` 的失败清理也直接调用 `store::delete_session`。新增 `src-tauri/src/session_lifecycle.rs` 作为所有生产删除路径的协调服务，先对所属 BrowserSession fence/清理，再取得绑定 owner/revision 的删除许可；store 的最终删除校验该许可，防止新增调用绕过。自动化创建失败且尚无 BrowserSession 的分支也要由同一服务证明空引用，不能自行跳过。SSH 删除、批量删除、自动化失败和清理期间新建 tab 均纳入竞争测试。

## Preview 下载与 Design Mode

下载首先修复 URL key 冲突：由 downloadId 和 backend transfer binding 关联 start/finish，同 URL 两次下载不能覆盖彼此。现有 Tauri `DownloadEvent::Finished` 只有 URL/path/success，macOS path 可为 None，不能凭该回调补造 transfer identity。平台 adapter 在开始时保有真实下载对象：macOS `WKDownload`、Windows `ICoreWebView2DownloadOperation`，并映射到 `(bindingId, tabId, bindingGeneration, nativeTransferToken)`；完成/取消时使用同一对象，URL 仅是 metadata。native token 仅在受持有对象的生命周期内有效，释放后不复用旧映射。

原生接入须证明与现有 Wry delegate、导航、blob 分发兼容；若当前 hook 不足，任务内提供最小版本固定的 Wry/Tauri 适配补丁与维护测试，不能偷偷替换 delegate 后破坏其他行为。WebKitGTK 的新增下载能力同样以 `WebKitDownload` 对象证据为门。无法取得身份时该新增能力验收失败，不能按 URL/FIFO 猜完成归属。M0 保留现有用户保存流程，M1-R05 统一到 Transfer Broker；数据/blob 的 `side_browser_blob.rs` 路径保留独立 token 回归 fixture，不扩展页面取得 IPC 的权限。

现有 Design Mode 的选区、注释和发到 Composer 保持可用，拆出的 hook 继续受 local/injectable URL 限制。合成截图不得称为真实截图；M2 才迁移到 Managed capture。点击“发送”失败要保存用户注释草稿，不能清空后悄悄失败。

## 验收与发布

本包任务为 M0-W01 至 M0-W06，交付后才开放 `browser.previewV2`。M0 Runtime 可行性未通过不妨碍 Preview 2.0 内部验收，但 M1 开始受双方门约束。

验收涵盖重复迁移、同 URL 多 tab、多 AppSession 隔离、真实地址/标题/历史、停止、popup、网络/TLS 失败、下载同 URL、blob 导出、Design Mode、归档/删除/fork/rewind、快速拖拽/显隐、modal cover、十五语言 key parity、五语言压力和键盘。macOS/Windows 真实 WebView 证据独立于浏览器模拟 UI 的 Playwright 截图。

测试目录为 `src/components/browser/**/*.test.ts(x)`、`src-tauri/src/browser/preview/tests.rs`、`tests/browser-preview/`。截图至少记录 1440x900 与 390x844 视口，且检查文字溢出、点击穿透、原生画面非空与实际导航。证据写入 `docs/qa/browser-rearchitecture/m0-preview/`，开发前状态 `not_run`。

回滚关闭新建入口，已有 v1 metadata 保留；旧 Preview 兼容入口只打开明确关联的 tab，不能重新创建重复 Session。清理新缓存不能删除用户 Profile/导出文件。若新旧数据双写不一致，优先 Host manifest 并报告修复入口。
