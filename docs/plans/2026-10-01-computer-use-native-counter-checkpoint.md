# Computer Use：installed 原生 compositor 计数器配对验收

日期：2026-10-01。用户目标仍是「接手 grok 的工作，完成 Computer Use 所有功能开发，推进到最终版」。
状态：**active / partial — not releasable**。本检查点不是全功能完成、生产 P0 全部关闭或发布批准。

## 本阶段实际进展

- 新增独立实验 UUID `computer-use-counter-probe@grok-app.local`，读取上一阶段实际编译的 Mutter46.2 libinput 入口代次 ABI；复用未修改的生产 `PolicyState` 和实验 `PhysicalInputWatch`。
- 探针只允许显式 owned-VM UUID 环境变量、root-owned 0444 精确 VM marker 和原生非 headless Wayland。只读 D-Bus 观察，不过滤/消费/重注入输入，不建立授权；App 的生产 helper 管理不安装该 UUID。
- 新增严格八边沿同键重叠 oracle、10 项反例回归及 CI 调用；保留原六边沿物理/EI oracle 与原 GTK fixture，不降低现有判据。
- 在实际 installed Ubuntu24/GDM/seat0/Wayland/IBus libpinyin 会话完成两轮配对、两轮同键重叠。六次输入运行属于同一个 Shell PID4142、唯一 bus owner `:1.32`、login1 session25。

| 实际原生实验 | 第1轮 | 第2轮 | 代次变化 |
| --- | --- | --- | --- |
| QMP USB 物理路径 IME 六边沿 | 6/6 | 6/6 | `[1,1,1,1,1,1]` |
| 原生 libei/EIS IME 六边沿排除及交付 | 6/6，0误报 | 6/6，0误报 | `[0,0,0,0,0,0]` |
| EI 持有 N 期间 USB N 按下/抬起，然后 EI 完成 IME | 8/8 | 8/8 | `[0,1,1,0,0,0,0,0]` |

每轮真实 native GTK text-input 都从空 preedit 开始并提交预期中文。重叠时两个 USB 边沿确实未转发到 client（seat-wide 抑制），但代次均推进；不是忽略 EI 运行期间的物理输入。八边沿 oracle 要求前后快照连续、owner/fixture PID/epoch 不变、计数不回退、焦点/IME/非 blocked 状态有效。

**证据边界：**QMP USB 不是真人证明；私有 Mutter RemoteDesktop EIS 接口不是 portal consent，不是 App/ACP/MCP 输入授权或撤销验收；计数器异步通知不构成 compositor 原子停止保证。单独 oracle 的 `nativeTransportProvenanceVerified` 等范围字段仍为 false，实际来源另由原 session/peer/进程/构建证据绑定。

## 精确候选与加载方式

- owned VM UUID：`812400f8-a6c6-4c38-a735-2b8d0ef8d3e2`，attempt14，原 QEMU PID28699/工具句柄68253。
- 对比环境仍是 Ubuntu24 GA Mesa24.0.5；Mesa25.2.8/virtio 崩溃问题没有在本阶段解决。
- 系统 Shell `46.0-0ubuntu6~24.04.15`，系统 Mutter `46.2-1ubuntu0.24.04.16`。实验库是上游46.2加补丁，不是完整 Ubuntu downstream 源构建。
- 实验 libmutter SHA256：`33c6ce53a99820a4173c2b320798d63cd6904fc83d61145f84e5fcf000a8800d`；Meta typelib：`7ca9d706187621212dfe0e8da02f3bf1c9af3818dccf5b656090034d31166e03`。
- 只设置 LD/GI 路径的初次启动实际混入系统 Cogl，出现重复 GType 注册与断言。保留失败日志；没有把观察超时直接解释成已退出或盲目重启。
- 工作方案是在 owned guest 目录复制已安装 Shell，仅用提取的 patchelf 修改 ELF RUNPATH 为 `$ORIGIN/mutter:/usr/lib/gnome-shell`，配合原用户服务的临时 drop-in、新 GDM 登录和精确库/typelib 链接。没有覆盖系统二进制/库，没有修改 Shell JS 或机器码。
- 原 Shell SHA256：`5f678999f051dd1e17f2da50cb55ddc152d9b1f5d45a6ea8ead8b4df603b2a17`；私有副本：`0e5040f66eb43fdba9a20641eda8c24fcba3945c5a7bad270d31821ee164cff2`。`.text`、`.rodata`、`.data` 独立逐字节比对，二进制及候选/恢复 maps 均归档。

这是可逆的 owned-VM 实验加载，不是面向用户的支持/安装/更新方案；不能据此把 stock Ubuntu 的已知红项改绿。

## 失败、警告及清理

