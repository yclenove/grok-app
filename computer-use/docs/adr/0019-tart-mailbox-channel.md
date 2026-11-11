# 0019 · Tart 邮箱控制通道与宿主专用卷（S11）

状态：**Proposed** · 日期：2026-11-10 · 细化 [ADR 0018 §14](0018-tart-disposable-provider.md#14-h1-决定不经-ip-的控制通道owner-2026-11-10-选择方案-2)。

**仅设计；未实施、未独立评审、未操作 Mini、卷或网络。** 本文不安装 provider、不铸造准入能力、不改变 S6 门禁或[验收矩阵 §8](../real-eval-acceptance-matrix.md#8-开启真实运行的门槛)。路径相对 `computer-use/`；命令表是待验证的 argv 规格，不是运行记录。

## 1. 决定、事实与威胁模型

**原则：邮箱是一个受限的宿主共享目录例外，不是可信文件系统。** owner 2026-11-10 已决定：无 IP 控制通道；Softnet 保持 `block 0.0.0.0/0`（含网关）+ 仅模型 endpoint 的获批 `/32`；不加宿主 guard / pf 规则。唯一共享为每 attempt 新建、限容专用卷内的邮箱；其它宿主目录、剪贴板、拖放、设备/agent 转发仍禁用。

已测事实只沿用 ADR 0018 §13/§14：Apple silicon Mini、macOS 27、Tart 2.41.1；virtiofs 双向可见，宿主新文件约 1s 可见；guest 能写 3 MiB 以上文件及符号链接、宿主侧属主为宿主用户，**没有配额**。guest 路径为 `/Volumes/My Shared Files/<name>`。`tart exec` 不可用；不采用 `--disk` / `--serial-path`；`eval` 无免密 sudo。这些事实不证明下文的时限、原子性或隔离。

| 攻击面 | 宿主必须采取的边界 |
| --- | --- |
| guest 写入的全部字节、名字、目录和元数据 | 一律攻击者控制；即使声称来自 supervisor，也只能作为待核验证据，不能自授 session/ACK/owner 签认 |
| symlink、hardlink、FIFO/socket、可能的设备/特殊文件 | 不跟随链接、不打开目录中的任意路径、不执行内容；同句柄核验普通文件且 `nlink == 1`；跨卷硬链接也不假定必然失败 |
| 稀疏/巨大文件、大量小文件、目录 churn、stat/open 竞态 | 卷限容；固定名字/条目数/读取字节/耗时上限；不按声明长度分配，不递归遍历、复制或逐文件清理 |
| Unicode、大小写/规范化别名、名字替换 | 只接受字节精确的 ASCII 白名单；不 case-fold、不归一化后认领；文件名不是身份 |
| 修改中的文件、重放、旧 boot/attempt、伪造响应 | 句柄复验、消息绑定与单调序号；未知即 `not_run`，不重试副作用、不采纳替代实例 |

**宿主永不**信任 guest 的名字/大小/路径、无限解析、执行脚本/产物、导入凭据或 HMAC 密钥、保留跨 attempt 的目录/文件/VM 句柄。宿主用户属主相同不证明写入者。卷的 `noexec` 等不替代宿主安全打开，也不限制 guest 内执行。

**模型必须完全不可达邮箱。** S11e 须证明不同安全主体 + 实际 OS sandbox 同时成立：模型及其任意后代不能列举/读/写/rename 邮箱及别名，不能继承其 FD，不能经 helper/relay/fixture 代读代写，不能 debug/ptrace supervisor、读其内存/句柄、修改程序/启动项或取得管理员能力。仅不同 UID、0700、隐藏路径、私有 FD 均不足；任一否定探针失败则禁止模型启动。

## 2. 单次 attempt 的宿主卷流程

**原则：先登记、后副作用；先证明整树停止、后卸载；最后才封存。** 调用方持有全部资源和清理责任，无 daemon、observer、新线程或 Drop 清理。宿主受保护账本、回执和持久 Stop 位于卷外，guest 永远不可见。

拟选 **64 MiB、HFSX（区分大小写的 HFS+，不启 journal）、单文件 SPARSE 镜像、无分区布局**。独立文件系统限制数据与元数据；不使用宿主 APFS 容器共享空间，避免 APFS 最小容量/快照/COW 复杂度；exFAT 缺少所需 POSIX 元数据语义。64 MiB 是 ADR 0018 示例容量的具体化，不放宽共享范围；不自动扩容/压缩/快照。稀疏后端含格式开销，须实测镜像逻辑长度与实际占用均 ≤80 MiB，满盘/稀疏/小文件攻击不能越界，否则此规格不采用，重开设计。开始须有 ≥256 MiB 宿主余量，且没有未回收卷；不声称限制 guest CPU/IO 或宿主其它使用者的磁盘消耗。

符号均由宿主生成：`R` 为既定私有评测根，逐段确认非链接、当前 UID、0700；`A` 为其下用 128-bit 随机值排他创建的 attempt 目录（0700）。`I=A/mailbox.sparseimage`、`M=A/<另一个128-bit随机值>`（空目录、0700）、`S=A/seed`；`N` 为随机 ASCII 共享名。全部是可信的单独 argv 参数，不接受 guest/模型/公开配置提供路径。只向 Tart 导出 `M/mailbox`，不导出 `A`、`R` 或卷根。

| 顺序 / S11b 阶段 | 动作、持久事实与成功条件 | 失败与时限 |
| --- | --- | --- |
| 0 · `Idle` | 独占清理责任，先执行下述 orphan recovery；核验 Stop/安装清单/golden | 查询或锁不确定即拒绝 reserve；每查询 ≤5s |
| 1 · `Reserved → Cloned` | 沿用 VM 的先登记后 clone。账本精确绑定 run_id、clone incarnation/config、attempt 随机身份及卷资源记录；不能凭名称认领 | 丢回复只在同一截止内成功读回才算落地；不确定不继续 |
| 2 · `Cloned`，start 派发前 | 登记 A/M/S 的父目录身份与创建意图，再排他创建。S 仅含 `mailbox/{host-write,guest-write}` 及卷根 `.metadata_never_index`、`.fseventsd/no_log`；无个人文件；先排除 A 的备份 | 创建/容量检查失败 → `Revoking → Destroying`，本次永久 `not_run` |
| 3 · `Cloned` | 登记 create 意图；create 后确认 I 非链接、普通文件、nlink=1、当前 UID/0600，记录父目录/dev/inode及容量，排除 I 的备份。登记 attach 意图再挂载，读回并登记卷 UUID/FSID/设备与镜像对应 | create ≤20s；attach ≤10s；失败/超时不能当作没创建/没挂载，先精确查询，未知则隔离 |
| 4 · `Cloned` | 读回镜像→设备→卷 UUID→M 对应；核验真实 mount flags、所有权和容量。禁索引、排除 Time Machine；检查后才填充 host-write 的 binding/请求 | 每查询 ≤5s；卷准备整组 ≤40s；任一缺证不启动 VM，按上行清理 |
| 5 · `Cloned → Booting → Isolated` | start 前重验对应、StartIssued 及唯一 `--dir`；固定 Softnet 参数不变。inventory/probe 核对 boot/UID/config 与 S11e 否定证据 | 卷准备占 start 的120s，余下最多80s启动；probe每项2s/组60s；违约即 `not_run`/撤权 |
| 6 · `Revoking → Destroying` | 禁新工作、revoke/drain，不再续scope使用权；外层owner存活证明保持到封存。stop 并独立确认 VM、tart、softnet 均退出；关闭邮箱所有 FD。delete VM + 权威缺席确认后 detach | revoke≤10s，stop+确认≤20s；没有退出证明不得强卸载或删镜像，隔离 |
| 7 · `Destroying → Final` | detach 后确认精确设备/挂载均不存在，才以受保护父目录 FD unlink I、确认缺席；S 只按已登记固定条目 unlink/rmdir，最后删空 M/A、封存回执，绝不递归删 guest 树 | delete+卷清理合计≤20s；封存≤10s；任何资源/回执未确认 → `Quarantined`、`not_run`、停后续 |

### 固定 argv（全部尚未验证）

**无 shell、无 PATH 搜索、无 sudo/交互提权回退。** 子进程umask固定077，目录0700/文件0600；系统工具路径固定，Tart 路径取受核验安装清单。输出边收边限 ≤1 MiB，plist 禁外部实体且限深度/条目数；返回码不能代替读回。以下包括选项拼写、权限和语义全部为 **`not_run`，须 S11-0c 实测后才可实施**。

| 用途 | 完整 argv 模板 |
| --- | --- |
| 创建（S 已预置禁索引标记，避免首次挂载后才补） | `["/usr/bin/hdiutil", "create", "-size", "64m", "-fs", "HFSX", "-type", "SPARSE", "-layout", "NONE", "-volname", "GCU-MAILBOX", "-srcfolder", S, I]` |
| 镜像信息 | `["/usr/bin/hdiutil", "imageinfo", "-plist", I]` |
| 挂载，一次带齐限制 | `["/usr/bin/hdiutil", "attach", "-plist", "-noautoopen", "-nobrowse", "-owners", "on", "-mountOptions", "rw,nosuid,noexec,nodev", "-mountpoint", M, I]` |
| 设备/挂载对应 | `["/usr/bin/hdiutil", "info", "-plist"]`；`["/usr/sbin/diskutil", "info", "-plist", D]`（D 来自精确镜像对应的设备） |
| mount flags 读回 | `["/sbin/mount"]`，只核对该设备/M；无另一次 mount/remount，attach 必须从首次挂载即安装上述 flags |
| 索引/备份读回 | `["/usr/bin/mdutil", "-s", M]`；`["/usr/bin/tmutil", "addexclusion", M]`；`["/usr/bin/tmutil", "isexcluded", M]` |
| 绑定唯一邮箱启动 | `[TART, "run", CLONE_ID, "--net-softnet", "--net-softnet-block=0.0.0.0/0", "--net-softnet-allow=<获批endpoint /32>", "--dir", "<N>:<M>/mailbox"]`（两个尖括号参数均由私有适配器按固定格式填充） |
| 正常 detach；唯一一次有限重试 | `["/usr/bin/hdiutil", "detach", E]`；`["/usr/bin/hdiutil", "detach", "-force", E]`（E 为已核验的整个镜像设备） |

挂载要求 `nosuid/noexec/nodev/nobrowse` 且 **owners 开启**；不选 `noowners`，因为它会掩盖所有权异常。宿主检查 M 的受保护父目录、卷根与子目录 UID/mode/ACL、实际 FSID/UUID/容量，不能仅看 mount point 目录或 label。guest 文件仍可能全部映射宿主 UID；此检查不建立 guest 身份。

只对本卷预置禁 Spotlight/FSEvents 标记，须读回禁索引状态并实测无 mds 内容读取；Time Machine 排除只作用于 A/I/M（分别在 create 前、attach 前、导出前执行上述 addexclusion/isexcluded argv，以相应参数替代 M）。不改全局索引/备份设置；标记被系统忽略、权限不足或排除无法证明即阻断，不静默使用默认挂载。

正常 detach 最多5s；仅在停止证明仍有效、身份重验相符且有剩余预算时 force 一次（≤5s），不循环强卸载、不杀 mds/系统服务、不操作其它磁盘。VM delete+确认预留5s，镜像 unlink/挂载缺席确认预留5s，共享20s删除阶段；时间不足即隔离，不增加60s总清理预算。FS syscall 的 `O_NONBLOCK` 不保证磁盘 IO 有界：沿用 ADR 0018 外层调用方对 provider 子进程的截止/kill/reap，若无法确认退出或系统副作用，回执 unknown，停止新 attempt。

### 启动时幂等 orphan recovery

**只处理未封存账本行的完整 attempt 身份，不扫描名称前缀。** 在每次 create/attach/start/detach/unlink 前先持久登记意图和已知身份，覆盖成功但回复丢失的窗口；同名、相同卷标、相同 run 字符串都不够。

1. 新调用方在 `Idle` 取得清理责任，追加 `RecoveryNoted`；使用本次有界截止，绝不用已死 attempt 的截止。旧 attempt 永久 `not_run`，不重启 VM、不恢复评分或 Stop。
2. 用账本内 I 的父目录/dev/inode、卷 UUID 与 `hdiutil info`/`diskutil info` 的镜像→设备→挂载完整对应确认设备及 M；设备号可能复用，每次破坏性操作前重验。查询失败/不完整不是缺席。不得为恢复而重新挂载含 guest 内容的镜像。
3. 依 S11b 核验精确 VM incarnation/config，先 stop/确认 tart+softnet 退出，再 delete/缺席确认、detach/缺席确认、unlink/缺席确认。已确认完成的步骤幂等跳过；VM 名称缺席不能代替进程退出。
4. create/attach 回复丢失时，只在完整正向对应能建立时收回；尚未记录 inode、已发出 attach 却无法核验卷 UUID/对应、出现替代者、未知挂载、锁竞争或任何清理失败一律隔离，保留证据、禁止新分配。未 attach 且证明确无设备的镜像可按已记 inode 删除，不要求为此挂载。不得按名称前缀删目录/卸卷。

`Suspended` 保留原义：旧 owner 租约过期后不得 detach/unlink/delete，不封存，cleanup=unknown；新启动的 recovery owner 才能收回。恢复也只能在全部资源确认后 seal；未确认的磁盘操作不能被现有 fake 的 VM cleanup_ok 掩盖。

回执在 ADR 0018 §7 基础上增加：卷规格/程序版本及 argv 摘要、完整 attempt/VM/卷绑定、create/attach 意图与读回、flags/索引/备份检查、峰值容量、每步单调耗时/有效截止/退出码、detach/force次数及独立缺席证明、recovery 关联和 unknown 原因。精确资源定位只存卷外受保护账本；普通日志仅逻辑 id/枚举/长度/摘要，不记录 guest 原始名字/错误/载荷、凭据或个人路径。

## 3. 邮箱布局与消息协议

**原则：固定槽位、每次重新安全打开；磁盘消息不是 S3 scope 控制通道。** 宿主写 `host-write`、只读 `guest-write`；可信 guest supervisor 反向读写。目录权限只是附加检查，防模型修改仍依赖 §4。

| 目录 | 唯一文件白名单（精确 ASCII） | 大小/数量 |
| --- | --- | --- |
| `host-write` | `binding.msg`；`inventory.msg`、`probe.msg`、`bootstrap.msg`、`login.msg`、`run.msg`、`lease.msg`、`revoke.msg`，及每个名字对应的 `.tmp`（如 `lease.msg.tmp`） | 每文件含 framing ≤4096 bytes；至多16项；binding首次发布后不改 |
| `guest-write` | 同名七个操作的 `.msg` 及对应 `.msg.tmp`；无 binding | 每文件≤4096 bytes；至多14项；无其它目录或产物 |

卷根的宿主标记不在导出范围。邮箱根只允许上述两个目录。检查每层均不递归，最多读取白名单上限+1条目录项，超额/其它条目立即拒绝，不排序、不累积名字；每轮检查与解析总量≤128 KiB。目录剧烈变化不能靠“再扫描直到稳定”解决，有限一次复验失败即 `not_run`。

消息为 **4-byte 大端长度前缀 + 严格 UTF-8 JSON**，长度须等于实际余字节，整个文件≤4096；先固定缓冲读取，再解析。schema v1，拒绝未知/重复字段、额外尾随数据、嵌套>3、数组>16、超长字符串、非整数/溢出序号。字符串仅固定枚举或≤64-byte ASCII 身份，不接受路径、shell、URL、argv 或自由错误文本。

共同字段：`v=1, run_id, attempt_nonce, vm_config_digest, boot_id, op, seq, request_seq, status, payload`；摘要固定32-byte编码，nonce是公开的128-bit绑定值，**不是密钥或授权**。首次 inventory 请求 boot 未知用固定 null，回复须有 boot；随后精确绑定，变化即撤权。每个操作独立非零u64请求序列，从1递增；回复的seq/request_seq回显请求seq，每请求只发布一个不可变回复。binding只含宿主钉住的身份、安装/策略摘要与协议常量，无秘密、凭据、scope token或 HMAC secrets。

| 固定操作 | host-write → guest-write | 完成含义 |
| --- | --- | --- |
| inventory | `inventory.msg` → 同名文件；固定身份/状态快照 | boot/UID/desktop/config、在途操作的op/request_seq及pending/terminal结果、guest单调采样；无全桌面/任意路径列表 |
| probe | `probe.msg` → 同名文件，payload只含闭集 `Probe` | 一个预装无模型探针的 passed/failed/unknown，不执行传入命令 |
| bootstrap | `bootstrap.msg` → 同名accepted；完成经inventory | 固定 fixture/helper；S3私有FD/S4最终native ACK完成后才报完成，不能用磁盘 ACK 伪造 native ACK |
| login | `login.msg` → 同名accepted；完成经inventory | 固定无模型登录；owner在guest GUI完成链接；仅回元数据/ready，凭据与设备码永不进邮箱 |
| run | `run.msg` → 同名accepted；完成经inventory | 消费一次已准入运行，固定预装任务清单；inventory回terminal及有界评分，无任意prompt/程序入口 |
| lease | `lease.msg` → 同名确认 | 递增seq、当前绑定与下述采样；优先于普通操作，确认不等于VM清理证明 |
| revoke | `revoke.msg` → 同名确认 | 幂等撤权、释放输入、终止树；终态不可恢复；host仍须独立stop/退出证明 |

每方向单一发布者：同目录排他 `openat(O_CREAT|O_EXCL|O_NOFOLLOW)` 创建固定 `.tmp`，写满、fsync、关闭，`renameat` 到 `.msg`，再同步目录；绝不就地改已发布文件。临时文件也须为普通文件、nlink=1且≤上限，读者不解析其内容。`.tmp` 已存在/写满/同步或rename失败即终止本次，不覆盖未知对象。**同卷本地rename不等于virtiofs读者已获原子/及时视图**；fsync是否支持、目录同步/缓存/可见性均待S11-0c，未测不进入真实通道，不改用就地写作为回退。

宿主每次读取：从可信 M 的父 FD 验证当前挂载→逐段 `openat(O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)` 至 guest-write → 字节白名单及不跟链接的fstatat预检 → `openat(O_RDONLY|O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC)` 固定文件 → **同FD fstat** 必须普通文件、0<大小≤4096、nlink=1、预期FSID/dev → 有界读取至cap+1/EOF → 再fstat复验dev/inode/nlink/size/mtime/ctime，并用fstatat确认名字仍指向同一对象 → 校验framing/绑定/序号 → 关闭全部FD。目录与创建时已记身份及本轮前后身份均复验；打开失败、特殊文件或变化不做无限重试。预检有竞态，仍依赖nodev及同FD检查；host-write写入同样验证、排他建tmp，绝不截断guest放置的链接。

这些复验只能发现部分并发修改，**不是原子快照或来源认证**；即使元数据未变也只解析不可信的有界消息，安全依赖模型不可达、无通用命令及独立证据。所有FD只在本次有界轮询内使用并关闭，不能带入下个attempt或卸载之后。

最多一个副作用操作在途；inventory可独立取状态，lease/revoke为优先槽。每次inventory请求取新seq，快照绑定在途操作seq，故进度变化不会改写同seq回复。每操作只在消费前一回复后发下一请求；lease允许覆盖跳号但须严格增加。相同seq/相同字节只忽略，绝不重复执行/延长租约；同seq改内容、倒退/跨attempt/错boot/错op/错请求、非法跳号或未知状态均协议违约→永久`not_run`并清理。已处理旧槽的重复轮询不是重放执行；副作用无自动重发，截止内缺回复/完成结果记unknown；文件首次尚未出现仅为pending。revoke终态后迟到帧不能恢复。

### 租约、同步调度与现有 trait

**VM存活时宿主每≤1s发布递增lease，guest在最后有效租约后≤3s终止模型树；迟到文件不得续命。** 不比较两端墙钟：inventory返回guest单调采样`g`；host回显采样及lease seq，supervisor只允许该采样对应的固定`g+2s`截止，过期不接收，不按“首次读到文件的现在+3s”重置。预留≤1s给调度/终止整树，合计≤3s；采样/lease的往返延迟不满足余量即停止准入。重复采样不得推迟截止，boot变化或已过旧截止不可复活。具体时限仍须真机测量。

宿主死亡/取消即不再续租；supervisor到期撤S3、释放输入并杀净模型后代，独立S3 scope单调3s租约仍保留；两者不能互相代替。VM可能仍活着，外层owner回收；owner也死则cleanup=unknown、启动先recover。异常控制失联在VM外锁存持久Stop；新卷/clone不能清Stop。

[`GuestChannel`](../../crates/gcu-eval/src/disposable/tart/guest.rs) **不改trait及版本1**：七个方法仍只收固定操作、闭集Probe/seq和显式deadline；卷绑定在私有真实适配器构造时提供，不加路径/地址/密钥参数。小文件封包由适配器解码成有界结果数据，不产生准入能力。

**现有fake machine不是可直接替换传输的真实runner。** `machine/steps.rs`的login/run当前立即解析ready/最终score，且只允许Running时arm lease；真实接线须独立增加accepted/pending处理、绑定inventory后的首租约及bootstrap/login/run等待期间续租、inventory固定状态轮询，再在真实terminal后保留provisional分数。不增加trait方法，不重复run/login派发，不把accepted当成功评分。机器/调用方的这项修改未实现、未获fake测试证明。

清理还有同一接线限制：现有`arm_owner_lease`将外层owner存活和guest lease合在一次调用。真实接线须区分两者：既有外层owner的私有控制连接每≤1s提供绑定attempt/递增seq的存活证明，覆盖revoke、stop、detach直到seal；VM退出后无需再写guest文件，guest lease也永不恢复scope。沿用同一同步调用方，不新增监视者；每个慢子进程分片轮询。owner续期仍须发送前/采纳前检查旧截止，已过期一律`Suspended`，不能用本地now自续或以VM已停豁免检查。此分离与pending接线同属M3独立评审门槛，当前fake不能证明60s真实清理可完成。

同步调用方每轮优先Stop/revoke、续租，再轮询状态；每次邮箱IO/解析片≤100ms，长login/run只确认已发起，等待分片回到同一调用方，无线程/observer。若virtiofs延迟或磁盘阻塞无法满足，`not_run`，不延长租约。保留`io.rs`唯一截止入口：`min(调用方,阶段,attempt绝对截止,所属聚合)`；卷流程也必须接入它，不能另造now+预算。准备从reserve入口≤10min，run≤min(task,15min)，清理≤60s；查询5s、probe2s/组60s、bootstrap/ACK5s、login5min不变，长等待不能占住续租或Stop。

**Stop常量与fail-closed完全不变**：100ms锁存、500ms释放、5s native、4750ms driver及已有平台重试额度；邮箱/VM预算不是Stop预算。过期在状态上不可逆进入`Suspended`，续租发送前/接受结果前及每次破坏性派发前复验旧租约；不增加自动resume。

## 4. S11e guest supervisor 要求（仅要求）

**原则：先在无凭据准备副本安装并封存可信程序，模型不能安装控制方。** 不以邮箱bootstrap上传/执行代码，不使用`tart exec`、ssh或guest运行时sudo。

| 项 | S11e必须提交的证据 |
| --- | --- |
| 身份与部署 | 独立于模型的专用受保护主体；固定、不可被模型修改的二进制/启动配置/安装摘要，经准备副本的受限启动项随golden引导；guest启动只等固定邮箱，不执行磁盘给出的程序。变更重封存/签认，golden无登录态 |
| 邮箱不可达 | 在模型启动前施加不可由子进程撤销的OS sandbox；封闭`/Volumes/My Shared Files/<name>`、其它挂载别名、FD/IPC/调试旁路；无管理员、mount或修改sandbox能力。使用同模型身份的确定性攻击探针验证读/写/列举/rename/硬链接/继承FD/代访问全部拒绝；仅0700或不同UID不算通过 |
| 控制与生命周期 | 单一受控事件循环，无模型代码进入；持有整个模型树、fixture/helper和scope控制FD；派生/脱离进程组的后代也须可终止。父死/租约过期/满盘/邮箱断开先撤权，不自动重启模型或恢复Stop |
| S3/S4/S12与TCC | mailbox终止在supervisor；scope install/renew/revoke仍走guest内继承私有FD。专用helper双向实例/活代码身份验证、ACK后数据面、独立Stop/auth存储；无真实FD或S12读取失败即拒绝。TCC授给固定helper，不授模型，不借普通helper、管理员或共享UID继承权限 |
| owner不可替代动作 | 准备副本若需安装授权/GUI，owner仅完成所需授权及固定helper的Accessibility/Screen Recording（和既定守卫权限）；不请求免密sudo。跨UID/桌面会话的TCC、AX/SCK及clone保留必须逐attempt验证，不能假定可继承 |

## 5. Mini探针卡 S11-0c（实施前）

**每行都是无模型、无凭据、无宿主网络访问的独立一次性clone实验。** 先独立审阅本ADR/固定argv/回滚，再由owner授权此卡的卷操作；本任务不授权或运行它。保持Softnet block-all（需要配置的allow只用原获批endpoint，不发网络请求），不改宿主网络/防火墙、不为控制放网关。固定预装探针经guest GUI/可信启动项执行，不能靠ssh/exec；若尚无该入口，由owner在准备副本完成必要安装，不绕过S11e门槛。

每行绑定精确账本资源，宿主输出有界，最多60s实验负载、≤60s teardown；结束须证明VM/tart/softnet、挂载/设备/镜像全消失，失败隔离并停后续。结果只附逻辑id、版本、耗时、摘要及checked/unknown，不外推native-verified。

| 实验 | 精确测量 / 成功条件（任一未满足即阻断相关实施） |
| --- | --- |
| C1 rename/延迟 | 双向反复发布带seq/校验图样的4096-byte记录100次，同时轮询；只能见完整旧/新记录，无撕裂/混合；记录p50/p99/max、覆盖/丢更新；单轮读取≤100ms、采样+lease往返<1s且不超过租约余量 |
| C2 fsync/可见性 | 对比文件fsync、目录fsync及rename前后可见性，含写者在各点退出；记录错误/耗时和跨virtiofs缓存结果；成功发布必须完整可读，未完成不能当ACK，不宣称fsync保证guest/宿主断电持久性 |
| C3 对抗文件与目录 | guest创建symlink（含跨边界）、hardlink、FIFO/socket，尝试device（无权限也记录）；替换/缩截/稀疏巨文件、大小写/Unicode别名、满目录小文件并发churn；宿主不得跟链接/阻塞/越界或超限分配，异常必拒绝，外卷合成sentinel不变 |
| C4 mount限制 | 原样验证create/attach选项及HFSX/容量/ACL；读回nosuid/noexec/nodev/nobrowse、owners；只用合成对象测执行/setuid/device拒绝及不同UID访问。确认guest视角不受这些flags自动保护，模型隔离仍须S11e另证 |
| C5 hdiutil时序/恢复 | 逐步测创建/挂载/查询/删除的max；在意图后/成功回复前终止调用方，重启按精确身份恢复；满足§2子预算；替代同名卷/复用设备号绝不被清理；不能正向识别时必须隔离 |
| C6 busy detach | 记录guest持有文件/目录的影响；先stop并证整树退出，再仅由有界探针子进程故意持有合成busy FD，normal/force各至多一次；记录退出码/延迟，探针最终关FD并kill/reap；必须独立确认设备/挂载缺席，超时不删除仍挂载镜像；生产不故意保留busy FD |
| C7 提前detach | 仅本实验在clone运行时卸载该专用卷，观察Tart/virtiofs读写错误、缓存、进程存活及后续同路径空目录；不得落写宿主底层目录，supervisor按期限停树且宿主判not_run，不把该顺序用于生产teardown |
| C8 属主映射 | 两个guest测试主体分别create/chmod/chown/rename，记录宿主UID/GID/ACL/nlink/dev/inode与guest所见差异；证实host-write不能靠宿主属主区分写者；身份核验/安全打开规则不因映射放宽 |
| C9 容量耗尽 | 分别填普通数据、扩稀疏文件、建许多小文件直至ENOSPC；卷≤64MiB、后端逻辑/占用≤80MiB、无宿主其它目录增长；满盘无法发布lease/回复须失败关闭，仍能在清理预算内detach/delete |
| C10 Spotlight/备份 | 从首次挂载到有guest文件，读回禁索引/备份排除并观察mds/Spotlight/FSEvents及Time Machine访问/占用、busy影响；不得索引邮箱内容或备份I/A/M，不改变其它卷设置，系统自动条目不得混入导出根 |

## 6. 残余风险、not_run与分步交付

**容量上限只约束这一个卷；邮箱不证明guest可信。** 宿主内核文件系统/virtiofs/Tart/Softnet、provider、supervisor/helper仍属TCB；恶意元数据和IO可造成拒绝服务。检查不能消除同文件并发改写，OS拒绝终止/卸载时只能隔离。镜像删除只是逻辑删除，不保证SSD安全擦除；备份/索引否定证据不足即不运行。

无IP控制并不证明egress已验收：ADR0018 §5的LAN/metadata/DNS/IPv6/UDP/QUIC、endpoint服务边界与计数证据仍须S11c补齐；不因删除guard方案而删探针或降低要求。endpoint可泄漏、凭据短期/无refresh、native权限与进程级/窗口级残余全部沿用ADR0017/0018。非目标：通用文件交换/日志导出/代码分发、IP/串口/块盘回退、自动恢复、扩大scope、替代S3/S4/S12或启用真实模型。

**明确 `not_run`：** 全部§2 argv/限容卷/挂载限制/索引与备份/强卸载/崩溃恢复；§3原子性/时延/协议/真实pending接线；S11e主体分离/OS sandbox/真实FD与ACK/TCC；block-all+邮箱自主运行、egress证明、父死/宿主重启/整树、原生Stop、登录/凭据及模型评测。既有§14双向可见事实不升级这些项目；本文仅运行文档链接/空白检查，不编译Rust。

每卡一个可审阅的Grok session，默认门保持关闭；独立审阅最多三轮，重复问题用单一原则简化，优先删除机制，禁止加observer/线程补洞。

| 卡 | 单次session交付 / 进入条件 | owner动作（仅必要处） |
| --- | --- | --- |
| S11-M1 | 独立审阅本ADR尤其卷流程/威胁模型/预算与S11b衔接；未通过不做任何真机操作 | 审阅具体宿主卷操作范围与回滚，决定是否授权S11-0c |
| S11-M2 | S11-0c model-free probe card与结果；先有可信guest探针入口，失败停并缩减设计 | 仅准备副本安装/GUI所需授权；不登录模型、不改网络 |
| S11-M3 | 离线协议codec与安全打开/发布的fake；固定槽位、竞态、长度与重复/乱序拒绝，独立评审 | 无 |
| S11-M4 | S11b真实接线前的离线pending、首租约、guest/owner存活分离及同步轮询；所有IO经唯一截止入口，过期不可复活 | 无 |
| S11-M5 | 卷资源账本与完整销毁屏障的fake；覆盖每个副作用前后/丢回复/替代身份/恢复，不能先seal再卸载 | 无 |
| S11-M6 | 仅已测固定argv的宿主卷与Tart私有适配；接入M5的create/attach/detach/unlink及恢复，不加生命周期管理器；准入仍拒绝 | 无（只在M1批准的操作范围内） |
| S11-M7 | S11e专用supervisor/golden/sandbox；无模型替身树验证邮箱不可达、脱离进程组后代、迟到lease/父死/满盘≤3s停树、Stop保留 | 仅无凭据准备副本的安装授权与封存签认 |
| S11-M8 | S11e阶段admission及S3/S4/S12真实FD/native ACK/TCC接线与否定证据；跨主体/权限不成立即阻断 | 固定helper TCC点击及变更签认 |
| S11-M9 | 联合S11c/d/f做无模型真实隔离、Stop、故障与整树/卷清理验收回执；S11g仍单独排期 | 审阅回执；必要真实用户恢复；模型登录/运行留待矩阵§8全满足后 |
