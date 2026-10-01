# Computer Use：installed Ubuntu/GNOME VM 与安装权限回归

目标不变：**接手 grok 的工作，完成 Computer Use 所有功能并推进到最终版**。
状态 **active / partial — not releasable**；本记录不是最终验收、签名发布或缩小后的替代交付。
文档日期按当前开发环境指令为 2026-09-30；`20261001` 仅为本地运行目录标签。

## 本轮真实进展与边界

- 复核上一阶段的原 QEMU handle/PID、工作树和失败日志，而非按聊天意图重启：上一阶段及本轮均有实际状态/证据变化，属于 progress，不是无依据的 wait。
- 在独占、临时、无主机目录共享/输入透传的 KVM 来宾中运行了**已安装 Ubuntu desktop、GDM、GNOME46、native Wayland、真实 ScreenShield、PipeWire 和 portals**。不是之前缺少 GDM 的私有 headless Shell。
- 原 Host 安装遇到真实 umask 0002 缺陷；先保留失败并新增红色回归，再修复新目录的创建权限。没有放宽 `safe_path`、修改既有目录权限或更换用户安装目录。
- 原 Host `native::act/status` 的安装、新 Shell 识别、反复启停、损坏后显式修复、新 Shell 后再启停四项手工测试实际通过，产品开关始终为 false。**这不是 renderer IPC、原生 App 授权或桌面输入 grant 验收。**
- 实际锁屏观察到同一 helper epoch 的代次 1→2、blocked false→true，真实 ScreenSaver active=true；没有证明撤销延迟上界或 App 输入已停止。
- 已视觉复核正常桌面 PNG；锁屏时 QMP 截图仅为 `Display output is not active`，**不是可见锁屏对话框/原生 UX 验收**。第一次锁屏观察因来宾已自动锁定而未满足前置条件，原失败 JSON 留存；重启 owned GDM 后的新观察才验证转换。
- **发现并保留 P0：Wayland client 上的原生设备输入未推进 helper 代次。** 不能把启用通过升级为接管保护通过。

## 安装权限根因、修复与回归

原失败 `installed_vm_install_requires_new_shell_process.log`：status=missing，Install 返回
`computer_use_helper_files: untrusted writable installation ancestor`。
来宾 `umask=0002`；新建 `~/.local/share/gnome-shell/extensions` 为 0775，其余上级为可信路径。
`PublicationLock::acquire` 原先先检查缺失路径、`create_dir_all` 使用默认 0777 再受 umask 影响，
随后安全检查拒绝其刚创建的组可写目录。失败目录为空，helper 未创建。

修复：`lock.rs` 和 `bundle.rs` 对所有新建层级使用 `DirBuilderExt::mode(0700)`；
staging 也直接以 0700 创建，不再经历先宽松创建后 chmod 的窗口。既有目录保持原权限；
组可写、符号链接、非本用户/非 root 等原有拒绝规则保留，Bundle 新建后再次检查安全路径。

新增 `publication_ignores_permissive_umask_without_changing_existing_paths`：
仅在 exec 前的独立子进程设置 umask 0000/0002，不污染并行测试进程；分别覆盖有/无
PublicationLock 的发布、所有新目录 0700、文件 0600、已有 0750 祖先不变、已有 0775 根拒绝且不被改写。
红色构建 16 pass / 1 fail / 7 ignored；修复后 preview helper 17 pass / 0 fail / 7 ignored。
其中四个 installed-VM ignored 项另行逐个执行，均 1 pass / 0 ignored；其余三个旧 private-Shell 项本轮未另跑。
来宾只 `rmdir` 精确验证为空的旧失败目录，**没有 chmod 使测试通过**；同一 umask 0002 下重新安装后目录 0700、文件 0600。

## 原生输入缺口：已证伪，不得算通过

证据保存在 `.run/ubuntu-gnome-vm-20261001/` 以及对应 WSL evidence：

1. `native-policy-observation-001/002.json`：两次不同的 QMP USB tablet 位置，helper serial 均不变，失败保留。
2. `kernel-input-probe-001.json`：对精确 owned 来宾的 QEMU USB event4/event5 读取，实际收到了 ABS_X/Y 和 Shift down/up；原读者 SSH 子进程已 join。不是主机监听、真人输入或产品实现。
3. `input-routing-comparison-001.json`：同一 Shell owner/epoch，Shell panel serial 1→2→3；desktop client 两次移动仍 3→3；回到 Shell panel 后 4。
4. 上游 GNOME/mutter **46.2** `clutter/clutter/clutter-event.c` 1289–1303、1339–1341：filter 按注册先后串行执行，遇到 STOP 不再调用后续 filter；`src/core/events.c` 464–465 在 Wayland compositor 已处理事件时返回 STOP，493–498 注册 Mutter 路由 filter。已保留源码下载与 SHA；未改写或打补丁到来宾 Mutter。

