# Computer Use — compositor physical counter checkpoint

日期：2026-10-01。完整目标仍是接手 Grok 并完成 Computer Use 最终版。
状态：**active / partial — not releasable**。本轮是实现与实测进展，不是完成或等待。

## 本轮实现

- 新增 `tools/computer-use-gnome-shell/compositor/`，提供精确针对 upstream Mutter **46.2** 的实验补丁、输入线程安全计数器、只读版本化 ABI、未接入生产的 JS 订阅消费者及检查工具。
- 观察点在 `process_event` 的原生 libinput 入口，早于 IME、capture、按键 seat-wide/重复抑制及 client 路由。只排除 NONE / device-added / device-removed；未知输入类型保守计数。EI virtual-device 调用不走此入口。没有打开新输入设备、截获/重注入事件或导出输入内容。
- 新发现：即使移到 Clutter 过滤器最前，同键物理/EI 重叠仍可能已在 `process_device_event` / `meta_seat_impl_notify_key_in_impl` 被抑制。因此不能仅补一个更早的 Clutter 回调。
- 计数器在原输入线程递增、主线程合并通知，最多一个待处理 source；同步回读不依赖通知已 dispatch。原 emitter 生命周期由 source 持有，关闭后失效且不恢复，uint64 饱和不回绕。
- JS 消费者先订阅再取基线，验证版本和安全整数，回退/读失败永久失效，保留原订阅 ID 并幂等退出。**没有修改生产 `extension.js` / `policy.js` / App，不能据此启用自动化。**

## 实际验证与边界

| 项目 | 结果 | 不能证明什么 |
|---|---|---|
| GLib C contracts | 6 项；严格 C11/Werror 与 ASan+UBSan 各通过一次，含 4 线程共 100,000 次递增 | 不是原生输入/IME 验收 |
| Node | policy21 + 新消费者12 = **33 pass** | 不是 installed Shell 验收 |
| Python acceptance oracles | **32 pass**，原普通/物理 IME/EI IME 判定逻辑未降标 | 不代表实际六边沿通过 |
| 精确源码补丁 | before/after SHA256、零 fuzz dry-run/apply 验证通过 | upstream46.2 不等于 Ubuntu 全部 downstream 补丁 |
| 全量 Mutter 构建 | **773 Ninja steps**，包含 Meta-14 GIR/typelib，exit0 | 配置明确关闭 upstream tests，未冒领完整上游测试 |
| 实际 DSO 的 C ABI 检查 | MetaSeatNative 的 uint版本、uint64代次、只读/显式 notify 元数据通过 | 没有构造 seat，不是实时输入 |
| 实际 GJS/native seat | 最终 **010 / 011 两次** exit0；实际映射构建库、版本/代次回读、通知、原订阅退出均通过 | headless；通知明确为 synthetic，代次必须保持0，不能标为物理/EI/IME成功 |
| 原输入线程收尾 | 测试专用 preload 仅旁观 `g_thread_try_new` 返回的原 `GThread*` 与其原 `g_thread_join` 返回；两次 created1/join1/violations0 | 不替换输入函数，不用 OS 线程名或进程缺失冒充 join |
| 质量 | quality gate、YAML parse、git diff check通过 | 不是全平台 App/UI/签名安装回归 |

## 失败保留与修复

- 首次归档解包拒绝合法的六个归档内文档链接；随后逐个验证目标仍在 source root，再使用安全解包。未允许设备文件或路径逃逸。
- 第一次完整配置因缺 `xcb-randr` 失败；只增装对应开发包，原失败日志保留。
- GJS 001：调用了未暴露的 MetaBackend GI 方法，且 `meta_context_destroy()` 的实际 unref 与 GI transfer-none 元数据不匹配；改用已验证的 Clutter backend 路径及保留 GJS ref 的 dispose。
- GJS 002：输出初步通过后，Clutter wrapper 晚于 context GC 导致退出断言，**因此不是通过**。最终先解除订阅、释放 Clutter wrappers/GC，再 dispose context。
- GJS 004：GI 懒加载前检查映射位置不对；移到真实调用之后。006/008/009 的 OS comm 名称判据不成立，原始线程快照保留；最终不是删除收尾门槛，而是改为原 `GThread*` 的真实 join 审计。
- 最终010/011使用 `G_DEBUG=fatal-criticals`、独立 HOME/runtime/session bus、内存设置、local GIO VFS。系统 bus 不可达造成 RTKit warning 如实保留，未宣称无 warning。

