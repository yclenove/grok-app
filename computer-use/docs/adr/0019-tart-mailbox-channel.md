# 0019 · Tart 邮箱控制通道与宿主专用卷（S11）

状态：**Proposed** · 日期：2026-11-10 · 细化 [ADR 0018 §14](0018-tart-disposable-provider.md#14-h1-决定不经-ip-的控制通道owner-2026-11-10-选择方案-2)。

**仅设计；round-1 独立评审为 ACCEPT-AFTER-FIXES，本次修订待复审；未实施或操作 Mini、卷、网络。** [评审原文](../../../docs/plans/computer-use-v2/reviews/2026-11-10-mailbox-design-review.md)不变。本文不安装 provider、不铸造准入能力、不改变 S6 门禁或[验收矩阵 §8](../real-eval-acceptance-matrix.md#8-开启真实运行的门槛)。路径相对 `computer-use/`；argv 均为待测候选。

## 1. 决定、事实与威胁模型

**原则：邮箱是唯一受限共享目录例外，不是可信文件系统。** owner 已同意邮箱例外及经审阅的宿主卷设计；生产 H1 仍无 IP 控制通道、不用 SSH、不放行网关。Softnet 保持 `block 0.0.0.0/0`（含网关）+ 获批模型 endpoint `/32`，无宿主 guard/pf。只导出每 attempt 新建限容卷内的 `mailbox`；其它宿主目录、剪贴板、拖放、设备/agent 转发仍禁用。§5 的 DEFAULT 网络机械实验是单独标明的实验室例外，不改变此决定。

已测事实仅沿用 ADR 0018 §13/§14：Mini/macOS 27/Tart 2.41.1，virtiofs 双向可见、宿主新文件约 1s 可见；guest 可写 >3 MiB 及 symlink，宿主所见属主为宿主用户，**没有配额**。guest 路径为 `/Volumes/My Shared Files/<name>`。`tart exec` 不可用；不采用 `--disk`/`--serial-path`；`eval` 无免密 sudo。约 1s 的观察不证明租约可行。

全部 guest 字节、名字、元数据及声称的 supervisor 响应都不可信。宿主只认精确 ASCII 白名单、普通文件、同 FD 的 nlink/大小/dev/FSID 检查及有界解析；不跟链接、不执行产物、不接收路径/shell/URL/argv/凭据/HMAC 密钥，不递归遍历或清理 guest 树。大小写/Unicode 别名、跨卷硬链接尝试、FIFO/socket/device、稀疏文件、目录 churn、并发改写均须覆盖；属主相同不证明写者。`noexec` 等只限制宿主挂载，不隔离 guest。

**模型必须完全不可达邮箱。** S11e 证明不同安全主体与实际 OS sandbox 同时生效：所有后代不能列举/读写/rename 邮箱及别名、继承 FD、经 helper/relay/fixture 代访问、debug/ptrace supervisor、读其内存/句柄、替换程序/启动项或取得管理员能力。仅 UID/0700/隐藏路径/私有 FD 不够；任一否定探针失败就不启动模型。

## 2. 单次 attempt 的宿主卷流程

**原则：先登记、后副作用；先证明停止、后卸载；全部资源确认后才 seal。** 沿用既有调用方/provider 与外层 owner，无 daemon、observer、新线程、共享生命周期管理器或 Drop 清理。账本、回执和持久 Stop 在卷外且不导出。

候选为 **64 MiB、HFSX（区分大小写、不启 journal）、单文件 SPARSE、layout NONE**；不扩容/压缩/快照。实测 live 镜像 `st_size` 与 `st_blocks × 512 bytes` 均须 ≤80 MiB，create scratch 另记路径/峰值且合计 ≤80 MiB；开始有 ≥256 MiB 宿主余量且无未回收卷。此界限约束专用文件系统与 live 镜像，不限制 CPU/IO，也不消除宿主 APFS COW/快照保留；禁止并另查备份/快照副本，不能证明则阻断。

宿主生成 `R`（既定私有根）、排他随机 128-bit 目录 `A`、`I=A/mailbox.sparseimage`、随机空挂载目录 `M`、`S=A/seed`、共享名 `N`。逐段 no-follow 核验当前 UID、目录 0700/文件 0600、ACL；路径仅为可信独立 argv 参数。S 的固定合成树为 `mailbox/{host-write,guest-write}`、`host-stage`、`.metadata_never_index`、`.fseventsd/no_log`；只导出 `M/mailbox`，不导出卷根、`M/host-stage`、A 或 R。

**持久资源记录：** R 与 A 的父/子 dev、inode、UID/mode/ACL；A 内 I、**未挂载的底层 M₀**、S 各自的父/子身份；S 的每个固定目录、marker 文件也逐项记录。创建意图先落盘，创建后身份落盘才继续。另记挂载根 V 的 UUID、`fstat` dev/inode、`fstatfs(fd)` FSID、全部挂载子目录（mailbox、两子目录、host-stage）的身份及 I→整个镜像设备→卷→M 对应；V 与被覆盖的 M₀ 是不同对象，detach 后必须核对 M₀，不能比较 V 的 inode。

| 阶段 | 动作与成功条件 |
| --- | --- |
| `Idle → Reserved → Cloned` | 新 owner 先按下述清单 recover 未封存行，核验持久 Stop/安装/golden，先登记精确 run/attempt/clone incarnation/config 再 clone；准备存活证明从 reserve 入口开始 |
| `Cloned` | 先登记再排他创建 A/M/S/各 seed 项；排除 A 备份；登记 create 意图，核验 I 身份/容量，排除 I/底层 M 备份；登记 attach 意图，建立 V 与整个设备的对应 |
| `Cloned → Booting → Isolated` | 受控 attach 后读回 flags/owners/布局/marker/索引/备份，全部通过才写 binding、导出含 guest 字节的邮箱；start 前复验唯一 `--dir` 与固定 Softnet argv；首 inventory 绑定 boot 后才启 guest 租约，再逐项 probe |
| `Revoking → Destroying` | 禁新工作，revoke/drain；不再续模型/scope 使用权，外层 owner 证明保持到 seal。stop 并独立证明 VM/tart/softnet 整体退出，关生产邮箱 FD；delete VM/权威缺席后 detach |
| `Destroying → Final` | 全设备/挂载缺席后 unlink I；S 仅逐项清理有身份的固定合成树，最后 rmdir 空 M₀/A；VM、卷、通道、目录与回执都确认才 seal，否则隔离/`not_run`、停后续 |

### 固定 argv 与无提权测量清单（全部 not_run）

所有操作以**实际 eval 用户、无 root/sudo、无交互授权**测试；无 shell/PATH 搜索，umask 077，程序来自固定系统路径/核验安装清单。输出边收边限 ≤1 MiB，plist 禁外部实体并限深度/条目。选项不支持、GUI 授权/Full Disk Access 要求、忽略限制、证据不可读均阻断，绝不升级权限。

| 候选 argv（单独参数） | 必须测得的权限与语义 |
| --- | --- |
| `/usr/bin/hdiutil create -size 64m -fs HFSX -type SPARSE -layout NONE -volname GCU-MAILBOX -srcfolder S I` | 组合选项支持、无 journal、后缀/路径、根布局/权限、marker 实际复制、全部临时设备/挂载/scratch 与服务活动。S 此时仅合成内容；`-srcfolder` 可能内部挂载，预置 marker **不证明任何首次挂载前已存在**；若必须限制所有内部挂载而此配方做不到，就拒绝该配方 |
| `/usr/bin/hdiutil attach -plist -noautoopen -nobrowse -owners on -mountOptions rw,nosuid,noexec,nodev -mountpoint M I` | 非交互用户权限；真实 flags/owners、UUID/FSID/容量/ACL、plist 字段与精确设备映射。限制须从这次受控 attach 生效，静默缺项不接受；禁止 remount 回退 |
| `/usr/bin/hdiutil imageinfo -plist I`；`/usr/bin/hdiutil info -plist`；`/usr/sbin/diskutil info -plist D`；`/sbin/mount`；`/usr/bin/mdutil -s M` | 只读检查是否可由 eval 用户获取、可靠限量解析；D 来自精确映射，label 不是 UUID。不是授权 mount/diskutil 变更或 `mdutil -i off` |
| `/usr/bin/tmutil addexclusion X`；`/usr/bin/tmutil isexcluded X`，X 为 A/I/M | create 前排除 A，attach 前排除 I/底层 M，导出前读回挂载根 M 实际覆盖；测 sticky 排除的对象类型、是否跨挂载保持、卷是否另需排除；root/FDA/授权提示即阻断，不擅加 `-p`/`-v`，成功退出不等于排除生效 |
| 根 marker `.metadata_never_index`、`.fseventsd/no_log` | 不是访问控制；以 eval 用户可用的服务日志/文件访问观测、索引状态/合成 canary 查询及实际备份读回证明效果，C10 定义有限窗口；接口不可读/覆盖不足就是 unknown，不改全局服务 |
| `TART run CLONE_ID --net-softnet --net-softnet-block=0.0.0.0/0 --net-softnet-allow=<获批endpoint /32> --dir <N>:<M>/mailbox` | 核验安装清单与实际进程/配置/唯一导出；实验室 DEFAULT argv 仅限 §5 |
| `/usr/bin/hdiutil detach E`；`/usr/bin/hdiutil detach -force E` | 普通用户对**整个镜像设备 E**的 normal/force 权限、选取语义/耗时/权威缺席；仅按下述身份清单、有限 force 一次 |

### 破坏性身份清单（正常清理与 recovery 共用）

1. **检查不等于生效时绑定。** `hdiutil detach /dev/diskN` 即便刚查过 inode/UUID，设备号仍可被别的宿主 attach/detach 复用；`unlinkat` 的父 FD 固定祖先，不能原子锁住子名。eval cleanup 锁只约束参与者。路径式操作须有可信宿主独占变更前提：从映射核验到操作完成，无其它手工/自动磁盘 attach/detach、热插拔或目标路径替换；实验在 owner 明确保留的窗口暂停这些作业、禁止并发工具并记入回执。M2 测设备复用/映射与命令语义，M5 注入检查后替换；反复查询不能证明消除竞态。无法建立该排他前提时，必须有**经测量在 effect 时仍绑定 attachment 身份**的操作，否则不发破坏性 argv，隔离；不靠 daemon/宿主 guard 补洞。
2. 仅选择未封存账本的精确 attempt，追加 `RecoveryNoted`，核对 clone incarnation/config、I 的已记父/dev/inode、UUID/FSID、整个设备及所有挂载对应；每次 effect 前复验租约/身份/停止屏障。未知不是缺席，不按前缀/名字/label 认领，不重挂含 guest 内容的镜像，不触碰替代者或其它磁盘。
3. VM/tart/softnet 退出证明有效后才 normal detach；仅剩余预算足够、身份/排他条件仍有效时 force 一次。不杀系统服务。必须证明该镜像**未在任何位置挂载且已无 attachment**才 unlink I；未 attach 的已记 inode 镜像也须先证明无设备。`possibly attached` 永不授权 unlink。
4. 每个 unlink/rmdir 用核验父 FD、no-follow 与已记子身份；detach 后核验 M₀，无意外条目才 rmdir。丢 mkdir 回复、只有意图无子 inode、替换目录/镜像、丢 create/attach 回复无法正向对应、detach 回复丢失未读回缺席，均隔离，无递归/同名兜底；意图孤儿可无限保留。已确认缺席的步骤幂等跳过。
5. 过期旧 owner 永久 `Suspended`：不 delete/detach/unlink/seal，cleanup=unknown；仅新 recovery owner 用新的有界截止接手，旧 attempt 永久 `not_run`。异常未封存状态先在 VM 外保留/锁存 Stop，再考虑新分配；不能自动 resume。

### 唯一预算表（宿主单调绝对截止，含 kill/reap）

| 范围 | 上限及共享关系 |
| --- | --- |
| 准备 reserve→ready | ≤10min；安装/每查询 ≤5s，clone ≤60s；start ≤120s **含**卷准备 ≤40s（create ≤20s、attach ≤10s），未耗尽的 start 余量才供启动 |
| guest 操作 | probe ≤2s/项、组 ≤60s；bootstrap/最终 ACK ≤5s；login ≤5min；run ≤min(task.max_seconds,15min) |
| 清理 | 总 ≤60s：revoke/drain 10s + stop/确认 20s + delete/卷清理 20s + seal 10s；删除阶段 VM/确认 5s、normal detach 5s、唯一 force 5s、unlink/缺席 5s 共用 |
| 调度/租约 | 两次 Stop/存活检查的**总间隔 ≤100ms**；host 发布/外层 owner 证明各 ≤1s；guest grant 截止 `g+2s`、预留 ≤1s 撤权/整树终止，总 ≤3s；独立 S3 scope 3s 不变 |

子预算是上限，不保证各项最大值能同时装入；不足立即停止/隔离，不延长总截止。回执记完整资源绑定、意图/身份/读回、版本/argv 摘要、权限/flags/索引/备份、容量峰值、每步有效截止/耗时/退出码、force 次数、独立缺席、recovery/unknown。定位仅在卷外受保护账本；普通日志仅逻辑 id/枚举/长度/摘要，不记原始 guest 内容或个人路径。

## 3. 邮箱布局、codec 与调用方状态机

**原则：固定槽位；发布已创建且检查的对象，读取仍作不可信输入。** 导出根只含 `host-write`/`guest-write`。host-write 只有 `binding.msg` 与七操作 `inventory/probe/bootstrap/login/run/lease/revoke.msg`（≤8项）；guest-write 有七个 `.msg` 及对应 `.msg.tmp`（≤14项）。各文件 framing 在内 ≤4096 bytes；binding 首次发布后不变，仅含版本、钉住的 run/attempt/VM/安装/策略摘要与协议常量，无秘密或 scope token。未导出的 host-stage 只供宿主固定 `.msg.tmp`（≤8项），不是第二个共享目录。

每方向单一发布者。宿主从**同卷、不导出且 guest 不可达**的已核验 host-stage 父 FD 排他 `openat(O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC)` 建 tmp；保留 FD，经 fstat 普通/nlink=1/大小/dev 与 `fstatfs(fd)` FSID 核验、写满/fsync、复验，再 `renameat(stage_fd,tmp,host_write_fd,slot)`，同步两父目录后关 FD。源名始终在受保护父中，不能把 guest 可替换的导出 tmp 当对象能力；导出边界、`..`/硬链接别名不能到达 stage 须实测，否则阻断。目标目录须匹配已记身份并前后复验；固定目标允许 absent 或上次已发布槽（预检替代者即违约），rename 原子替换其目录项、不跟随目标链接；目标名在检查后被换掉也不改变受保护源对象，事后异常即失败。guest 之后仍能改写/移走导出对象，发布不证明来源。guest 回复以本目录 tmp→msg 发布，宿主不信任该过程；tmp 已存在/任何同步或 rename 失败即停止，无就地写回退。

宿主每次读：可信 M 父 FD 核验当前挂载→逐段 `openat(O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)` 到 guest-write→字节白名单/no-follow fstatat 预检→固定文件 `openat(O_RDONLY|O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC)`→**同 FD fstat** 普通、0<size≤4096、nlink=1、预期 dev，**fstatfs(fd)** 核验 FSID→固定 cap+1 缓冲读至 EOF→再 fstat 核对 dev/inode/nlink/size/mtime/ctime，fstatat 复验名字仍为该对象→解码/绑定检查→关 FD。目录与已记身份亦前后复验；每层只读白名单上限+1项，总轮询 ≤128 KiB，不排序、不递归、不扫描到稳定。FIFO 调换依赖非阻塞 open，device 调换还依赖已证 nodev；路径查询不替代同 FD 检查。

元数据复验不是原子快照，不能消除硬链接/同文件改写竞态；读回永远不是认证。FD 只限本次有界 IO，除发布源 FD 保留至本次发布结束外不跨 poll，更不跨 attempt/卸载。同卷 rename、fsync 不证明 virtiofs 的及时/原子视图或断电持久性，均须实测；O_NONBLOCK 也不保证磁盘 syscall 有界，沿用外层 owner 的 provider 截止/kill/reap，无法确认退出/副作用就 unknown 并停新 attempt。

线格式：4-byte 大端长度 + 严格 UTF-8 JSON，长度等于余字节；固定缓冲后解析，拒绝未知/重复字段、尾随数据、嵌套>3、数组>16、溢出/非整数序号。字符串仅闭集枚举或 ≤64-byte ASCII 身份，摘要为固定32-byte编码。七操作帧共同字段为 `v=1,run_id,attempt_nonce,vm_config_digest,boot_id,op,seq,request_seq,status,payload`；请求两 seq 相同，回复原样回显；nonce 是公开128-bit绑定，不是密钥。首次 inventory 请求 boot=null，终态回复必须给 boot，后续精确绑定，变化即撤权。请求 status=request，回复仅 accepted/terminal；pending 是本地未完成状态。

[`GuestChannel`](../../crates/gcu-eval/src/disposable/tart/guest.rs) **七方法及版本1不变**。M3 解码为规范化 `BoundedOutput {state:pending|accepted|terminal,op,request_seq,result}`；文件未出现/有效操作未完成返回 pending，不能伪装成 CallError。真正 IO/协议错误仍失败关闭。私有适配器保存当前请求/seq；同一逻辑操作重复 poll 不再发命令。下表 payload 为闭集，除所列字段外禁止；序号/毫秒为 u64，checked arithmetic，UID 为 u32，布尔/枚举按类型验证。四个异步操作的 terminal result 另含 `outcome=ok/failed/unknown/cancelled`：仅 ok 时下表值字段有效，其余须为 null，无自由错误文本；中断/失败不得当完成证明。

| 方法 | request payload；reply/完成契约 |
| --- | --- |
| inventory | `{consume_op,consume_request_seq,consume_revoke_seq}`（无消费用 null/0）；仅 terminal 快照 `{uid,desktop,config,sample_ms,active_op,active_request_seq,active_status,active_result,revoke_request_seq,revoke_status,revoke_result}`。active 是一个 probe 以外的普通副作用，另列优先 revoke；状态 none/pending/terminal，none 配 null/0，pending 的 result=null，terminal 的 result 依原操作 schema。快照自身 seq 不变，下一次采样另发请求 |
| probe | `{probe:闭集Probe}`；无 accepted，pending 后仅 terminal `{check:passed/failed/unknown}`，同一 Probe/seq 才能消费 |
| bootstrap | `{}`；accepted `{}` 只说明已启动，terminal 经 inventory 的 active_result `{native_ack_generation:u64}`；必须独立核对 S3/S4 最终 native ACK/S12/真实 FD，磁盘字段不是 ACK |
| login | `{}`；accepted `{}`，经 inventory terminal `{login:ready/missing}`；只回元数据，无设备码/链接/凭据 |
| run | `{}`；accepted `{}`，经 inventory terminal `{score:pass/fail/partial/timeout/crash}`；固定预装任务，只记 provisional，清理封存前不计分 |
| lease | `{inventory_request_seq,sample_ms}`；pending 后 terminal `{inventory_request_seq,sample_ms,until_ms}`，必须等于该已发 grant；确认不产生 owner/scope/准入或清理证明 |
| revoke | `{}`；accepted `{}` 不算 drain；经 inventory 的 revoke_result `{scope_revoked:true,input_released:true,tree_exited:true}` 才是 terminal；host 仍独立 stop/退出确认，撤权不可恢复 |

普通副作用至多一个在途，inventory 独立，lease/revoke 优先。guest 同 seq 回复不可变，普通槽保留到下一请求隐含消费；bootstrap/login/run 的 terminal 与被 revoke 中断的 cancelled 终态保留到 host 已消费后由后续 inventory 的 consume 字段确认（最多 active+revoke 两项）。适配器仅在 terminal 成功交回机器、机器同一步记录消费后排入确认；accepted/pending 不消费。消费确认精确匹配保留终态，已消费确认幂等，错误确认违约。丢失/过期即 unknown，不重放副作用。M4 必须改 bootstrap、probe、login、run、lease、revoke 和 startup inventory 的全部解析/状态转移，不能仅改 login/run，不能由任何 accepted 产生评分、native ACK、cleanup proof 或 admission。

各 op seq 从1连续发出、无回绕；已消费旧槽的同 seq/同字节重复只视为仍在等当前请求，同 seq 改内容/其它错 op、错请求/boot/attempt、未来或未发序号一律违约。**lease 是可覆盖例外**：host 仍逐次递增，guest 可跳过观察；每 attempt inventory/lease 各最多2048请求，保存全部已发 lease 的 `(seq,inventory_request_seq,sample_ms,until_ms)` 及已见回复摘要，超限即撤权。属于本 boot/attempt 的已实际发出且内容一致的旧 lease ACK，即使首次见到也只丢弃，不延长任何截止；不在该有界表或内容不一致则失败。M3 覆盖 codec/覆盖槽/延迟帧，M4 决定采纳与截止。

### 租约与同步调度（M4）

`sample_ms=g` 是 guest **本 boot 的连续单调毫秒**，须实测 VM pause/宿主休眠期间推进语义；若恢复后可续用实际已过期 grant 就阻断。guest 保留本 boot 实际生成的有界 inventory `(request_seq,g)` 表，只接受引用其中真实样本、非未来、`g+2000` 不溢出的 grant；重复样本不推迟截止。host 回显该样本，guest 只取固定 `g+2s`，不按收到时刻重新起租；新 grant 须在旧截止前到达，首次 grant 也不得已过自身截止。旧截止一到即不可复活，预留最后1s撤权/释放/杀净树，总≤3s。测 sample→guest接受年龄、有效 grant 间隙、负载下终止完成时间；C1 约1s量级可见性不足以放行，不能延长租约/Stop。

外层 owner 经既有私有控制连接每≤1s给绑定 attempt/递增序号的证明，**从 reserve/准备到 seal**；不得用本地 now 自续。guest grant 仅从首 inventory 绑定 boot 到开始 revoke 前续期，VM 退出后不写邮箱；它与 owner 存活、S3 scope 租约各自独立。发送前、接受结果前及破坏性派发前检查旧 owner 截止，过期永久 Suspended；revoke 后 lease/迟到 terminal 均不能恢复 scope。provider 死由外层 owner 回收；两者皆死只由 guest 到期停树，不声称删除 VM；重启先 recovery/保留 Stop。

**具体调用契约：** 同一个同步调用方在既有 owner 内持命令/provider 子进程句柄与 pending 记录；每命令 begin 一次，每 step 只 poll 一个有界片后返回。每轮先读 Stop/owner 证明并检查所有旧截止，处理 revoke/到期/适用的 guest 续租，再推进一片 IO/状态；全部排队、IPC、解析与 IO 的总间隔受预算表100ms约束，不能串行16个 probe 或多个100ms片才返回。reserve 读回、卷准备、start/inventory/probe、bootstrap/login/run、revoke、stop、delete/detach、查询和 seal 全部走此循环；慢命令不在 runner 方法中等待退出，无线程、observer 或 runner 回调暗续租。

M4 首次 begin 保存 `op_end=min(初始caller,开始+cap,phase_end,aggregate_end,attempt_end)`；prep 从 reserve 入口、start 从卷准备入口、Task 从首次 run、cleanup 从首次撤权/清理入口起算。唯一 `machine/io.rs` 入口每片再取 `min(本次caller,已存op_end/phase_end,attempt硬截止,所属聚合)`，包含 kill/reap 余量。query 可有自己的短 cap，始终被原 bootstrap/login/**Task aggregate**/probe组/cleanup end 限住；pending/accepted/新 inventory 不重置原 end，不重启命令。到期发生在片内则返回后先丢弃迟到成功、停止新派发、kill/reap；无法确认 effect 则 unknown/隔离，已发破坏性操作不能声称被撤销。离线 trace 必含慢查询/连续 probe/4s stop/清理时 owner 证明、片内过期、kill/reap 与不复活；当前 fake/`io.rs` 的 Task 无 aggregate、立即 ACK/score、Running 才 arm lease、revoke 任意成功 body 均不能作为真实接线。

**Stop 不变量不变：100ms 锁存、500ms 释放、5s native、4750ms driver及既有平台重试额度；无自动 resume。** 邮箱/VM 预算不增加这些常量，FS 阻塞不能满足调度则真实通道不准入。

## 4. S11e guest supervisor 要求（仅要求）

可信 supervisor 在无凭据准备副本安装/封存，独立受保护主体、固定程序/启动配置摘要，受限启动项只启动该程序并等邮箱；不从邮箱装代码，不用生产 SSH/exec/guest sudo。M2 的确定性机械探针独立于此实现，不能替代其安全证据。

S11e 须证明模型 OS sandbox 与邮箱/别名/FD/IPC/debug 旁路全部关闭；supervisor 单事件循环持有 fixture/helper、scope 私有 FD 与整个模型树，包括脱离进程组后代。控制丢失/父死/过期/满盘先撤权、释放、杀树，无自动重启。S3 install/renew/revoke 仍走 guest 内继承 FD，S4 最终 ACK/S12 活代码身份/真实 TCC 缺一拒绝；不借普通 helper、共享 UID 或管理员继承权限。准备副本安装/固定 helper 权限只由 owner 必要 GUI 操作，跨 UID/桌面/TCC/clone 保留须逐 attempt 验证，变更重新封存签认，无登录态。

## 5. Mini 探针卡 S11-0c（机械实验，先于 S11e）

**采纳 round-1 建议：C1–C10 的纯机械部分可用 DEFAULT-Softnet 一次性 clone + key-only SSH 驱动。** 实验 argv 为 `TART run CLONE_ID --net-softnet --dir <N>:<M>/mailbox`（无 block/allow）。最小入口是事先在无个人数据/无登录态准备副本装好的、固定摘要的确定性探针程序；经受限 SSH 固定命令启动，不依赖 M7 supervisor/golden/sandbox。预备两个非管理员测试主体，各只操作自身对象；跨 UID/chown 无权即记录 permission-denied，缺第二身份/需权限的子例 unknown/后移，不发明 guest sudo。

owner 已同意邮箱例外与经审阅卷设计；**运行前仍需 owner 一行 notice，明确本次 DEFAULT 网络机械实验期间网关暴露**。此例外仅在实验期间使用默认 Softnet：既测宿主网关 22/53/5900/445/7000 可达，key 认证不限制 egress，丢弃 clone 不能撤销已发生的外部影响。依赖审查过的镜像/启动脚本与确定性代码；不改宿主网络/防火墙，不重开生产 H1。

钉住 guest SSH host key 与精确 clone/IP/boot 映射，不自动接受 host key。专用受限测试身份/一次性 key，私钥只留宿主；`ssh -F /dev/null -i <专用key>`、独立 known_hosts、`GlobalKnownHostsFile=/dev/null`、`StrictHostKeyChecking=yes`、`IdentitiesOnly=yes`、`IdentityAgent=none`、`BatchMode=yes`、仅 publickey/禁止密码与键盘交互，禁 agent/X11/所有端口转发（`ClearAllForwardings=yes`）、TTY、继承配置/ProxyCommand/ProxyJump；只准固定经审阅命令，无通用远程 shell/交互认证。输出有界、每命令显式 deadline≤60s（建连≤5s、含外层超时 kill/reap），预装探针本地亦有负载截止；任何 SSH/映射不确定即结束/隔离。

无模型/eval 凭据、个人挂载、外部下载或有意 guest 网络请求（仅此 host→guest SSH driver 连接）。SSH **不提供被测 lease、预期文件字节或权威 mount/cleanup 结果**；图样由两端预装程序独立生成，guest/host 本地计时，guest 摘要经同卷固定 ASCII/≤4096-byte 测试结果槽读回（独立测试白名单，不混用生产帧），宿主直接读文件/设备/挂载/账本。SSH stdout 只记 driver 诊断。结果一律标 **DEFAULT-network / mechanics-only**；不证明生产 egress、S11e、控制存活或 real-eval。C7 自主终止及所有 transport/liveness 验收移到 M7/M9，在 block-all 下用无害替身树重测。

每个下表**子例独立 attempt**：绑定精确账本、合成 canary（卷内/跨 export/卷外皆无个人内容），负载≤60s、teardown≤60s，并应用§2更短命令/阶段 cap。先到工作量或时间上限即停止；未完成记 unknown，watchdog 不是文件系统失败。每次报样本数、观测点、max/错误及是否完整，少量样本不报可信 p99。teardown 全资源缺席才可开始下一例，失败隔离。探针只读诊断/输出也限量；C7 的运行中 detach 是唯一故意偏离生产 stop-first 的机械子例。

| 子例 | 有限负载、观测与判定 |
| --- | --- |
| C1 rename/延迟 | 两方向各8次4096-byte seq/图样发布，每次可见性等待≤3s；两端本地记录 write/fsync/rename/首次完整读时刻、丢更新与 sample 往返，仅完整旧/新帧可接受，混合/撕裂即失败；另测打开/读/发布片 max 与100ms门槛。慢可报告机械数据；往返已≥2s或无法给固定 grant 留调度余量则阻断真实通道实施，不能以发布频率救活；通过有限样本仍须 M7/M9 block-all/负载下的接受年龄、grant 间隙与终止 gate |
| C2 fsync | 文件同步/目录同步/rename 三边界各一个退出故障子例、各3次发布；记录支持/错误/缓存与完整旧或新字节，未完成绝不当 ACK；无断电持久性结论 |
| C3 对抗身份/打开 | 每种各一子例、≤32次受控替换/≤64次读取：预植写 tmp 链接；open 后 tmp 换名/替身（stage 应不可达、导出替身不能被发布）；预检后普通→FIFO/device；目录替换；rename-into-place；跨 export/卷外 hardlink；反复读时 mutation/replacement；大小写/Unicode别名、特殊文件。用同步故障点覆盖窗口，核对发布源 inode/dev、拒绝/非阻塞/限内存与所有 canary 不变；hardlink/device syscall 被拒本身只是一条观察，不放宽检查 |
| C4 挂载/首挂载 | create 与受控 attach 各一子例，≤12项固定选项/权限探测；本地记录内部设备/挂载/scratch、根 marker 复制/布局/服务活动；回读 flags/owners/ACL/容量，合成执行/setuid/device/跨 UID 测试无权项 unknown；不把宿主 flags 当 guest sandbox |
| C5 崩溃/恢复 | 每个副作用的意图后、效果成功但回复前各一故障点，每 attempt 仅1次故障/≤12次查询；含丢 mkdir/seed/create/attach 回复、detach 成功未 ACK、替换目录/镜像 inode、设备号复用及检查后 retarget；M5 可控故障注入先验拒绝，真机仅对已登记合成资源，不销毁替代者，缺身份保持隔离 |
| C6 busy detach | 文件/目录 FD 各一例，guest 持1个合成FD至stop并记录影响；先 stop 并证 VM 整树退出，再仅1个受限宿主探针进程持 busy FD 测 normal/force 各最多一次，记 max/退出码/独立缺席；探针关 FD/kill/reap，超时不 unlink，生产不保留此 busy FD |
| C7 提前 detach | clone 活着时仅卸本专用卷，guest 本地各8次有界读写，观察错误/缓存/Tart存活；宿主核对底层 M₀ 身份与内容无落写。只记机械现象/异常阻断，不断言自主停树；该断言及控制丢失时限只属 M7/M9 |
| C8 主体映射 | 两个预备主体各≤12次 create/chmod/chown-attempt/rename，记录两侧 UID/GID/ACL/nlink/dev/inode 与拒绝；不靠 chown 提权，不以宿主属主识别写者 |
| C9 容量 | 普通写≤80×1MiB、稀疏/truncate≤16次且声明≤1GiB、小文件≤8192×1KiB、churn≤4096次，各独立例；填满子例未到 ENOSPC 记 unknown，其它报告实际逻辑/分配变化。期间每批及结束测 I 的 st_size/blocks×512、create scratch、清理 held-FD（复用C6单FD），要求§2界限；只观测预声明 scratch/备份/索引存储，另列正常有界账本/回执，不能声称全宿主无增长；峰值抽样非普遍证明，满盘 liveness 后移 M7/M9 |
| C10 索引/备份 | 索引/FSEvents/备份各30s子例、≤16个1KiB合成 canary；从 create 内部挂载到导出后读回 marker/排除，并用 eval 可读服务日志、文件访问记录/索引查询、备份覆盖读回观察。先证明观测接口覆盖该卷；看不到系统服务、未覆盖某阶段、没有实际备份作业/只见空闲均 unknown，不能据此证明无读取/备份；需提权或全局变更则配方阻断 |

## 6. 残余风险、not_run 与分步交付

TCB 仍含宿主内核/文件系统/virtiofs/Tart/Softnet、provider、supervisor/helper；恶意 IO 可拒绝服务，检查不能消除并发改写，OS 不执行终止/卸载时只能隔离。镜像删除是逻辑删除，不保证 SSD 擦除；备份/索引/容量/身份证据不足就不运行。

无 IP 控制不证明 egress：ADR 0018 §5 的 LAN/metadata/DNS/IPv6/UDP/QUIC、endpoint 服务边界与计数仍须 S11c，IPv4 `/32` 不替代这些证明。endpoint 泄漏、短期凭据/无 refresh、进程/窗口残余及 S3/S4/S12/TCC 全沿用 ADR 0017/0018。全部新卷/权限/协议/时序/恢复/真实接线、安全隔离/Stop/登录与模型评测仍 **not_run**；仅旧双向可见为已测。本文只做文档检查，admission 默认关闭，无 IP/串口/块盘回退、通用文件交换/执行或自动恢复。

每卡一个可审阅 session，独立审阅最多三轮；一个义务只归一张卡，禁止用 observer/线程补洞。

| 卡 | 交付与进入条件 | 必要 owner 动作 |
| --- | --- | --- |
| S11-M1 | 本修订独立复审，未通过不操作宿主 | 已同意邮箱/经审阅卷设计；实验前按§5一行 notice |
| S11-M2 | S11-0c 纯机械探针/无提权清单及 DEFAULT-network/mechanics-only 结果，**不依赖 S11e/M7**；失败缩减设计 | 必要准备副本安装/两个测试主体，不登录模型 |
| S11-M3 | **codec/open/publication**：离线固定槽/源名替换/同FD竞态/有界状态帧与过时 ACK 解码；安全打开/发布 fake 独立审阅 | 无 |
| S11-M4 | **全部 pending/lease/scheduling/deadline machine**：七操作完成契约、消费保留/覆盖跳过/迟到 ACK、首租约、准备到seal的owner证明与guest分离、所有慢命令/绝对Task end/片内到期/kill-reap trace；独立审阅后才真实接线 | 无 |
| S11-M5 | 卷账本/持久目录身份与完整销毁屏障 fake：含丢 mkdir/替换目录/M₀与V/丢回复/生效前retarget/recovery；`cleanup.rs`/`recover.rs` 禁 VM-only 成功先 seal | 无 |
| S11-M6 | 仅 M2 已测固定 argv 的宿主卷/Tart 私有适配，接 M5，不增生命周期管理器，准入仍拒绝 | 只在已审阅宿主范围内 |
| S11-M7 | S11e supervisor/golden/sandbox；**block-all** 下无害替身树测自主 lease/迟到/父死/满盘/提前detach及脱组后代终止、模型邮箱不可达与 Stop 保留 | 必要准备安装/封存签认 |
| S11-M8 | S11e 阶段 admission 与 S3/S4/S12 真实FD/native ACK/TCC 接线/否定证据 | 固定helper TCC点击/变更签认 |
| S11-M9 | 联合 S11c/d/f，**block-all** 重测 transport/liveness/负载时限、隔离/Stop/整树/卷清理验收；SSH结果不能计入；S11g 单排 | 审阅回执/必要用户恢复；矩阵§8全满足后才另排登录/模型 |
