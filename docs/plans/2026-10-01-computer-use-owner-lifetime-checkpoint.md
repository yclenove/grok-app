# Computer Use：GNOME owner 生命周期与原始 D-Bus FD 退出隔离

日期：2026-10-01。目标仍为 **active / partial — not releasable**。
原目标“接手 Grok，完成 Computer Use 所有功能并推进最终版”没有缩小；本阶段不是最终发布验收。

## 当前实现

1. `gnome_session.rs` 不再订阅所有服务的 `NameOwnerChanged` 后才过滤。
   改为两个带精确 argument-zero 的 Shell / ScreenShield match，并合并原订阅流。
   订阅仍先于快照和最后 owner 检查；原 bus/owner 失效不会通过重连恢复授权。
2. 新增真实私有 D-Bus 的无关服务洪泛、名称前缀、相同 connection 的 release/reacquire、
   同一 socket 地址/unique-name 的 daemon 更换测试。覆盖 pending、激活中、ready 及重新选择。
3. 原生 PipeWire/libeis 联测先观察独立 C peer 的 disconnect，再查询 registry；
   无下一模型调用或 Stop。旧 capture/input/claim 被拒绝，原 capture node 退出。
   **这是私有 GNOME 协议来源，不是 installed GNOME/App 授权或用户可见操作效果。**
4. 并行全量暴露真实 FD 继承问题。新增确定性用例证明：zbus 原始 SCM_RIGHTS FD flags=0，
   解码副本 flags=1；保留原 message 跨 `/bin/cat` exec 时，原 socket 被子进程继承，
   丢弃本进程原件/副本仍无法 EOF，直到精确子进程退出。
5. 校验 crates.io 缓存原包 SHA 后 vendoring zbus 5.18.0，仅修改 Linux `fd_recvmsg`，
   原子传入 `CMSG_CLOEXEC`。不是事后 fcntl、仅保护副本、增加等待时间或限制并发。
   async-io/Tokio 共用该接收入口；非 Linux 保持上游行为，不冒领非 Linux 修复。
   原始 114 个包文件及许可证保留；只有一个上游源码文件变化，新增说明 `GROK-PATCH.md`。
6. 修复 runner 漏存 `gnome-owner-revocation.json`；先复现缺失，再加入显式归档名单与契约测试。
   早期套件原输出不改写；从其精确原 fixture 路径补取的报告单独标记为 supplement。

## 本阶段验证

证据目录：`tools/computer-use-probe/.run/gnome-owner-lifetime-20261001/`。

| 证据 | 当前结果与边界 |
| --- | --- |
| `baseline-test.log` | 旧全服务订阅在 96 个无关服务的请求/释放后阻塞同一 connection 回读，1 fail；原 monitor 已收尾 |
| `fd-baseline-010-test.log` | PW/EIS 两种原始 message FD 均可继承；两个实际子进程退出前 EOF 失败，退出后 EOF 成功 |
| `fd-patched-012-test.log` | 同一严格用例通过：原件/副本 flags=1、子进程无原 socket、子进程存活时 EOF 成功 |
| `fd-tokio-017-test.log` | 显式 `zbus/tokio` 构建的同一原始 FD/exec/EOF 用例通过 |
| `wayland-suite-013/014/020.json` | 同一 Rust/C 二进制串行一次、四线程两次，每轮 **198 pass / 0 fail / 0 ignored / 0 filtered** |
| `clippy-all-targets-016-result.json` | Wayland all-targets `-D warnings` 通过；既有 root-owned 缓存以 root 检查，未 chmod SDK/缓存 |
| `runner-tests-020.log` | **21** 项 runner/SDK 契约通过；不是 21 项原生桌面验收 |
| `check-patched-018-*` | Node33、Python61+10、fmt、语法、YAML、quality、diff 通过 |
| Linux App 集成 | 023 preview 与 024 default 的 App all-targets `-D warnings` 均通过；使用既有真实 Linux 资源 overlay，仅是编译检查，不是 bundle/安装验收；021 缺 glob 的原失败保留 |

全量最终二进制 SHA-256：
`218a2b43f72aa79c511fd6aee6d951a93baeb9f30e17270fdaee9d987ab461cf`。
Tokio 定向二进制：`00d32d2bf2de9013ce394bb488fdc7f67ec04bce71c54863654745a326403fa1`。
zbus 原包：`fe18fb60dc696039e738717b76eaea21e7a4489bbb1885020b43c94236d7e98a`。
最终源冻结 **574** 条（455 条先前源 + 4 个测试源 + 115 个 vendor 包/说明文件）；
源码规模增长主要是完整上游 vendoring，不代表新增 119 项产品功能。
最后 runner 修复不改变 Rust/C 二进制；020 使用最终 runner，013/014 为修复前归档器。

## 保留的失败，不用绿色重试覆盖

- `focused-002` / `diagnostic-003`：fresh-selection 测试提前释放最后 Source Arc；改为保留原 fixture，未放松产品规则。
- 005 的 Clippy：既有 root-owned example fingerprint 权限错误；未更改权限，之后以原缓存 owner 执行。
- 008 并行 **196 pass / 1 fail**：旧票据/跨 registry 的原 socket EOF 阻塞。
  010 确定性证明了同类 FD 继承机制，012 修复；008 没有当时的具体 FD/child 身份快照，
  不声称对该历史单次事件有逐 FD 追溯证明。其完整失败与 namespace cleanup 均保存。
- 011 `cargo update --offline` 意外改动其他依赖边：比较 gate 拒绝，输出单独保存；
  最终 lock 仅去掉 zbus 5.18.0 registry source/checksum 两行，`--locked` 构建通过。
- 019 新归档测试先失败；当次外层 shell exit 变量也被 PowerShell 展开，
  Python 原始 unittest 日志独立保留，不将外层退出码伪称 Python 退出码。
- 021 App 首次检查 bundle glob 失败及 CRLF exit 错误均保留；后续只修检查环境/脚本，不伪造缺失资源。

- 独立 verifier 首次错误要求两个 `match fd_recvmsg`；实际 Tokio 在 `try_io` 闭包调用。原失败/原 verifier/未验证 receipt 保留，改为分别核对两个真实实现，未改产品或运行时测试门槛。第二次把历史 panic 只在 stdout 查找而失败；对照原 stderr 后改为核对两个原始输出流，两次原 verifier/未验证 receipt 均保留。审计只读归档改为内存顺序读取，文件使用 O_NOFOLLOW+fstat，减少跨文件系统往返且仍校验全部内容。

## 完整目标仍开放

Windows x64、macOS arm64/Intel、Linux X11、installed Ubuntu24 Wayland、Ubuntu22；
Desktop、managed browser、existing Chrome/Edge、App WebView 经真实 App/ACP/MCP；
全部输入/IME/剪贴板、consent/takeover/lock/focus/topology/cancel/recovery；
签名安装/更新/修复/回滚/卸载、原生 UX/DPI、真实 Grok E4、同一冻结最终候选 12h active soak，
仍需逐项当前证据。尤其 stock GNOME 物理输入/锁屏与支持的生产 compositor 集成不能被本阶段私有总线通过替代。

没有 commit/push/tag/release；未开启产品默认授权、未改 stock helper、未重启 owned GNOME VM 或用户桌面。
本阶段证据通过独立 verifier 后才封存；下一步继续真实 installed GNOME 生产接管/授权链及完整平台范围。
