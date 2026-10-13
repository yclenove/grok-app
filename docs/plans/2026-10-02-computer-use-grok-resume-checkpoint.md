# Computer Use：Grok 恢复批次检查点

日期：2026-10-02。状态：**active / partial — not releasable**。
HEAD 仍是 `8d8b71c262ab222748bca32412eea884d4b892e0`，分支 `feat/computer-use-implementation`。
本批没有 commit、push、PR。Computer Use 全平台最终版没有完成。

## 本批已核实

owned VM attempt 22（QEMU pid 559，UUID `812400f8-a6c6-4c38-a735-2b8d0ef8d3e2`）上，distro `xdg-desktop-portal-gnome` 46.2-0ubuntu1 的 SHA256 仍是 `cf6839a8003098a7a4d710d520b8fcd9afbba6bbd21931bc0e420ac724b79bc7`，大小 564040。`/usr/libexec` 没有被覆盖。

CreateSession 最初被 Mutter 拒绝：`Session creation inhibited`。当时 session 1 是 cuaccept 的 seat0 Wayland 会话，`LockedHint=yes`，`org.gnome.ScreenSaver.GetActive` 为 true。用既有的 `sudo -n loginctl unlock-session 1` 解开这台 owned VM 的 OS 锁屏后，`LockedHint=no` 且 shield 为 false。没有点击 portal 授权框，也没有改 idle-delay（仍是 uint32 300）或 lock-enabled（仍是 true）。

原 guarded client（SHA256 `5392481e4bfa4b7d5b746225d1a16e7f6ca1d2376fa726f4f79f89c0e7ed62dc`）在原后端 PID 1870、owner `:1.89`、procStart `1005` 上跑到了断言：

- `request-close-pending`：Start 仍 pending 时 Request.Close，Start 恰好回 response 2 一次，Close 回一次，随后 request 对象消失。原 PID 1870 仍在，没有 SIGSEGV。这是单次完成，不是已证实的二次完成崩溃。
- `session-close-pending`：Session.Close 回了一次，但 Start 在 30 秒调用超时前没有 portal 回复。原 PID 1870 仍在。客户端最终的 Request.Close 才把仍活着的 Start request 关掉。

对话框取消后再 Request.Close 没有在 distro 进程上重跑：那需要外部取消，本批不点击系统权限框。官方 46.2 源码加上仅在 `CU_OWNED_LIFETIME_CANCEL=dialog-cancel` 时发出既有 cancel `done` 信号的私有钩子，在私有二进制上复现了同一崩溃：Start 先回 response 1，随后 Request.Close 使 PID 4252 `status=11/SEGV`，systemd `Result=core-dump`，`NRestarts=0`。这不是 distro 进程，也不是 App 授权。

同一私有树打上 Start 所有权补丁后：

- 取消钩子仍在时，dialog-cancel 再 Request.Close：Start response 1 一次，Close 一次，request 随后消失，PID 4645 / procStart `177272` 仍在，没有 SEGV。
- 关掉取消钩子后，Session.Close while Start pending：Start response 2 一次，Session.Close 一次，session 对象消失，PID 4836 / procStart `180964` 仍在。最终 Request.Close 仍得到一次回复，说明 Session.Close 完成了 Start，但没有自行卸下 Start request；这次 Close 没有第二次完成 Start。

实验结束后 drop-in 已删除。当前 portal 是新的 distro 进程 PID 5026，哈希与原包一致，`DropInPaths` 为空。PID 1870 在切到私有二进制时已退出，不能把 5026 说成原进程存活。gnome-shell Wayland 仍是 PID 1367。没有读取或修改启动器 auth.json/config。

随后在这个已经恢复的 stock 进程上补了断连场景。PID 5026、owner `:1.138`、procStart `187165` 在两次实验后都还是同一个进程。客户端在 Start request 已导出、尚未回复时直接关掉 bus 连接，不调用 Close，也不点击对话框。46.2 `session.c` 不监视客户端 peer，所以 request 和 session 都还在。新连接第一次 Request.Close 成功，第二次得到 `UnknownMethod`；Session.Close 同样是第一次成功、第二次对象消失。016 的 dbus-monitor 只把 destination 等于 Start 调用方的 method return 算作 Start 回复：serial 8，恰好一条，body 是 `uint32 2`。journal 没有 SIGSEGV 或 GLib-CRITICAL。gnome-shell 仍是 1367，idle-delay 仍是 uint32 300，lock-enabled 仍是 true。`productionAppVerified` 为 false。

