# macOS 指针输入与跨平台滚动合同检查点

终端日期：2026-09-27，Asia/Shanghai（+08:00；对应 UTC 2026-09-26）。
接续 [macOS 模型观察事务与 Unicode 赋值](2026-09-27-computer-use-macos-observation-value-checkpoint.md)。
完整目标“接手 grok 的工作，完成 computer use 所有功能开发，推进到最终版”仍为 **active**。
本轮分类 **progress**：实际增加生产输入调用链，复现并修复原生问题，执行终态回归；不是仅修改状态。
未 commit/push/PR，未替换或关闭用户 App、改动其运行时或授予系统权限。原有 R17 UI 保留。

## 1. macOS 新增滚动、拖拽和点击权限边界修复

- `macos_adapter/pointer.rs` 接入固定参数的 `CGEventCreateScrollWheelEvent2`，只向精确 PID 投递一个截图绑定的滚轮事件；无聚焦点击、鼠标瞬移、剪贴板或重复投递。
- 拖拽预分配一次按下、16 个插值移动以及每个可能停止点各自的释放，共 34 个事件。
  分配失败发生在任何输入之前；分配/配置后重查全部采样点，再在每次派发前检查当前观察、窗口身份、几何、遮挡、权限和取消。
- Stop/观察退休禁止后续新输入；已经按下时，只尝试在最后实际投递的点向原窗口释放，不跳到尚未经过的目的地。
  目标替换、权限丢失等使释放无法确认时保留占用，Stop 不把未知完成变成空闲。
- 修复原有点击的两项反例：配置事件期间丢失 Screen Recording 权限仍按下，以及按下后丢权限仍声称释放成功。
  新滚动/拖拽与点击共用最终点校验和清理边界。
- 能力只增加在 Screen Recording 与 AX 权限均可用时的 coordinate scroll/drag；不伪造 semantic scroll/drag、追加文本、键盘或 IME 支持。
- `CGEventPostToPid` 的无返回值/排队行为不是目标实际处理或物理清理完成的证明。结果保持 unverified。
  16 段路径采样不是连续无遮挡证明；真实 Quartz/AX 线程、SDK ABI、系统时序和恢复仍须 Mac 真机验收。

## 2. 统一方向，并用原生读回修复 Windows/X11

- 新 `protocol::ScrollDelta` 严格接受唯一整数 `delta`，范围 -2400..2400；不默认、不截断、不夹取、不忽略额外参数。
  保留既有 Choice/browser 约定：正数向下，负数向上。只在 Quartz/Win32 正数向上的边界反号。
- X11 改为正数 button 5、负数 button 4；原生红测确认此前方向相反。
  零滚动不移动指针、不发轮事件；进一步修复“无事件却 applied=true”的原生结果错误。
- Windows 修正 UIA 增/减方向及 WM_MOUSEWHEEL 高字符号；零滚动返回 applied=false。
  `windows-scroll` 用自建 Win32 listbox 的 `LB_GETTOPINDEX` 独立读回，分别覆盖 UIA/定向窗口消息的向下、零、向上。
  该门禁同时接入 `windows-native` 总入口，未把单独入口当作整个总门禁已通过。
- 严格 WPF oracle 揭露旧 S4.5 探针的虚假绿灯：旧参数 `dy` 无效、结果被忽略；改成合法 `delta` 后仍出现 Applied 但内容 0→0。
  根因是无名 ScrollViewer 被观察树过滤，而探针改选了不提供 ScrollPattern 的滚动条，落入无效坐标兜底。
- 生产观察改为依据真实可垂直滚动的 ScrollPattern 保留并发布无名容器；不再仅凭 list/listitem 角色声明 scroll。
  WPF 探针只选择明确 advertised scroll，要求存在无名容器、结果为 `windows uia scroll` 且独立文件读回 0→140。
  同时按精确标题和自建进程 PID 绑定窗口/对话框，每一步更新观察；不在整次操作后继续使用过期快照。
- 方向/零值合同不是跨应用像素距离完全一致的声明：Quartz 像素、X11/Win32 原生滚轮单位、UIA 页面量仍有原生语义差异。
  使用 `value()` 而非误导性的 `pixels()` 命名；完整产品幅度/行为一致性仍需需求和原生矩阵验收。

## 3. 可复核的失败与终态证据

证据目录：`tools/computer-use-probe/.run/macos-pointer-actions-20260927/`。