因此 source-device 的物理/virtual 分类谓词本身不足以证明输入覆盖；晚注册 Shell extension
filter 会漏掉先被路由/消耗的 client 事件。当前 endpoint=ready 只证明管理/协议可用，
**不能证明完整接管监控健康**。不得用 idle 时间、设备名称、只在 Shell panel 测试或放宽
策略作为“修复”。下一实现必须获得不会被早期 client 路由吞掉的、内容最小化的原生观察源，
在实际 Wayland client、键盘/IME、pointer/touch/grab、原生 EI 虚拟输入区分及 App grant 撤销上联证。
不得把此缺口缩减为“只支持 Shell 区域”。默认关闭、native_wayland=false 保持；禁止据此候选发布或真实自动化。

## 来宾与兼容性限制

- 唯一 VM UUID `812400f8-a6c6-4c38-a735-2b8d0ef8d3e2`，账户 `cuaccept`；SSH 仅 loopback 42791，独立临时 key/known_hosts，凭据不归档/展示。
- 官方 noble daily `20260926` cloud image，SHA256 `6a81c37564db9b1ee84e141922625e1d7c5b389b99bb3c572e0243607d5bb4d2`；签名指纹 `D2EB44626FDDC30B513D5BB71A5D6C4C7DB87C81` 在先前准备阶段验证。不是正式发行镜像/安装包发布认证。
- 来宾 os-release 自报 Ubuntu 24.04.5，kernel 6.8.0-142；Shell 46.0、Mutter 46.2、GDM 46.2。以 `final-guest-state-001.log` 精确包版本为准，不能由此推断发行基线已经通过。
- 更新版 Mesa 25.2.8 + 当前无 3D 的 virtio GPU，在 helper 安装前就发生 GNOME/EGL `strdup(NULL)` 崩溃并退回 X11。日志/core backtrace 原样保留；**未解决、未算 Wayland 通过**。
- 只在该临时来宾改用官方 GA Mesa 24.0.5 + LLVM17 做对比，恢复真实 native Wayland；不是建议用户降级，也不是证明更新版 Mesa/任意 GPU 兼容。
- std-VGA 对比因该 cloud kernel 无适用 DRM 节点而只得到 X11，没有把 X11 冒充 Wayland。网络等待修复、GDM 会话选择均只发生于 owned clone；原 backing disk 留存，无用户主机包/登录桌面变更。
- 最后原 Host 明确 Disable，Shell state=2/enabled=false，helper endpoint absent；来宾正常 poweroff，原 QEMU PID3071 returncode=0、原 handle88580 已 join。磁盘保留供后续同一 owned VM 继续，不把已退出进程记为等待中。

## 构建、证据与未覆盖范围

- 构建前 414 条选择源冻结；`umask-red` 与 `umask-fixed` 及更早原失败分别保留，不覆盖上一阶段 receipt。
- fixed preview App test ELF SHA256 `368f21fa11173145be7130be6f413ed70080f24b1bf3c8d11cef7ca37b2ac992`；传入来宾前后 digest 相同，ldd 全部使用来宾已安装库，不注入 Debian SDK 运行时。
- preview commands/portal/feature 15/6/1 pass；App + Wayland preview 和 App default all-target strict Clippy pass；本轮完整 Wayland **171 / 0 ignored / 0 filtered**，default helper **18 / 0 ignored**，Node policy contracts **10**。独立解码核对 65 owned 目录、34 EI disconnect、18 PNG（CRC/像素），原 native owners 全部 join；rustfmt、diff-check、final quality gate 均通过。此私有 labwc/PW/EIS 回归与真实 installed VM 是两组不同范围的证据。
- README 的已知缺口说明是构建后文档变化；receipt 区分构建冻结与最终工作树记录，不伪称构建后所有字节都未变。
- 汇总将存入 `tools/computer-use-probe/.run/ubuntu-gnome-vm-20261001/receipt.json`；链接上一 immutable receipt SHA256 `1cf59802f2638376d33719206a717b7fb82be59a3284c2d41991fb8d78ad2e52`。关键失败、成功、来宾身份、binary/source SHA 和原 process join 均需独立核对。
- Windows/macOS、renderer/前端视觉、原 App 当前 login1/PID 绑定、实际 portal consent、真实 Grok E4、签名 clean-install/update/repair/rollback/uninstall、本轮未执行；不能由四个管理测试推出完成。

## 下一动作与最终版审计

1. 解决已复现的 client-routed 原生输入观察缺口；保持真实 client/IME/EI 区分与 grant 立即撤销范围，不替换为弱轮询或缩小表面。
2. 继续同一 installed VM 的原 App/GTK WebView、真实 system consent、当前 session 绑定、取消/故障/锁屏/焦点/拓扑联测，并关闭 Mesa 更新版的环境问题。
3. Windows x64、macOS arm64+Intel、Linux X11 与 Ubuntu24.04 GNOME native Wayland；Desktop/managed browser/existing Chrome+Edge/App WebView，经 App/ACP/MCP 的完整功能、中文 IME/clipboard、权限接管与恢复逐项留证。
4. Ubuntu22.04 baseline、AppImage/deb/rpm 与各平台签名安装/升级/修复/回滚/卸载、原生 UX/DPI、真实 Grok E4、**同一最终冻结候选连续 12h active soak**仍是必须条件。当前测试数不能替代任一项。

没有完成全部要求，因此不调用 complete；没有连续三轮同一外部阻塞审计，也不调用 blocked/paused。