- 两个锁屏检查均未通过：001 在异步 EnableExtension 导出就绪前读 endpoint；002 修复等待后，起始会话已经自然锁屏，因此未观测有效 unlocked→locked 转换。结果 `passed:false`，不计为锁屏或原子停止成功。
- 候选 journal 保留 EIS `adding device without capabilities` 和 virtio cursor atomic commit 警告；不宣称完整后端无警告。
- 中间设置失败（用户 runtime 目录消失、`.so...p` 目录、初次 objcopy 无输出参数）原样保存。objcopy 尝试原输入写回被非 root 权限拒绝，之后确认系统 hash 未变，改为只读 ELF 解析。
- 四个 EIS session/原 peer 均实际关闭、endpoint 缺失，原 peer、RPC、SSH 子进程均退出0并 join；各轮输入源及 MRU 保存/恢复。
- 删除精确自有临时 drop-in；探针四文件验证 hash 后移出 Shell 扩展发现目录保留证据。生产 helper 保持 disabled，endpoint 缺失。
- 新 stock Shell PID6842 / owner `:1.24` / session47 的实际 system-only maps 已验证；旧候选 PID4142 缺失，系统 Shell hash 未变，`dpkg -V gnome-shell libmutter-14-0 mutter-common` 无差异。
- 随后 owned VM 正常关机；原句柄68253退出0并确认 original child join。最终 owner 与 PID28699缺失、SSH42791关闭再次核验；未重启、未留下实验会话。

## 回归和证据索引

目录：`tools/computer-use-probe/.run/gnome-counter-native-20261001/`。

- `physical-ime-{001,002}.json`、`ei-ime-{001,002}.json`、`same-key-overlap-{001,002}.json`：实际行、未改判据、来源绑定和原进程收尾；封存前重新计算 oracle，不只信任布尔结果。
- `guest-evidence/{active,relocation,rollback}.json`、两个 maps、候选 journal、原/私有 Shell 二进制、已退场探针：原生候选、兼容性边界及系统回滚证据。
- `owned-session-artifacts.tar.gz`：17文件、51016字节，SHA256 `4e792b5d7082923dcdcb0aa912bd4c65f8fb3b0ae4670c13ae2cbda8ffb13fe6`。
- 本阶段 Node33（policy21+watch12）、既有 Python32、新 overlap Python10、探针语法和 CI YAML 检查通过；最终 quality gate 和 Windows git diff 检查通过。此前误用不存在的 quality 脚本和 WSL git worktree 路径失败日志保留，并由正确命令复核。
- 上阶段完整773步构建、C6严格/C6 ASan/UBSan、实际 ABI 与两次 headless GI 原线程收尾属于历史证据，未冒领本阶段重跑。
- `source-freeze.json`、`final/selected-sources.tar.gz`、`receipt.json` 记录当前来源与证据；`previous-documents/` 保存更新前四份文档。前一 receipt `9648df838b90f142cc863d436fa0177d6eeca00253ce5378f2cedc43b7b914f5` 与前一源归档保持不变，不以变化后的 live 文档冒充旧冻结。

## 未完成需求与下一执行顺序

| 要求 | 当前证据 / 尚缺证据 |
| --- | --- |
| Stock GNOME 原生输入安全观察 | 旧物理 `[0,1,0,1,0,1]`、EI同值误报仍红；实验配对通过不等于 stock 修复。生产 helper/App 未接入新计数器。 |
| 实际全输入覆盖 | 先扩展普通键/修饰键/指针/滚轮与 EI 同步交错，再覆盖 capture、pad、touch、tablet、grabs、设备/源销毁、锁屏/焦点/拓扑/重登录和原 owner 失效；需要同一候选实测，不能只靠单位测试。 |
| 支持的分发集成 | 需要明确版本/ABI/包签名与安装升级回滚路线；实验 RUNPATH 副本及上游补丁不满足 Ubuntu downstream/支持性要求。 |
| 产品授权与接管 | 在验证 provider 后才接入真实 App/ACP/MCP 授权、撤销、取消/恢复和原生资源收尾；不得把 helper 健康或 EIS 测试接口当作 grant。 |
| 全平台及全部目标 | Windows x64；macOS arm64/Intel；Linux X11、Ubuntu24 installed Wayland、Ubuntu22 baseline；Desktop/managed browser/existing Chrome+Edge/App WebView，最终候选逐项证明仍未完成。 |
| 最终交付 | 全部输入/IME/剪贴板、签名打包安装升级修复回滚卸载、UX/DPI、真实 Grok E4、同一冻结最终候选12h active soak 全范围保留，尚无完整最终验收。 |

不 commit/push/tag/release，不修改宿主桌面输入权限。继续按上述全范围推进，不用局部配对成功重定义最终版。
