# macOS 模型观察事务与 Unicode 语义赋值检查点

本地终端记录：2026-09-27，Asia/Shanghai（+08:00；对应 UTC 2026-09-26）。
接续 [窗口内 AX 树与精确语义点击](2026-09-27-computer-use-macos-ax-tree-checkpoint.md)。
完整目标“接手 grok 的工作，完成 computer use 所有功能开发，推进到最终版”仍为 **active**。
上一轮与本轮均为 **progress**：依据当前源码、具体反例、实际修改和终态测试；不是状态重述。
没有 commit/push/PR，没有替换/关闭用户 App、修改其运行时或授予系统权限。R17 UI/UX 保留。

## 1. 图片与 AX 共用一次发布

- 移除彼此独立的 FrameRegistry 和 AX Snapshots，改为
  `computer-use-core/src/quartz_frame/observations.rs` 的单一 ObservationRegistry。
  一个 ticket、一个 Arc 同时持有截图元数据、是否授予坐标权限和保留的 AX 树。
- 新模型观察开始即撤销旧观察；最多八个已发布或待发布条目，淘汰不会留下另一种输入权限。
  同 run/target 替换、Stop、目标释放、窗口实例退休均撤销整份观察。
  旧生产者不能覆盖新 ticket，失败/迟到的读取不能重新授予权限。
- 已载入 Arc 仅保护内存，不代表授权仍有效。坐标新按下前、双击每次按下前和 AX 写入前核对
  当前观察；撤销后允许按原窗口清理已按下的鼠标键，但禁止第二次新按下。
  原窗口无法核实的释放仍保持占用，不向替代窗口释放。
- 原生采集、AX 调用和 payload 析构不在注册表锁内执行。九项新纯 Rust 回归涵盖真实线程的
  迟到生产者/Stop、持有 payload 锁时退休、精确作用域、八条容量与析构锁边界。
- 生产 FFI 回归进一步验证：坐标配置中被无图模型观察替换，以及 AX 校验中替换观察失败，
  都不能复用已载入的旧图片/节点；随后新的合法观察仍可操作。

## 2. 真正的 AX Unicode 赋值调用链

- 对 `AXTextField`、`AXTextArea`、`AXComboBox`，只有可交互且
  `AXUIElementIsAttributeSettable(AXValue)` 明确返回可写才发布 `set_value`。
  不把 AXSlider 等任意可写属性伪装成文本；安全角色/子角色在查询前排除。
- `ax_api.rs` 预先分配完整 CFString，再通过 `ax_tree.rs` 现有精确引用、祖先、窗口、
  快照和几何校验，调用一次 `AXUIElementSetAttributeValue`。没有点击聚焦、键盘、剪贴板或坐标兜底。
- 支持空字符串清空、中文、emoji、组合字符和协议规定的最多 4000 个字符；拒绝 NUL、
  缺失/错误类型及额外参数，不截断、不忽略参数、不读取旧 AXValue、不在结果中回显输入文本。
- 分配文本后及最后写入前重查权限、角色/安全子角色、隐藏/启用、可写能力、窗口/显示几何、
  取消和当前观察。只读快照不能因为控件后来变为可写而自行取得写权限。
- API 返回非零按完成未知处理，维持原生占用，不自动重试；Stop 不伪造完成。
  成功也只返回 applied/unverified，不声称业务结果已验证。取消发生在成功返回期间不会重放。
- 能力表新增 semantic set_value 和 Unicode replacement；`type_text`、键盘、滚动、拖拽、
  Chinese IME 仍未冒充实现。新增 `macos_value_contract` 并接入非 macOS CI 替身步骤。

## 3. 失败与最终验证

事务证据目录：`tools/computer-use-probe/.run/macos-observation-transaction-20260927/`。

- `observation-boundary-red.log`：30 passed / 3 failed，分别为淘汰后的旧坐标、
  事件配置中撤销后的旧帧、首次按下后撤销仍发生第二次按下。修复未放宽这些断言。
- `observation-registry-contract.log`：25/25（包括九项新注册表测试）。
  `observation-replacement-contract.log`：AX 27/27 + 截图 33/33。
- 事务阶段冻结 159 个文件，`observation-acceptance-final.log` 终态 Core 544/544
  （111.87 秒）+ AX 27/27 + 截图 33/33；该阶段 Clippy/格式通过、源指纹零漂移。
  随后继续开发赋值功能，故这些是中间阶段证据，不冒充最终赋值源码的冻结结果。

赋值与本轮最终证据目录：`tools/computer-use-probe/.run/macos-ax-value-20260927/`。

- 初始 `value-contract-red.log` 保留测试夹具的 TLS 析构异常：未调用的 hook 强持有适配器。
  改为弱引用并检查 hook 确实执行；没有删断言或忽略失败。
  `value-contract-red-harness-fixed.log` 得到可解释的 **4 passed / 7 failed**。
- 接入生产调用链后 `value-contract-first-implementation.log` 为 11/11 + AX 27/27 + 截图 33/33；
  追加可写性查询中变化、只读快照升级、取消/引用/元数据校验，`value-boundary-contract.log` 14/14。
- 最终 `value-acceptance-final.log` 无名称过滤、串行 **Core 544/544（119.64 秒）+
  AX 27/27 + 截图 33/33 + 赋值 14/14 = 618/618**，终态 exit 0。
  不把多轮重复运行或一个测试内的循环累加为独立用例。
- `value-clippy-final.log` strict all-targets `-D warnings`、`value-format-final.log` scoped rustfmt 均 exit 0。
  `source-before-final.json` / `source-after-final.json` 冻结并复核 **160 个源码/测试/依赖/CI 文件，0 差异**。
  使用独立 Windows current seed，没有改用户运行时目录。完整来源/日志/文档指纹见 `source-receipt.json`。
- 已实际读取 Apple 官方 DocC IsAttributeSettable/SetAttributeValue 的声明、参数和错误说明，
  两份原始 JSON 与来源哈希保存在该目录。API 文档不是 Mac 原生成功证明。

**618 项证据是 Windows Core 与生产 Rust 适配器链接 AX/Quartz FFI 替身；不是 Mac 真机或安装版验收。**
纯 Rust 线程测试只证明注册表并发边界，替身回调不证明原生 AX/CF 跨线程生命周期、SDK ABI、系统超时
或物理输入效果。注册表原子发布不意味着原生采集/查询/写入原子化，也没有消除所有系统 TOCTOU。

## 4. 全目标仍未通过完成审计

1. macOS arm64/x64 原生编译、签名、权限、窗口/控件销毁重用、线程/超时与业务结果验收仍待完成。
   完全遮挡、AX 不支持/自定义控件等场景没有从最终产品范围删去。
2. 完整 TypeText/追加/键盘/滚动/拖拽/IME、ScreenCaptureKit 与真实原生取消/恢复继续；
   未知 AXPress/AXSetValue 仍保留占用，没有把“重建适配器”当作恢复证明。
3. Windows/浏览器退出长尾、WebView 运行中取消、X11 全输入/恢复、GNOME 原生 Wayland、
   Desktop/managed browser/existing tabs/App WebView 与 App/ACP/MCP 全链、正式扩展、
   签名安装/升级/回滚仍要完成；本轮未声明这些通过。
4. UI/UX 全面重设计的原生窄窗/缩放/权限/交互矩阵、真实 Grok E4、冻结 12 小时 active soak
   仍待验收。底层绿色回归不能替代完整最终版；目标保持 active，不调用 complete/blocked/paused。

下一步基于已统一的观察与精确语义写入推进剩余输入和原生完成/恢复，保留所有平台/UI/安装/模型门禁。