- `pointer-contract-red.log`：3 passed / 8 failed。最初实现的绿测还使用了错误的同符号假设；随后按既有 Choice/browser 合同修正，不能用最初绿测证明方向正确。
- `pointer-boundaries-red.log` 保留夹具把独立请求错误共享取消令牌、在 extern C 回调中断言导致的中止。
  修正夹具而非放宽断言后，`pointer-boundaries-red-harness-fixed.log` 得到 16 passed / 2 failed，明确为上述两项点击权限问题。
- `scroll-linux-direction-red.log` 保留实际 X11 方向失败；该次 shell 尾行另有 CRLF 退出错误，不能将其混为生产缺陷。
  `scroll-linux-zero-red.log` 保留 applied 标志错误；修复后二者均在真实 X server 事件读回通过。
- `scroll-windows-wpf-first.log` 保留返回 Applied/`windows sendinput` 但文件 0→0 的失败。
  `scroll-windows-wpf-pattern.log` 修复后真实文件 0→140，并完成该自建 WPF 的点击、中文赋值和对话框确认链。
- 最终 `core-final.log`：无名称过滤、串行 Core **547/547**（122.42 秒），AX **27/27**，截图 **33/33**，pointer **18/18**，value **14/14**，合计 **639/639**，exit 0。
  macOS 部分是 Windows 上链接生产 Rust 调用链的 FFI 替身，不是 Mac 原生验收。
- `x11-unit-final.log` **11/11**；`x11-native-final-{1,2,3}.log` 每轮 **17** 个原生坐标门禁，
  `x11-semantic-final-{1,2,3}.log` 每轮 **12** 个 GTK/AT-SPI 门禁，全部终态 exit 0。
  环境为 Debian WSL 中自建 Xvfb，明确不是 GNOME 原生 Wayland或安装版 App。
- `windows-build-final.log` 构建成功；保留一条 linker import-library 信息警告，不冒称构建无警告。
  `windows-scroll-final-{1,2,3}.log` 每轮六种方向/零值情况通过；UIA 列表 0→8→8→0、定向消息 0→6→6→0。
  `windows-wpf-final-{1,2,3}.log` 每轮无名容器与 WPF 整条夹具链通过。只有自建窗口，不是全应用/安装版矩阵。
- 严格 `-D warnings`：Windows Core all-targets、Linux Core/X11 all-targets（含 native-probe）、Windows App lib（computer-use-probe）均通过。
  `format-final.log` 的 18 个直接修改 Rust 文件格式检查通过。CI 增加 `macos_pointer_contract`，远端 CI 本轮未运行。
- `source-before-final.json` / `source-after-final.json`：选定的 **281** 个原生/Core 源码、测试、依赖/CI 与直接夹具文件 **0 漂移**。
  这是明确范围的源指纹，不是整个发行包可复现构建/签名证明。二进制和全部日志/文档哈希见 `source-receipt.json`。

### 浏览器不能掩盖的一次失败

`browser-scroll-contract-current.log` 同一 31 项套件先通过；最终并行构建/原生回归期间，
`browser-scroll-contract-final.log` **29 passed / 2 failed**（一个失败子测试及其父测试）。
失败点是 wait/snapshot 子场景开始时 `/goto` 返回 504，而不是该场景的快照断言；HTTP 测试第 912 行要求 200。
`navigation.mjs` 的原生导航截止为 8000ms。保留失败，**尚未证明其根因或负载稳定性已修复**。

其他构建/原生句柄确认终止后，不改代码、不延长超时、不筛选用例，以同一命令完整重跑：
`browser-scroll-contract-isolated.log` **31/31**、exit 0（59.88 秒）。
该通过不能覆盖先前失败，不能据此宣称浏览器长尾、压力或 12 小时长稳通过。

## 4. 全目标未达到最终版

1. macOS arm64/x64 真机编译、权限、签名、窗口/控件生命周期、原生输入效果及取消/恢复仍未验收；键盘、追加/完整 TypeText、IME、ScreenCaptureKit 等继续开发。
2. Windows 与浏览器完成/退出长尾、WebView 运行中取消、X11 全输入/恢复、GNOME 原生 Wayland仍开放。本轮浏览器并行导航超时须继续定位，不能用单次独立通过关单。
3. Desktop / managed browser / existing tabs / App WebView，App / ACP / MCP 全链、正式扩展及所有目标平台的签名安装、升级、回滚仍须逐项完成。
4. R17 不回退；完整重设计 UI 的原生窄窗、缩放、权限与交互矩阵，真实 Grok E4，冻结 12 小时 active soak 仍需证据。

下一步优先定位已暴露的浏览器负载导航长尾，并继续 macOS 剩余输入与物理完成/恢复；未缩减任何平台、UI、安装或模型门禁。
目标保持 active；没有调用 complete、blocked 或 paused。
