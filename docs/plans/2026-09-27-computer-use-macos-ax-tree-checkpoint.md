# macOS 窗口内 AX 树与精确语义点击检查点

本机时间：2026-09-27，Asia/Shanghai（终端时钟核对）。
接续 [AX 窗口实例绑定](2026-09-27-computer-use-macos-ax-window-checkpoint.md)。
完整目标“接手 grok 的工作，完成 computer use 所有功能开发，推进到最终版”仍为 **active**。
上一轮属于 **progress**：存在生产源码和终态测试证据。本轮再次读取工作树后实作，
不是重述计划或等待旧会话。没有 commit/push/PR，没有替换或关闭用户 App、授予系统权限。
R17 的 UI/UX 源码保留；本轮没有把底层测试当作界面验收。

## 本轮实际实现

- 从 `ax_window.rs` 抽出 `ax_api.rs`，统一 AX 类型、PID、字符串/布尔/几何类型校验、
  每个元素代理的消息 timeout、两秒查询预算、取消检查与 Create/Copy/Retain 的释放。
  采用公开的子项计数和分段复制接口，不先无界复制全部子项。
- 新 `ax_tree.rs` 从已保留的真实窗口代理遍历：最多 256 个节点、32 层，检测重复/循环，
  校验每个子项的 `AXParent`、`AXWindow` 和进程身份。树被截断时显式标记。
  标题/描述保留 Unicode，输出名称最多 512 字符；原生字符串超过 8192 UTF-16 单元则拒绝。
- 在读取名称、动作和子项前排除安全文本角色/子角色；不读取任何 `AXValue`。
  几何裁剪到授权窗口并映射为截图像素；零尺寸控件可读取语义但没有点击能力，
  非有限/负尺寸不被悄悄转换为正常值。隐藏容器不暴露可操作的后代。
- 模型 AX 快照按 run、完整窗口实例 ID、snapshot 和 geometry 绑定，引用为随机 nonce。
  原生元素对象被保留，而不是根据标题/位置重新查找替身；最多保留八个快照。
  预览引用不能取得/覆盖模型权限；无图片模型观察只授予语义引用，不授予坐标输入权限。
  失败的新模型观察、Stop、目标释放及缓存淘汰都撤销对应旧引用，迟到读取不能重新发布。
- 单次左键语义点击真正调用 `AXUIElementPerformAction(AXPress)`。派发前重查保留对象、
  完整祖先关系、角色/名称/边界、隐藏/启用状态和动作能力，再核对当前窗口/显示几何、
  进程身份、快照是否仍有效以及取消/查询期限。绝不替换为 CG 坐标点击。
- 非零 AXPress 返回按结果未知处理，保持原生占用，不自动重试；Stop 不能伪造完成。
  返回成功也只报告已派发，不声称业务后置条件验证成功。文本/按键/滚动/拖拽尚未开放。
- CI 的非 macOS 替身回归步骤增加 `macos_ax_contract`；不是远端 CI 已运行的声明。

## 先失败再修复

证据目录：`tools/computer-use-probe/.run/macos-ax-tree-20260927/`。

1. 初始树测试和首次编译日志保留为 `ax-tree-red.log`、`ax-tree-first-compile.log`。
   原截图测试中“没有 AX 节点/能力”和只保留一个 AX 代理的断言已经过时，
   更新为真实根节点/能力及新增模型根代理数量；销毁适配器后零残留检查没有删除。
2. `ax-tree-contract-red.log`：**18 passed / 1 failed**，合法零尺寸子项导致整次观察失败。
   修复后 `ax-tree-contract-green.log`：**AX 19/19 + 截图 30/30**。
3. `ax-tree-revocation-red.log`：**23 passed / 2 failed**，隐藏父容器仍允许子项点击；
   目标释放发生在 AX 校验中时，已载入的 Arc 仍被当成有效授权。
   修复祖先隐藏检查、隐藏子树裁剪和派发前当前快照核对后，
   `ax-tree-revocation-green.log`：**AX 25/25 + 截图 30/30**，新反例断言未放宽。

## 冻结源码最终验证

- `ax-tree-acceptance-final.log`：无名称过滤，串行 **Core 535/535（112.38 秒）+
  AX 25/25 + 截图 30/30**，终态 exit 0。共 590 项，不把多轮重复运行累加为独立用例。
- `ax-tree-clippy-final.log`：Core strict all-targets Clippy `-D warnings`，exit 0。
  `ax-tree-format-final.log`：本轮生产模块及测试的 scoped rustfmt，exit 0。
- `source-before-final.json` 记录测试前 **158 个源码、测试、依赖清单和 CI 文件** SHA-256；
  `source-receipt.json` 保存最终核对及测试/API 文档/本检查点指纹。
  使用独立的 Windows current seed，没有修改用户运行时目录。
- 新替身用例覆盖嵌套祖先、80 层截断、百万子项数量上限、循环、Unicode/截断/过长字符串、
  零/负/非有限尺寸、同形同名控件替换、跨 run/目标/快照/引用、预览隔离、权限撤回、
  Stop 在读取中发生且迟到发布被拒绝、读取中窗口替换、未知动作结果占用、禁止语义兜底。
  每项新测试最后显式销毁适配器并检查 CF/AX 所有权计数归零、未读取受保护内容。

**这是 Windows 上生产 Rust 适配器链接确定性 AX/Quartz FFI 替身的证据，不是 macOS 真机验收。**
替身 Retain 使用独立测试对象模拟保留，线程内回调模拟撤销边界，不证明原生对象唯一性、
跨线程行为、真实系统超时、Mac 链接、权限 UI、原生物理效果或所有 TOCTOU 已消除。

## 接口依据与证据限制

已读取 Apple 官方 DocC 的 CopyActionNames、CopyAttributeValues、GetAttributeValueCount
和 PerformAction 声明、参数、返回值与讨论；原始 JSON 和来源/哈希见
`api-source-receipt.json`。尤其 CannotComplete 不能视为动作未发生，因此不使用文档中
一般性重试建议去重放用户动作。接口文档本身不是原生完成/恢复的验收证据。

## 仍须推进的完整范围

1. macOS arm64/x64 真机编译、签名、权限、真实 AX 生命周期/销毁/重用、线程与超时验证。
   当前仍要求可核实的暴露窗口；AX 不支持和完全遮挡的产品场景没有被从最终范围删除。
2. 完整文本、按键、滚动、拖拽/IME，ScreenCaptureKit，真实原生派发/完成/取消/恢复。
   AX 查询与派发不是原子操作；图片和 AX 的双注册表发布仍须加强事务性/并发验收。
   未知 AXPress 的真实恢复尚未实现，不能把重建适配器当作恢复证明。
3. Windows/浏览器退出长尾、WebView 运行中取消、X11 完整输入与恢复、GNOME 原生 Wayland、
   正式扩展、App/ACP/MCP 全链与签名安装/升级/回滚继续。
4. UI/UX 全面重设计的原生窄窗/缩放/权限/交互矩阵、真实 Grok E4 与冻结 12 小时 active
   soak 继续。590 项回归通过不能代替这些要求；没有通过最终完成审计，不更新为 complete。

下一步以当前保留对象/精确引用为基础推进完整输入和原生验收，同时保留上述事务性、
生命周期与 UI 交付项，不回退为根节点占位、同名替身或全桌面兜底。