## 构建和生命周期证据

阶段目录：`tools/computer-use-probe/.run/gnome-physical-counter-20261001/`。

- 上游 tar SHA256：`009baa77f8362612caa2e18c338a1b3c8aad3b5fe2964c2fef7824d321228983`；补丁：`487534780cfb6e97825c0f8d49e5fbe0afe7c33f7784b1ef425902f6e3f92b17`。
- 实际 libmutter DSO：`33c6ce53a99820a4173c2b320798d63cd6904fc83d61145f84e5fcf000a8800d`；Meta typelib：`7ca9d706187621212dfe0e8da02f3bf1c9af3818dccf5b656090034d31166e03`。
- `owned-build-artifacts.tar.gz` 保存85个精确构建/日志/源码/探针文件，SHA256 `11dee596033631d71c27e87edb8a8fd33a26328411951e93f923d4f55b56fa69`，逐成员索引在同名 JSON。
- 只在 owned VM 添加180个构建依赖；**1564个原已安装包版本不变**。系统 Mutter/Shell 的包文件验证通过。没有 `ninja install`，安装 prefix 不存在。
- 生产 helper 三文件哈希保持上一阶段值；helper disabled、GetState endpoint absent 已回读。九个原 GJS 进程均已退出；所有原 SSH 子进程已 join。
- VM attempt13/PID25631 正常关机，原工具句柄 **22662** 返回exit0、`originalChildJoined=true`；原完整构建句柄 **97841** 返回exit0。未动用户桌面、输入组、host设备权限，也未发布/提交/tag。
- 上一阶段 receipt 保持 `0d668763ff1b238733fa3318fad33ee5f162d855f25cddea919c819b4cf2b3c4`；旧入口文档在本阶段 `previous-documents/` 按原hash保存，旧封存目录不修改。
- 封存时 H: 空间耗尽，已将**仅本轮**三个大归档逐一复制、SHA256/字节数回读一致后迁至 WSL `/var/tmp/grok-cu-physical-counter-20261001-artifacts/`，释放21,957,474字节；没有删除历史验收或用户文件。索引 `artifact-storage.json` 保留原/现路径。完整 source archive 也写入该独立目录。H: 余量仍低，后续大构建/归档应继续使用有空间的 owned WSL 目录。

## 下一步和未完成门槛

**P0 仍未关闭。** Stock GNOME 的上一阶段物理/EI 六边沿仍都是 `[0,1,0,1,0,1]`，各3/6、双 gate false。本轮未在新的 compositor 下重跑原生 IME，也未以 headless 结果替换这个红色结论。

1. 在 owned installed GNOME 会话对照 stock 与实验集成，保留不变的物理/EI六边沿、同键重叠、普通 client、capture/pad/touch/tablet/grabs、lock/source-teardown 门槛；需要真正的 Shell 与原生输入，不是仅 ABI 或 synthetic notify。
2. 明确支持的分发/更新/回滚与上游集成路线。不能把自行打补丁称为 stock Ubuntu 已支持。验证 owner/session/topology/lock/heartbeat 与授权撤销，再考虑连接生产 provider；异步计数通知不提供 compositor 原子的“零后续输入”保证。
3. 完整目标不变：Windows x64、macOS arm64/Intel、Linux X11、installed Ubuntu24.04 native Wayland、Ubuntu22.04基线；Desktop/managed browser/现有Chrome+Edge/App WebView，经 App/ACP/MCP 全链路；所有输入/中文IME/剪贴板、系统授权、接管/锁屏/焦点/拓扑/取消恢复；AppImage/deb/rpm、签名安装/升级/修复/回滚/卸载、原生UX/DPI、真实Grok E4、**同一最终冻结候选的12h active soak**，均不能由本轮局部结果代替。
