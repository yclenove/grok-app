# 0019 · Tart 邮箱控制通道与宿主专用卷（S11）

状态：**Proposed** · 日期：2026-11-10 · 细化 [ADR 0018 §14](0018-tart-disposable-provider.md#14-h1-决定不经-ip-的控制通道owner-2026-11-10-选择方案-2)。

**仅设计；未实施或操作 Mini、卷、网络。** 路径相对 `computer-use/`；argv 均为待测候选。

## 1. 决定、事实与威胁模型

**邮箱是唯一受限共享目录例外，不是可信文件系统。** owner 已同意邮箱例外及经审阅的宿主卷设计；生产 H1 无 IP 控制通道，不用 SSH/exec/guest sudo，不放行网关。
Softnet 保持 `block 0.0.0.0/0`（含网关）+ 获批模型 endpoint `/32`，无宿主 guard/pf。只导出每 attempt 新建限容卷内的 `mailbox`；其它宿主目录、剪贴板、拖放、设备/agent 转发禁用。
§5 DEFAULT 机械实验例外不改变此决定。

已测仅 ADR 0018 §13/§14：Mini/macOS 27/Tart 2.41.1，virtiofs 双向可见、宿主新文件约 1s 可见；guest 可写 >3 MiB 及 symlink，宿主所见属主为宿主用户，**没有配额**。
guest 路径为 `/Volumes/My Shared Files/<name>`。`tart exec` 不可用；不采用 `--disk`/`--serial-path`；`eval` 无免密 sudo。可见性不证明租约可行。

guest 字节/名字/元数据/supervisor 响应均不可信。宿主只认精确 ASCII 白名单、普通文件、同 FD 的 nlink/大小/dev/FSID 检查及有界解析；不跟链接、不执行产物，不接收路径/shell/URL/argv/凭据/HMAC 密钥，不递归遍历或清理 guest 树。
须覆盖大小写/Unicode 别名、跨卷硬链接、FIFO/socket/device、稀疏文件、目录 churn、并发改写；属主不证明写者。`noexec` 等只限制宿主挂载，不隔离 guest。

**模型必须完全不可达邮箱。** S11e 证明不同安全主体与实际 OS sandbox 同时生效：所有后代禁列举/读写/rename 邮箱及别名、继承 FD、helper/relay/fixture 代访问、debug/ptrace supervisor、读其内存/句柄、替换程序/启动项或取得管理员能力。
仅 UID/0700/隐藏路径/私有 FD 不够；任一否定探针失败就不启动模型。

## 2. 单次 attempt 的宿主卷流程

**先登记、后副作用；先证明停止、后卸载；全资源确认后才 seal。** 沿用既有调用方/provider 与外层 owner，无 daemon、observer、新线程、共享生命周期管理器或 Drop 清理。账本、回执和持久 Stop 在卷外且不导出。

候选为 **64 MiB、HFSX（区分大小写、不启 journal）、单文件 SPARSE、layout NONE**；不扩容/压缩/快照。
live I 的 `st_size`/`st_blocks × 512 bytes` 均须 ≤80 MiB，create scratch 记路径/峰值且合计 ≤80 MiB；开始宿主余量 ≥256 MiB、无未回收卷。
此界限仅约束专用文件系统/live I，不限制 CPU/IO 或消除宿主 APFS COW/快照保留；禁备份/快照副本并另查，证据不足阻断。

宿主生成排他随机 128-bit 目录 A、随机空挂载目录 M 与共享名 N。逐段 no-follow 核验当前 UID、目录 0700/文件 0600、ACL；路径仅作可信独立 argv 参数。
**创建意图先落盘，身份落盘才继续。** 身份表中的父/子身份含 dev、inode、UID/mode/ACL。

| 资源 | 持久身份/关系 |
| --- | --- |
| R / A | 既定私有根/排他子目录，均记父/子身份 |
| I=`A/mailbox.sparseimage` | 父/子身份；I→整个镜像设备→卷→M 精确对应 |
| S=`A/seed` | 父/子身份；固定合成树 |
| seed 固定项 | `mailbox/{host-write,guest-write}`、`host-stage`、`.metadata_never_index`、`.fseventsd/no_log`；每个目录/marker 的父/子身份 |
| M₀ | A 内未挂载底层 M 的父/子身份 |
| V | 挂载根 UUID、`fstat` dev/inode、`fstatfs(fd)` FSID；覆盖 M₀，勿混用两者 inode |
| 挂载子目录 | mailbox、host-write、guest-write、host-stage 各自身份 |

唯一导出 `M/mailbox`；卷根/host-stage/A/R 禁导出。

| 阶段 | 动作与成功条件 |
| --- | --- |
| `Idle → Reserved → Cloned` | 新 owner 按[破坏性清单](#destroy) recover；核验 Stop/安装/golden，登记精确 run/attempt/clone incarnation/config 后 clone |
| `Cloned` | 按账本/argv 清单排他建 A/M/S/seed/I、排除备份、attach；持久化资源表各身份/映射 |
| `Cloned → Booting → Isolated` | attach 后读回 flags/owners/布局/marker/索引/备份，全通过才写 binding、导出含 guest 字节的邮箱；start 前复验唯一 `--dir`/固定 Softnet argv；首 inventory 绑定 boot 后启 guest 租约，再逐项 probe |
| `Revoking → Destroying` | 禁新工作，revoke/drain；stop/证退出后关生产邮箱 FD；按破坏性清单 delete VM/证缺席后 detach |
| `Destroying → Final` | 按下述清单清 I/S/M₀/A；VM/卷/通道/目录/回执全确认才 seal，否则隔离/`not_run`、停后续 |

<a id="no-elevation"></a>

### 固定 argv 与无提权测量清单（全部 not_run）

所有操作以**实际 eval 用户、无 root/sudo、无交互授权**测试；无 shell/PATH 搜索，umask 077，程序来自固定系统路径/核验安装清单。
输出边收边限 ≤1 MiB；plist 禁外部实体、限深度/条目。选项不支持、GUI/Full Disk Access 授权、限制无效/证据不可读均阻断，禁提权。

| 候选 argv（单独参数） | 必须测得的权限与语义 |
| --- | --- |
| `/usr/bin/hdiutil create -size 64m -fs HFSX -type SPARSE -layout NONE -volname GCU-MAILBOX -srcfolder S I` | 组合支持、无 journal、后缀/路径、根布局/权限、marker 复制、全部临时设备/挂载/scratch/服务活动；适用下述内部挂载 gate |
| `/usr/bin/hdiutil attach -plist -noautoopen -nobrowse -owners on -mountOptions rw,nosuid,noexec,nodev -mountpoint M I` | 非交互权限；真实 flags/owners、UUID/FSID/容量/ACL、plist/精确设备映射。限制从受控 attach 生效，静默缺项拒绝；禁 remount 回退 |
| `/usr/bin/hdiutil imageinfo -plist I`；`/usr/bin/hdiutil info -plist`；`/usr/sbin/diskutil info -plist D`；`/sbin/mount`；`/usr/bin/mdutil -s M` | eval 只读/限量解析；D 来自精确映射，label 不是 UUID。不授权 mount/diskutil 变更或 `mdutil -i off` |
| `/usr/bin/tmutil addexclusion X`；`/usr/bin/tmutil isexcluded X`，X=A/I/M | create 前排除 A，attach 前排除 I/底层 M，导出前读回挂载根实际覆盖；测 sticky 对象类型/跨挂载保持/卷是否另需排除；禁擅加 `-p`/`-v`；退出成功不等于生效 |
| 根 marker `.metadata_never_index`、`.fseventsd/no_log` | 非访问控制；eval 可读服务日志/文件访问、索引状态/canary 查询与实际备份读回证明效果；C10 有限窗口，覆盖不足 unknown，不改全局服务 |
| `TART run CLONE_ID --net-softnet --net-softnet-block=0.0.0.0/0 --net-softnet-allow=<获批endpoint /32> --dir <N>:<M>/mailbox` | 安装清单、实际进程/配置/唯一导出；DEFAULT argv 仅限 §5 |
| `/usr/bin/hdiutil detach E`；`/usr/bin/hdiutil detach -force E` | 普通用户对整个镜像设备 E 的 normal/force 权限/选取语义/耗时/权威缺席；仅按下述清单 force 一次 |

`-srcfolder` 时 S 仅合成内容；create 可能内部挂载，预置 marker **不证明任何首次挂载前已存在**。
若必须限制所有内部挂载而此配方做不到，就拒绝配方；不能仅观察后来的受控 attach。

<a id="destroy"></a>

### 破坏性身份清单（正常清理与 recovery 共用）

1. **检查不等于生效时绑定。** 刚查 inode/UUID 也挡不住 `/dev/diskN` 复用；`unlinkat` 父 FD 不锁子名，eval cleanup 锁只约束参与者。
   路径操作须有可信宿主排他窗口：映射核验至完成，无其它手工/自动磁盘 attach/detach、热插拔或目标路径替换；owner 明确保留窗口、暂停作业/禁并发工具，记回执。
   M2 测设备复用/映射/命令语义，M5 注入检查后替换；重查不消除竞态。无法建立排他前提时，须有**经测量在 effect 时仍绑定 attachment 身份**的操作，否则不发破坏性 argv，隔离；不靠 daemon/宿主 guard。
2. 只选未封存账本的精确 attempt，追加 `RecoveryNoted`，核对 clone incarnation/config、I 已记父/dev/inode、UUID/FSID、整个设备/所有挂载对应；每次 effect 前复验租约/身份/停止屏障。
   未知不是缺席，不按前缀/名字/label 认领，不重挂含 guest 内容的镜像，不碰替代者或其它磁盘。
3. VM/tart/softnet 退出证明有效后才 normal detach；仅剩余预算足够、身份/排他条件仍有效时 force 一次，不杀系统服务。
   证明镜像**未在任何位置挂载且已无 attachment**才 unlink I；未 attach 的已记 inode 镜像也须先证明无设备；`possibly attached` 永不授权 unlink。
4. 每个 unlink/rmdir 用核验父 FD、no-follow 与已记子身份；detach 后核验 M₀，无意外条目才 rmdir。
   丢 mkdir 回复、只有意图无子 inode、替换目录/镜像、丢 create/attach 回复无法正向对应、detach 丢回复未读回缺席，均隔离；无递归/同名兜底，意图孤儿可无限保留。确认缺席的步骤幂等跳过。
5. 过期旧 owner 永久 `Suspended`：不 delete/detach/unlink/seal，cleanup=unknown；仅新 recovery owner 用新的有界截止接手，旧 attempt 永久 `not_run`。
   异常未封存状态先在 VM 外保留/锁存 Stop，再考虑新分配；不能自动 resume。

### 唯一预算表（宿主单调绝对截止，含 kill/reap）

| 范围 | 上限及共享关系 |
| --- | --- |
| 准备 reserve→ready | ≤10min；安装/查询各 ≤5s，clone ≤60s；start ≤120s **含**卷准备 ≤40s（create ≤20s、attach ≤10s），未耗尽的 start 余量才供启动 |
| guest 操作 | probe ≤2s/项、组 ≤60s；bootstrap/最终 ACK ≤5s；login ≤5min；run ≤min(task.max_seconds,15min) |
| 清理 | 总 ≤60s：revoke/drain 10s + stop/确认 20s + delete/卷清理 20s + seal 10s；删除阶段 VM/确认 5s、normal detach 5s、唯一 force 5s、unlink/缺席 5s 共用 |
| 调度/租约 | Stop/存活检查及 pending 推进总间隔 ≤100ms；host 发布/外层 owner 证明各 ≤1s；guest grant `g+2s`，预留 ≤1s 撤权/整树终止，总 ≤3s；独立 S3 scope 3s 不变 |

子预算最大值不保证可并取；不足即停/隔离，不延总截止。
回执记资源绑定、意图/身份/读回、版本/argv 摘要、权限/flags/索引/备份、容量峰值、每步有效截止/耗时/退出码、force 次数、独立缺席、recovery/unknown。
定位仅在卷外受保护账本；日志仅逻辑 id/枚举/长度/摘要，不记原始 guest 内容或个人路径。

## 3. 邮箱布局、codec 与调用方状态机

导出根只含 `host-write`/`guest-write`；发布/读取遵循下列步骤。
host-write 只有 `binding.msg` 与七操作 `inventory/probe/bootstrap/login/run/lease/revoke.msg`（≤8项）；guest-write 有七个 `.msg` 及对应 `.msg.tmp`（≤14项）。各文件含 framing ≤4096 bytes。
binding 首次发布后不变，仅含版本、钉住的 run/attempt/VM/安装/策略摘要与协议常量，无秘密或 scope token。未导出 host-stage 只供宿主固定 `.msg.tmp`（≤8项）。

### 宿主发布与读取

每方向单一发布者；宿主发布：

1. 从**同卷、不导出且 guest 不可达**、已核验的 host-stage 父 FD 排他 `openat(O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC)` 建 tmp，保留源 FD。
2. 同 FD `fstat` 核验普通/nlink=1/大小/dev，`fstatfs(fd)` 核验 FSID。
3. 写满。
4. fsync，复验源身份。
5. 核验目标父目录已记身份；目标只准 absent/上次发布槽，替代者违约。
6. `renameat(stage_fd,tmp,host_write_fd,slot)`，同步两父目录。
7. 复验目标目录/发布对象，异常失败。
8. 关源 FD。

源名须在受保护父中，导出 tmp 不是对象能力；实测导出边界、`..`/硬链接别名不可达 stage，否则阻断。
rename 原子替换目标目录项、不跟目标链接；检查后目标被换不改变受保护源对象。guest 仍可改写/移走导出对象，发布非来源证明。
guest 回复在本目录 tmp→msg 发布，宿主不信任；tmp 已存在/同步或 rename 失败即停，无就地写回退。

宿主读取：

1. 可信 M 父 FD 核验当前挂载，逐段 `openat(O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)` 到 guest-write；目录与已记身份前后复验。
2. 字节白名单/no-follow `fstatat` 预检；每层只读白名单上限+1项，不排序/递归/扫描到稳定。
3. 固定文件 `openat(O_RDONLY|O_NOFOLLOW|O_NONBLOCK|O_CLOEXEC)`。
4. **同 FD fstat** 核验普通、0<size≤4096、nlink=1、预期 dev；**fstatfs(fd)** 核验 FSID。
5. cap+1 固定缓冲读至 EOF，总轮询 ≤128 KiB。
6. 再 fstat 比较 dev/inode/nlink/size/mtime/ctime，fstatat 复验名字仍指该对象；解码/绑定检查，关 FD。

FIFO 调换依赖非阻塞 open，device 调换还依赖已证 nodev；路径查询不替代同 FD 检查。
元数据复验非原子快照，硬链接/同文件改写竞态仍在；读回非认证。
FD 只限本次有界 IO（发布源保留至本次发布结束），不跨 poll/attempt/卸载。rename/fsync 不证明 virtiofs 及时/原子视图或断电持久性，须测。
O_NONBLOCK 不保证磁盘 syscall 有界；沿用外层 owner 的 provider 截止/kill/reap，无法确认退出/副作用则 unknown、停新 attempt。

<a id="operations"></a>

### 七操作与消费

线格式：4-byte 大端长度 + 严格 UTF-8 JSON，长度等于余字节；固定缓冲后解析，拒绝未知/重复字段、尾随数据、嵌套>3、数组>16、溢出/非整数序号。
字符串仅闭集枚举/≤64-byte ASCII 身份，摘要固定32-byte编码。共同字段为 `v=1,run_id,attempt_nonce,vm_config_digest,boot_id,op,seq,request_seq,status,payload`；请求两 seq 相同，回复原样回显；nonce 是公开128-bit绑定，不是密钥。
首次 inventory 请求 boot=null，terminal 必须给 boot，后续精确绑定，变化即撤权。请求 status=request，回复仅 accepted/terminal；pending 是本地未完成状态。

[`GuestChannel`](../../crates/gcu-eval/src/disposable/tart/guest.rs) **七方法及版本1不变**；M3 规范化 `BoundedOutput {state:pending|accepted|terminal,op,request_seq,result}`。
未出现/未完成返回 pending，真正 IO/协议错误才 CallError/失败关闭。私有适配器存请求/seq，同一操作重复 poll 不重发。
下表为闭集 payload；序号/毫秒 u64、checked arithmetic，UID u32，布尔/枚举按类型验证。bootstrap/login/run/revoke 的 accepted 均为 `{}`，terminal 另含 `outcome=ok/failed/unknown/cancelled`；仅 ok 时值字段有效，其余为 null，无自由错误文本；中断/失败不算完成证明。

| 方法 | request 字段 | 完成位置 | result 字段 |
| --- | --- | --- | --- |
| inventory | `{consume_op,consume_request_seq,consume_revoke_seq}`；无消费 null/0 | 本槽 terminal 快照 | `{uid,desktop,config,sample_ms,active_op,active_request_seq,active_status,active_result,revoke_request_seq,revoke_status,revoke_result}` |
| probe | `{probe:闭集Probe}` | 本槽 terminal，无 accepted | `{check:passed/failed/unknown}` |
| bootstrap | `{}` | accepted 后 inventory.active_result | `{native_ack_generation:u64}` |
| login | `{}` | accepted 后 inventory.active_result | `{login:ready/missing}` |
| run | `{}` | accepted 后 inventory.active_result | `{score:pass/fail/partial/timeout/crash}` |
| lease | `{inventory_request_seq,sample_ms}` | 本槽 terminal | `{inventory_request_seq,sample_ms,until_ms}` |
| revoke | `{}` | accepted 后 inventory.revoke_result | `{scope_revoked:true,input_released:true,tree_exited:true}` |

inventory 的 active 是 probe 以外一个普通副作用，另列优先 revoke；状态 none/pending/terminal：none 配 null/0，pending 的 result=null，terminal 按原操作 schema。快照自身 seq 不变，下一采样另发请求。
probe 只消费同 Probe/seq；lease terminal 必须等于该已发 grant，确认不产生 owner/scope/准入/清理证明。
bootstrap 仍独立核对 S3/S4 最终 native ACK/S12/真实 FD，磁盘字段不是 ACK。login 仅元数据，无设备码/链接/凭据；run 只执行固定预装任务，结果 provisional，清理封存前不计分。
revoke 不续模型/scope 权，accepted 不算 drain；terminal 后 host 仍独立 stop/证退出，撤权不可恢复；任何 accepted 都不能产生评分、native ACK、cleanup proof 或 admission。

普通副作用至多一个在途，inventory 独立，lease/revoke 优先；普通槽留到下一请求隐含消费。
bootstrap/login/run terminal（含 revoke 中断的 cancelled）及 revoke terminal 保留至 host 消费/后续 inventory.consume 确认，最多 active+revoke 两项。
仅 terminal 成功交回且机器同一步记录消费后才排确认；accepted/pending 不消费。确认精确匹配保留终态，已消费确认幂等，错误确认违约。丢失/过期即 unknown，不重放副作用。

例（字段按上表）：`login/1 request` → `accepted {}` → inventory 报 login/1 terminal/ok/ready → 机器记消费 → 后续 inventory 以 `consume_op=login,consume_request_seq=1` 确认。

各 op seq 从1连续发出、无回绕；已消费旧槽的同 seq/同字节重复仅表示仍等当前请求；同 seq 改内容、错 op/请求/boot/attempt、未来/未发序号均违约。
**lease 是可覆盖例外**：host 仍逐次递增，guest 可跳过观察。每 attempt inventory/lease 各最多2048请求；保存全部已发 lease `(seq,inventory_request_seq,sample_ms,until_ms)` 及已见回复摘要，超限撤权。
本 boot/attempt 实发且内容一致的旧 lease ACK（含首次见到）只丢弃、不延截止；不在表/内容不符则失败。

<a id="schedule"></a>

### 租约与同步调度（M4）

`sample_ms=g` 是 guest **本 boot 连续单调毫秒**；须测 VM pause/宿主休眠推进语义，恢复能续用已过期 grant 则阻断。
guest 保留本 boot 实际生成的有界 inventory `(request_seq,g)` 表，只接受引用真实样本、非未来、`g+2000` 不溢出的 grant；重复样本不推迟截止。
host 回显样本，guest 只取固定 `g+2s`，不按收到时刻起租；新 grant 在旧截止前到达，首次 grant 不得已过自身截止。
**旧截止一到不可复活**，最后1s撤权/释放/杀净树，总≤3s。测 sample→接受年龄、有效 grant 间隙、负载终止完成时间；不能延长租约/Stop。

外层 owner 经既有私有控制连接每≤1s给绑定 attempt/递增序号的证明，**reserve/准备到 seal**；不得用本地 now 自续。
guest grant 仅从首 inventory 绑定 boot 到开始 revoke 前续期，VM 退出后不写邮箱；与 owner 存活、S3 scope 租约独立。
发送/接受结果/破坏性派发前查旧 owner 截止，过期永久 Suspended；revoke 后 lease/迟到 terminal 不恢复 scope。
provider 死由外层 owner 回收；两者皆死仅由 guest 到期停树，不声称删除 VM；重启先 recovery/保留 Stop。

同一同步调用方在既有 owner 内持命令/provider 子进程句柄与 pending 记录；每命令 begin 一次，每 step 只 poll 一个有界片后返回。每轮：

1. 读 Stop/owner 证明，检查所有旧截止。
2. 处理 revoke/到期/适用的 guest 续租。
3. 推进一片 IO/状态后返回；排队、IPC、解析和 IO 都计入 ≤100ms 总间隔，不能串行16个 probe 或多个100ms片才返回。

reserve 读回、卷准备、start/inventory/probe、bootstrap/login/run、revoke、stop、delete/detach、查询、seal 全走此循环；runner 不等慢命令退出，无线程/observer/runner 回调暗续租。
**100ms 是服务检查/pending 推进的最大间隔，不是每 tick 新发 inventory/lease。** M4 须给有界 fresh-sample/grant 调度：覆盖最长准备/任务存活期（含最长 login/清理），计首样本/完成/消费采样，证明 inventory/lease 各≤2048、满足实测 grant-age/gap。
耗尽仍失败关闭；M4 trace 见§6。无可行调度即 gate 失败，不增租约/线程/observer。

首次 begin 保存 `op_end=min(初始caller,开始+cap,phase_end,aggregate_end,attempt_end)`。起点：

- prep：reserve 入口；start：卷准备入口。
- Task aggregate：首次 run；cleanup：首次撤权/清理入口。

`machine/io.rs` 唯一入口每片取 `min(本次caller,已存op_end/phase_end,attempt硬截止,所属聚合)`，含 kill/reap 余量。
query 可另有短 cap，仍受原 bootstrap/login/**Task aggregate**/probe组/cleanup end 限制；pending/accepted/新 inventory 不重置原 end，不重启命令。
片内到期则返回后先丢弃迟到成功、停新派发、kill/reap；effect 不明则 unknown/隔离，已发破坏性操作不能声称撤销。

**Stop：100ms 锁存、500ms 释放、5s native、4750ms driver及既有平台重试额度；无自动 resume。** 邮箱/VM 预算不增加这些常量；FS 阻塞无法满足调度则真实通道不准入。

<a id="supervisor"></a>

## 4. S11e guest supervisor 要求（仅要求）

可信 supervisor 在无凭据准备副本安装/封存：独立受保护主体、固定程序/启动配置摘要，受限启动项只启该程序并等邮箱；禁从邮箱装代码，遵守[§1](#1-决定事实与威胁模型)/[无提权规则](#no-elevation)。

supervisor 单事件循环持 fixture/helper、scope 私有 FD 与整个模型树（含脱离进程组后代）。控制丢失/父死/过期/满盘先撤权、释放、杀树，无自动重启。
S3 install/renew/revoke 走 guest 内继承 FD，S4 最终 ACK/S12 活代码身份/真实 TCC 缺一拒绝；不借普通 helper、共享 UID 或管理员继承权限。
准备副本安装/固定 helper 权限只由 owner 必要 GUI 操作；逐 attempt 验跨 UID/桌面/TCC/clone 保留，变更重封存签认，无登录态。

<a id="lab"></a>

## 5. Mini 探针卡 S11-0c（机械实验，先于 S11e）

C1–C10 纯机械部分可用一次性 DEFAULT-Softnet clone + key-only SSH；argv `TART run CLONE_ID --no-graphics --net-softnet --dir <N>:<M>/mailbox`（无 block/allow；`--no-graphics` 必需，Mini 上绝不起图形界面）。
最小入口为经审阅固定摘要确定性探针；controller 经同一受限 key-only SSH 向每个无个人数据/无登录态 DEFAULT clone 上传固定大小上限程序，按下列检查表核验/执行。
不改 golden、无需 owner 安装，不依赖 M7 supervisor/golden/sandbox；复制/执行不安全即 unknown，禁扩权/新增服务。
已有两个非管理员主体各仅操作自身对象；跨 UID/chown 无权记 permission-denied，缺第二身份/需权限则 unknown/后移；上传不授权建账户/提权。

**运行前需 owner 一行 notice，明确本次 DEFAULT 网络机械实验期间网关暴露**。
默认 Softnet 既测宿主网关 22/53/5900/445/7000 可达，key 认证不限制 egress，丢弃 clone 不能撤销已发生的外部影响。依赖审查过的镜像/启动脚本与确定性代码；不改宿主网络/防火墙，不重开生产 H1。

<a id="lab-driver"></a>

### 实验室 driver 检查表

- 传输/启动都钉住 SSH host key、精确 clone/IP/boot；不自动接受 key。专用受限测试身份/一次性 key，私钥仅宿主。
- `ssh -F /dev/null -i <专用key>`、独立 known_hosts、`GlobalKnownHostsFile=/dev/null`、`StrictHostKeyChecking=yes`、`IdentitiesOnly=yes`、`IdentityAgent=none`、`BatchMode=yes`。
- 仅 publickey，禁密码/键盘交互、agent/X11/所有端口转发（`ClearAllForwardings=yes`）、TTY、继承配置/ProxyCommand/ProxyJump。
- 只准此有界固定目的上传与经审阅固定核验/启动命令，无通用远程 shell/交互认证。scp 常用 SFTP，须核实现有 forced-command/key 路由兼容；不兼容则此变体 unavailable，不开放通用 SFTP、shell/legacy 回退，不加特权配置/服务。
- 宿主先核对源固定摘要/大小，guest 核对完整副本摘要后才执行。目的为受限身份新私有目录内固定 ASCII 路径、邮箱外；拒链接/特殊文件、意外属主/mode/替换，目录/文件防其它测试主体改写至执行结束。钉住解释器/依赖身份；禁包安装、外部下载、抓取脚本、guest 自选命令。
- 程序字节/输出、建连/传输/核验/执行/kill-reap 均受声明的 setup/attempt 预算限制；每命令 ≤60s，建连 ≤5s，探针本地亦有负载截止。丢结果/摘要不符结束子例，重试不重置聚合截止；SSH/映射不确定则结束/隔离。

### 实验证据边界与共用规则

无模型/eval 凭据、个人挂载、有意 guest 网络请求（仅 host→guest SSH driver 连接）；操作遵守[无提权规则](#no-elevation)。
SSH **仅部署代码/驱动**，不传被测邮箱流量、逐样本预期答案、lease 或权威结果；上传例外仅程序产物。
两端固定摘要程序按同一经审阅规格独立生成图样、各用本地时钟；guest 摘要经同卷固定 ASCII/≤4096-byte 测试结果槽读回（独立测试白名单，不混生产帧），宿主直接读文件/设备/挂载/账本。
SSH 摘要仅验证可信实验室部署，stdout 仅诊断；均不证明邮箱发布/mount/cleanup。结果标 **DEFAULT-network / mechanics-only**，不证明生产 egress、S11e、控制存活或 real-eval；自主终止及全部 transport/liveness 须§6 M7/M9 重测。
C7 自主终止及所有 transport/liveness 验收移到 M7/M9，在 block-all 下用无害替身树重测。

每个**子例独立 attempt**：绑定精确账本与合成 canary（卷内/跨 export/卷外无个人内容）；负载≤60s、teardown≤60s，并取§2更短命令/阶段 cap。
先到工作量/时间上限即停，未完成 unknown，watchdog 非文件系统失败；报样本数/观测点/max/错误/完整性，少量样本不报可信 p99；诊断/输出亦限量。
探针随 clone 移除，独立证明 clone/全部资源缺席才可下一例，失败隔离。仅 C7 运行中 detach 故意偏离生产 stop-first。

### C1 rename/延迟

- 负载：两方向各8次4096-byte seq/图样发布，每次可见性等待≤3s。
- 观测：两端本地 write/fsync/rename/首次完整读时刻、丢更新/sample 往返；打开/读/发布片 max 与100ms门槛。
- 判定：只接受完整旧/新帧，混合/撕裂失败。往返≥2s或固定 grant 无调度余量阻断真实实施，不能以发布频率救活；仍受 M7/M9 gate。

### C2 fsync

- 负载：文件同步/目录同步/rename 三边界各一个退出故障子例，各3次发布。
- 观测：支持/错误/缓存、完整旧或新字节。
- 判定：未完成绝不当 ACK，不得推断断电持久性。

### C3 对抗身份/打开

- 负载：每种各一子例，≤32次受控替换/≤64次读取；预植 tmp 链接、open 后 tmp 换名/替身、预检后普通→FIFO/device、目录替换、rename-into-place、跨 export/卷外 hardlink、反复读时 mutation/replacement、大小写/Unicode别名/特殊文件。
- 观测：同步故障点覆盖窗口；发布源 inode/dev、拒绝/非阻塞/限内存、全部 canary。
- 判定：stage 不可达，导出替身不能发布，canary 不变；缺覆盖 unknown。hardlink/device syscall 被拒只是一条观察，不放宽检查。

### C4 挂载/首挂载

- 负载：create 与受控 attach 各一子例，≤12项固定选项/权限探测。
- 观测：内部设备/挂载/scratch、根 marker 复制/布局/服务活动；回读 flags/owners/ACL/容量，合成执行/setuid/device/跨 UID 测试。
- 判定：§2配方/内部挂载 gate 全部适用；无权项 unknown，不把宿主 flags 当 guest sandbox。

### C5 崩溃/恢复

- 负载：每个副作用意图后、效果成功但回复前各一故障点，每 attempt 仅1次故障/≤12次查询。
- 观测：丢 mkdir/seed/create/attach 回复、detach 成功未 ACK、目录/镜像 inode 替换、设备号复用与检查后 retarget。
- 判定：依[五条破坏性规则](#destroy)；M5 注入先验拒绝，真机仅已登记合成资源。

### C6 busy detach

- 负载：文件/目录 FD 各一例，guest 持1个合成 FD 至 stop；VM 整树退出后，仅1个受限宿主探针进程持 busy FD，normal/force 各最多一次。
- 观测：guest FD 影响、detach max/退出码、独立缺席。
- 判定：探针关 FD/kill-reap，超时不 unlink；生产不保留 busy FD。

### C7 提前 detach

- 负载：clone 活着时仅卸本专用卷，guest 本地各8次有界读写。
- 观测：错误/缓存/Tart存活，宿主核对底层 M₀ 身份/内容无落写。
- 判定：仅机械现象，异常阻断；自主停树依共用证据边界。

### C8 主体映射

- 负载：两个已有主体各≤12次 create/chmod/chown-attempt/rename。
- 观测：两侧 UID/GID/ACL/nlink/dev/inode 与拒绝。
- 判定：依§1写者边界/共用权限规则。

### C9 容量

- 负载：普通写≤80×1MiB、稀疏/truncate≤16次且声明≤1GiB、小文件≤8192×1KiB、churn≤4096次，各独立例；清理 held-FD 复用 C6 单 FD。
- 观测：每批/结束测 I 的 st_size/blocks×512、create scratch；仅预声明 scratch/备份/索引存储，另列正常有界账本/回执。
- 判定：满足§2容量界限；填满未到 ENOSPC 为 unknown，其它报实际逻辑/分配变化。抽样峰值不证明普遍界限/全宿主无增长；满盘 liveness 后移 M7/M9。

### C10 索引/备份

- 负载：索引/FSEvents/备份各30s子例、≤16个1KiB合成 canary。
- 观测：从 create 内部挂载至导出后读回 marker/排除；eval 可读服务日志、文件访问/索引查询、备份覆盖读回，先证明接口覆盖该卷。
- 判定：看不到系统服务、阶段缺覆盖、无实际备份作业/仅空闲均 unknown，不能证明无读取/备份；需提权或全局变更则配方阻断。

<a id="admission"></a>

### S11-0c 拆分与首轮实测事实（2026-11-11）

**拆分**：S11-0c 分为 **M2a**（邮箱目录机制 C1/C2/C8，导出为宿主普通 0700 私有目录，不是限容 HFSX 卷）与 **M2c**（卷实验 C4/C5/C6/C7/C9/C10，含 hdiutil 流程与本节原有卷相关约束；其中 M2c-a 即 C4 与 C1/C2/C9 的卷上复测，已实测，见下节，M2c-b 即 C5/C6/C7/C10，已实测，见 M2c-b 一节）。M2a 的私有根规则：叶子目录为当前 UID 所有且 0700，祖先由 root 或当前 UID 所有、无组/其他写权限、无 ACL 写授权（逐段 FD 遍历，类似 ssh StrictModes），替代“每段 0700”的字面要求（Mini 的家目录与临时目录祖先不是 0700）。

**实测**（Mac mini，Tart 2.41.1，一次性 DEFAULT-Softnet clone + 受限 SSH 实验室 driver，无模型、无凭据；每例结束确认 clone 删除且缺席、无 tart/softnet 残留进程；证据标签 `DEFAULT-network/mechanics-only`，导出为普通私有目录）：

| 实验 | 结果（只记事实） |
| --- | --- |
| C1 双向 8 帧发布/读取（首轮） | 两方向完整性 ok，撕裂/混合帧 0，收到对端摘要；首轮工具未实现延迟测量，判 `unknown/partial-coverage` |
| C1 乒乓往返（补测，同一时钟、预建固定槽位） | **两方向各 8 轮全部完成，完整性 ok、撕裂 0**；宿主→访客往返 min 983 ms / 中位 997 ms / max 1023 ms，访客→宿主 min 977 ms / 中位 992 ms / max 1021 ms；单向可见延迟只给上界（≤往返）；低于 2 s 门槛；访客侧读到瞬时 ENOENT 并重读 346 次 |
| C2 file-sync 边界（写者在文件 fsync 后、rename 前退出） | 计划 3 次、尝试 3 次、rename 2 次、可见 2 次；未 rename 的 `part` 文件内容完整；判 `not-ack`（未完成的发布不当 ACK） |
| C2 dir-sync 边界 | 3 次均 rename 且可见，目录同步 1 次；`not-ack` |
| C2 rename 边界 | 3 次均 rename 且可见；`not-ack` |
| C8 主体映射 | `unknown: no-non-admin-principal`（n=0）。直接核对：guest 唯一普通账号 `eval`（uid 501）属于 `admin` 组；首轮的 `principal-unknown` 是工具缺陷（`nobody` 的 UID 为 -2，无符号解析失败），已修。未做任何创建/提权，双主体映射仍未测 |

首次真机运行暴露一个工具缺陷：macOS arm64 上以内联汇编直接发 `openat` 系统调用、未声明内核会改写的寄存器，release 构建在真机段错误；已改为调用 libc 的可变参数 `openat`。debug 单测未能发现它，这一点也记为测试教训。

**virtiofs 可见性事实（Mac mini，访客每 20–100 ms 轮询，宿主侧为本地文件系统）**：
1. 访客等待一个**尚不存在**的名字：宿主创建后，可见延迟不确定（测到 0 s、约 50 s、>100 s 未见）；访客列目录也不能刷新。从未被访客查找过的新名字立即可见。
2. 宿主把新内容 rename 覆盖到**已存在**的名字：访客新内容约 1 s 级可见；覆盖期间访客打开常返回 ENOENT（30 次覆盖中 303 次 ENOENT、186 次成功），并会错过中间版本（只见到 31 个版本中的 11 个）。
3. 访客 rename 覆盖已存在名字：宿主约 0.16 s 内可见。
4. 乒乓往返稳定在约 1 s（见上表），与 2 相符。

**对 §3 协议的约束（由上述事实推出，M3/M4 实现必须遵守）**：(a) 访客将会读取的每一个邮箱名字都由宿主在 VM 启动前预建，绝不让访客等待新名字出现；(b) 发布一律是 staging 后 rename 覆盖预建槽位，消息内容带方向与单调 seq，读方只接受期望 seq，旧 seq/占位继续等；(c) 读预建槽位时 ENOENT 与“读取中被替换”都是瞬时状态，丢弃本次读到的字节并在本轮截止内重读，只有读到的字节本身畸形/撕裂/seq 不符才是粘性失败；(d) 约 1 s 的可见延迟要计入租约预算（续租 ≤1 s、访客 ≤3 s 停树），M4/M7 须在负载下实测余量；(e) 重读次数要计入证据，不能隐藏。

### M2c-a 限容卷实测事实（2026-11-12）

同一台 Mac mini，`s11-0-probe.sh --stage V`：每个子例新建一个 64 MiB HFSX 稀疏镜像，挂在本次尝试目录里，用完按 §2 身份规则卸载并删除；C1/C2/C9 的邮箱放在卷上（槽位启动前预建），每例一台新 clone。每例结束都确认：无 clone、无挂载、`hdiutil info` 中无本次镜像。

| 实验 | 结果（只记事实） |
| --- | --- |
| C4 卷配方 | `ok/volume-ok`。回读：`hfs`、Case-sensitive HFS+、容量 67108864；nosuid/noexec/nodev/nobrowse 均生效，`owners` 生效（无 noowners）；`mdutil -s` 为 Indexing disabled；`tmutil isexcluded` 为 included（未排除备份，归 C10）。create 约 1 s、attach <1 s、detach 0 s；空镜像 7,344,128 字节 |
| C1 乒乓（卷上） | 两方向各 8 轮，完整性 ok、撕裂 0；宿主→访客往返 min 567 / 中位 993 / max 1013 ms，访客→宿主 986 / 1001 / 1011 ms；宿主侧重读 0。与 M2a 私有目录一致，约 1 s |
| C2（卷上） | file-sync：rename 2/3、可见 2，`part` 完整；dir-sync、rename：3/3 可见；均 `not-ack`。与 M2a 一致 |
| C9 data（≤80×1 MiB） | 两次运行完全相同：写到 62 个文件（65,011,712 字节）时访客收到 ENOSPC，卷剩 454,656 字节；镜像 st_size 与 blocks×512 峰值均为 76,550,144 字节（73.0 MiB，≤80 MiB 上限），较空镜像增 69,206,016 字节。宿主可用空间下降两次分别为 79,949,824 与 58,961,920 字节，第一次超出“镜像增量 + 1 MiB”判为 `failed/host-grew`，第二次 `ok` |
| C9 small（≤8192×1 KiB） | 两次均写满 8192 个文件（8 MiB）仍未 ENOSPC，按设计 `unknown/enospc-absent`；镜像峰值 47,190,016 字节（每个 1 KiB 文件约占 4.9 KiB 镜像），宿主下降 39.6/39.9 MB 与镜像增量 39.8 MB 相符 |
| detach | 本轮 9 次卸载均 0–1 s，未用 force。此前一次 C4 的按挂载点 normal detach 超过当时的 5 s 上限，被 kill 后镜像处于“已卸载但仍 attached”，探针按设计 quarantine；依 receipt 核对镜像 inode 与设备映射后人工卸载删除。同机单独计时 9 次均 0.14–0.17 s（按挂载点或设备、有无 mdutil/tmutil/marker 写入都一样），未能复现 |

**推论**：(a) 限容有效：同样的写入，镜像峰值是确定的，且受 64 MiB 卷约束；访客得到 ENOSPC，宿主卷没有被写满。(b) 宿主可用空间下降只能做旁证：在共享系统盘上，一分钟内的其它写入/释放会带来约 ±10 MB 的噪声，同一写入两次判定不同；上限证据以镜像 st_size/blocks 峰值为准，M5/M9 不得只凭宿主空间差值判卷外增长。(c) normal detach 偶尔会超过 5 s，而超时 kill 会留下已卸载但仍 attached 的镜像；探针已把 normal detach 的上限放到整个卷清理份额 20 s（force 10 s），并记录实际耗时。§2 预算表的 5 s 目标要在 M5 按实测分布重定，不能在 5 s 时直接 kill。(d) 工具教训：macOS 的 bash 3.2 下进程替换的管道要等读端回收才关闭，探针每 20 ms 轮询一次，会把打开的 fd 推到约 100 个（`ulimit -n` 为 256）；已改为不用进程替换。Linux 上的桩套件发现不了这个问题。

### M2c-b 卷实验实测事实（2026-11-12）

同一台 Mac mini（系统语言 zh_CN），`s11-0-probe.sh --stage V --case C5|C6|C7|C10` 与 `--stage R`。每例结束都确认：无 clone、无挂载、`hdiutil info` 中无本次镜像。所有报告带 `"exclusive_window":"assumed"`：按路径删除依赖所有者保留的排他窗口（§2 规则 1），不是生产实现。

| 实验 | 结果（只记事实） |
| --- | --- |
| C10 索引/备份（不起 VM） | 卷根 16 个 1 KiB canary，等 30 s：`mdfind -onlyin` 命中 0，`mdutil -s` 为 Indexing disabled；`.fseventsd` 挂载即出现，uid 0、0700，本用户不能写 `no_log`；`tmutil destinationinfo` 为 `tmutil: No destinations configured.`（0 个目的地，因而没有可观察的备份作业）；`isexcluded` 镜像 I 为 Included、挂载点 M 为 Excluded。判定 `unknown/observed`：看不到系统服务读取，不能证明无读取/备份。写完 canary 后 normal detach 耗时 11 s（未 force） |
| C5 崩溃/恢复（不起 VM） | 5 个故障点各崩溃一次（`kill -KILL`，无退出陷阱），再用 `--stage R` 恢复。F1（意图已记、未 create）、F3（已 attach、receipt 未记 mounted）、F4（已 detach、receipt 未记）、F5（已删镜像、receipt 未记）均 `ok/recovered`，F3 由新鲜映射核验后 normal detach 成功、未 force；F2（create 已成、镜像 inode 未记）按设计 `quarantine/image-unmatched`，镜像保留，R 未做任何卸载/删除；总控按 receipt 核对 attempt/挂载目录 inode、镜像未 attach、挂载目录为空后手工删除。崩溃运行各留下一个 `$TMPDIR/s11-0.*` 工作目录（无卷内容），已删 |
| C6 客机持 FD 至停机 | file、dir 两子例：客机握住预建槽位或 mailbox 目录直到 VM 停止；VM 停止删除后宿主 normal detach 0 s、退出码 0、未 force。客机 FD 不会在停机后继续占住卷 |
| C7 VM 运行时提前 detach | normal detach 立即被拒：退出码 16（“资源忙”）、0 s；按设计不 force。随后客机 8 次有界读写全部成功，Tart 一直运行。卷未卸下，所以 M₀ 不可观察（`m0_entries=unknown`）。VM 停止后 normal detach 成功、删除完成 |
| 宿主进程占用卷（手工，临时 64 MiB 卷） | 文件被占用：normal detach 退出码 16、hdiutil 自行重试约 10.7 s 才失败；目录（cwd）被占用：0.15 s 失败；`detach -force` 0.14 s 成功，占用进程不会被杀、持有失效 FD。报错文案随系统语言本地化，分类只能用退出码 |

**推论**：(a) 运行中的 VM 共享卷时，normal detach 会被系统拒绝，所以提前拆卷只能靠 force，而 force 仍只允许在证明停机之后。(b) 被占用时 hdiutil 会自行重试约 10 s，写入后服务短暂占用也会触发（C10 的 11 s）；§2 预算表“normal detach 5s”不可行，M5 至少按 20 s 份额重定，并且不能在重试中途 kill（会留下已卸载但仍 attached 的镜像，见 M2c-a）。(c) 恢复只对 receipt 能正向对应的状态生效；“create 已成、身份未记”这类状态只能隔离，生产实现应在 create 前把预期路径与排他窗口写入账本，并在 create 返回后立即记下身份。(d) `.fseventsd` 由系统在挂载时创建且本用户不可写，`-srcfolder` 预置 marker 能否生效仍待验证；C10 只能给观察，不能给保证。

### 预置 marker 与内部挂载实测（2026-11-13）

同一台 Mac mini，一次性 64 MiB 临时镜像，不起 VM，用完即删。

| 实验 | 结果（只记事实） |
| --- | --- |
| §2 候选 argv `create … -type SPARSE … -srcfolder S I` | 被拒：`-type cannot be specified with -srcfolder or -srcdevice`。改用 `-format UDSP`（稀疏）可用，镜像大小与普通 create 相同（7,344,128 字节） |
| `-srcfolder` 的内部挂载 | create 期间卷被挂到 **`/Volumes/GCU-MAILBOX`**（hfs、nodev、nosuid、nobrowse，**无 noexec**），由当前用户挂载，系统全局可见、路径由卷名决定 |
| 普通 create（现用配方，无 `-srcfolder`） | 3 次均未出现任何挂载；只有短暂的设备 attach 用于格式化 |
| 预置 `.fseventsd/no_log` | 卷上 `.fseventsd` 保持为当前用户所有、0700；受控挂载后写 4 个文件、等 20 s，目录里仍只有 `no_log`（未生成事件日志）。不预置时系统在首次挂载时以 root、0700 创建 `.fseventsd`，本用户无法再放 `no_log` |
| 预置 `.metadata_never_index` | `mdutil -s` 为 Indexing and searching disabled（不预置、挂载后再写时为 Indexing disabled）；`mdfind -onlyin` 命中 0 |
| 种子树 | 目录/文件的属主与权限按 `S` 原样保留（`owners` 开启） |

**推论与建议**：`-srcfolder` 能让两个 marker 在首次受控挂载前就生效，但代价是一次全局可见、无 noexec 的内部挂载；依 §2“必须限制所有内部挂载而此配方做不到就拒绝配方”，**建议保留普通 create**：不出现内部挂载；受控 attach 后写 `.metadata_never_index`（实测不被索引）；系统创建的 `.fseventsd` 日志只在一次性卷上、随镜像删除，记录的只是固定槽位名。最终由 M6 定。

### M4a 调度可行性（2026-11-13）

M4 第 2 轮审阅后拆为 M4a（邮箱侧）与 M4b（machine 集成，见 §6）。M4a 的可行性门按实际协议周期计时：inventory 请求 → 访客采样 → 回复对宿主可见 → 宿主下一步发布 grant → grant 对访客可见；两个方向分开计，只用实测上界。

- 已有实测：C1 往返 max 1023 ms；单向只知不超过往返；一步服务 100 ms；发布间隔 800 ms；访客截止固定为 `g+2000`。
- 样本在 grant 对访客可见时的年龄上界 1023+100+1023 = 2146 ms；相邻 grant 可见间隔上界 800+100+1023 = 1923 ms；连续性要求“年龄 + 间隔 < 2000 ms”，不满足。即使把整段交换只算一次往返：1023+100+800+100 = 2023 ms，仍超过 2000 ms。
- 结论：**在现有常量（`g+2s`、访客 ≤3 s 停树）与实测 virtiofs 可见延迟下，没有可行的 fresh-sample/grant 调度**；可行性门对任何窗口都返回 `Infeasible`，按 §3“无可行调度即 gate 失败，不增租约/线程/observer”，真实通道不得准入。代码与测试见 S11-M4a。
- 待 owner 决定的方向（未决定，均需另行审阅）：(1) 修订租约常量（例如更长的访客租约与停树时限），这会改变 §3 的安全时限；(2) 精测单向延迟分布（实测“访客 rename 宿主约 0.16 s 可见”“宿主覆盖访客约 1 s 可见”提示两向不对称，可能来自 virtiofs 属性缓存），看能否在不改常量下证明可行；(3) 换一种宿主→访客的快速通知方式（须仍符合 2026-11-10 的“无 IP 控制通道”决定）。

**仍未测**：C8 真正的双主体映射（准备副本 `macos-s2-eval-c8` 已克隆，等 owner 建标准账号，步骤见 runbook）、block-all 下的自主运行与存活（M7/M9）。

## 6. 残余风险、not_run 与分步交付

TCB 含宿主内核/文件系统/virtiofs/Tart/Softnet、provider、supervisor/helper；恶意 IO 可拒绝服务；OS 不执行终止/卸载只能隔离。镜像逻辑删除，不保证 SSD 擦除；备份/索引/容量/身份证据不足就不运行。

无 IP 控制不证明 egress：ADR 0018 §5 的 LAN/metadata/DNS/IPv6/UDP/QUIC、endpoint 服务边界与计数仍须 S11c，IPv4 `/32` 不替代证明。
endpoint 泄漏、短期凭据/无 refresh、进程/窗口残余及 S3/S4/S12/TCC 沿用 ADR 0017/0018。全部新卷/权限/协议/时序/恢复/真实接线、安全隔离/Stop/登录与模型评测仍 **not_run**；仅旧双向可见已测。
不安装 provider/铸造准入，不改 S6/[矩阵§8](../real-eval-acceptance-matrix.md#8-开启真实运行的门槛)；admission 默认关闭，无 IP/串口/块盘回退、自动恢复或通用文件交换/执行；仅[固定探针例外](#lab-driver)，不替代 M7/M8 准备/封存。

每卡一个 session、独立审阅≤三轮；义务单卡归属，遵守§2生命周期边界。

| 卡 | 交付 | 进入条件 | 必要 owner 动作 |
| --- | --- | --- | --- |
| S11-M1 | 本修订独立复审 | 未通过不操作宿主 | §1决定/§5 notice |
| S11-M2 | [S11-0c](#lab)机械结果/无提权清单；失败缩减设计 | M1；不依赖 S11e/M7 | 复制探针无需 owner 安装；已有双主体可测，否则按§5 unknown/后移，不登录模型 |
| S11-M3 | [codec/open/publication](#宿主发布与读取) fake/离线竞态、固定槽/源名替换、有界状态帧/过时 ACK | [七操作](#operations)；独立审阅 | 无 |
| S11-M4a | 邮箱侧：`MailboxGuestChannel`（七方法、pending、不重发、仅经 inventory 完成、消费记录核对）、grant 记账与样本固定、按实测上界的调度可行性门、单次读取 128 KiB | M3；独立审阅 | 无 |
| S11-M4b | machine 集成：每个公开 step 共享 100 ms 截止与可续跑分片、kill/reap 句柄与冻结截止、Stop/owner 证明服务（reserve→seal）、unknown effect 粘性隔离、回收游标、保存的 revoke end；先短设计再代码 | M4a、调度可行性有解（见“M4a 调度可行性”）、设计与代码独立审阅后才真实接线 | 视调度结论而定 |
| S11-M5 | 卷账本/持久目录身份/销毁屏障 fake、C5/recovery | [五条规则](#destroy)；禁 VM-only 成功先 seal | 无 |
| S11-M6 | 宿主卷/Tart 私有适配（依§2生命周期） | 仅 M2 已测固定 argv，接 M5；[准入边界](#admission) | 只在已审阅宿主范围内 |
| S11-M7 | S11e supervisor/golden/sandbox | [§4](#supervisor)与下列自主终止义务 | 必要准备安装/封存签认 |
| S11-M8 | S11e admission、S3/S4/S12 真实 FD/native ACK/TCC 接线/否定证据 | M7/§4 | 固定 helper TCC 点击/变更签认 |
| S11-M9 | 联合 S11c/d/f，block-all transport/liveness/负载时限、隔离/Stop/整树/卷清理验收；S11g 单排 | M7/M8；SSH 结果不计入 | 审阅回执/必要用户恢复；矩阵§8全满足才另排登录/模型 |

M4 代码相对 `crates/gcu-eval/src/disposable/tart/machine/`：

- `io.rs:117` 增 Task aggregate；`io.rs:245`/`cleanup.rs:81` 校验 lease/drain，拒任意成功 body。
- `steps.rs:289` 分片 probe；`:333/:361/:437` 不以即时响应作 ACK/login/score，不仅 Running 才 arm lease。

离线 trace：慢查询/连续 probe/4s stop/清理时 owner 证明、片内到期/kill-reap/不复活、long-login/max-task 请求计数/年龄/间隙；不得只改 login/run。
M5 在 `cleanup.rs:202`、`recover.rs:108` 纳入完整卷清理屏障（含资源表 M₀/V）。
M7/M9 在 block-all 覆盖自主 lease/迟到/父死/满盘/提前 detach、脱组后代终止、模型邮箱不可达、Stop 保留与负载下 grant-age/gap/终止 gate。