015 先以退出码 1 结束，因为匹配器把另一个连接上 Session.Close 的 `reply_serial=8` 也算进去了。保存的监视日志里，回到原调用方的 Start return 只有一条。这是匹配器缺陷，不是第二次 completion。修正后的匹配先用这段日志文本做了断言，再由 016 在 guest 上跑过。

这条 stock 路径只说明 pending Start 在断连和重复 Close 下完成一次。它不能外推成 distro 上“对话框取消后再 Request.Close”已经修好。后者仍然只在私有二进制上复现和验证。

## Start invocation 生命周期（46.2 `src/remotedesktop.c`）

| 点 | 原 46.2 | 补丁后 |
| --- | --- | --- |
| 持有 | `handle_start` 把 invocation 存进 `start_invocation`，并导出 Request、接上 dialog `done` 与 Request `handle-close` | 同左 |
| 成功消费 | `start_done` 在同步 D-Bus 之后 `complete_start`，然后才把指针置 NULL | 入口发现已空则直接返回；完成前再查一次。真正完成只通过偷走指针的 `complete_start_invocation` |
| 对话框取消 | `remote_desktop_dialog_done` 对非 0 response 调用 `cancel_start_session`，不清空指针，也不断开 `handle-close` | 取消走同一次偷取完成，并退休 dialog |
| Request.Close | `handle_close` 再次 `complete_start` 同一指针。原 core PID 2988 与本批私有 PID 4252 都在这里 SIGSEGV | 指针已空则不再完成；dialog 已退休则不再进入旧回调 |
| 迟到 ready | `on_gnome_screen_cast_session_ready` 调 `start_done`。取消不断开 ready id | 取消先断开非 0 的 ready handler。`start_done` 若发现指针已被偷走，不再回成功 |
| Session.Close | 不断开 id 0 以外的保护不足，且不完成 pending Start。Mutter stop 失败时打出 `Session not started` | 先完成 pending Start 一次并退休 dialog，再 stop；id 为 0 的 handler 不 disconnect |
| 成功与取消 | 成功路径不清 dialog，取消路径留下可再次完成的 invocation | 只有仍在等 screen-cast ready 时保留 dialog；两条路径不共用同一次 invocation |

`screencast.c` 里同类的非空取消没有改。上游 main `a4f6ed64474ff6fe6c4eb9dbe092d97cafd6ea81` 与 MR !231 在本批开始前已核对为没有这个 RemoteDesktop 修复；本批没有再下载上游 main，也没有升级主机或替换 distro 包。Ubuntu 2125128 仍是另一条 screencast UAF。

补丁、基线和 LGPLv2.1+ 说明在 `docs/plans/2026-10-02-xdg-desktop-portal-gnome-46.2-start-ownership.patch`。对官方 46.2 `remotedesktop.c` 的 `patch --dry-run -p1` 已通过。私有构建前缀和 systemd drop-in 只存在于这台 owned VM，并已移除 drop-in。

## 未完成

- 真人点击或外部操作下的 distro 对话框取消、同意后关闭，以及真实 Grok E4。协议 fixture 不是 App 授权。
- 迟到 session-ready 的两个顺序，以及成功 Start 与取消互不污染，没有跑。`open_screen_cast_session` 只在对话框 response 0 之后的 `start_session` 里连接 ready。pending Start 没有这个回调。要排出这两个顺序就得发出 OK 或调用 `start_session`，那是在批准 portal consent。本批没有这样做。
- Session.Close 后 Start request 仍要一次 Request.Close 才从总线上消失。这是私有补丁实验里看到的界限，本批没有再改那份已经通过的补丁。
- screencast 兄弟路径未修。
- Host/App 取消与恢复没有新的产品代码改动。现有覆盖包括 `feature_off_fences_before_slow_persist_and_releases_each_target_once`、`stop_requested_is_not_stopped_while_worker_runs`、`late_callback_ignored_after_generation_change`、`cancelled_attempt_tombstone_cannot_mint_a_late_preparation_ticket`，以及模型工具表不含 authorize/resume。这些测试本批没有重跑，不能记成新的通过。没有发现需要新写失败回归的产品缺陷。
- 全平台范围不变：Windows x64，macOS arm64 与 Intel，Linux X11 / 支持的 Wayland / Ubuntu 22，Desktop 与各类浏览器表面，输入/IME/剪贴板，锁与焦点及 owner/cancel/recovery，签名安装与回滚，原生 UX/DPI，真实 E4，以及同一冻结候选 12 小时 soak。
