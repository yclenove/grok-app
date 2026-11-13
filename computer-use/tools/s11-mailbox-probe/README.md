# S11-0c 邮箱测量核心

宿主编排在 `scripts/s11-0-probe.sh`（阶段 M 是普通目录，阶段 V 是限容卷）。这个二进制只测量一个目录。它不 clone，不做 SSH，也不调用 tart、hdiutil。做 C1、C2、C8、C11，C9 的 `fill`，以及 C6 的 `hold`、C7 的 `c7ops`。只用标准库。测试另计。

阶段 M 的导出目录是宿主上的普通私有目录。阶段 V 把同一套槽位放在限容 HFSX 卷的 `mailbox` 上。卷的创建和删除只在 bash 里做。

## 子命令

目录必须是绝对路径。每一级都用目录 fd 加 `O_NOFOLLOW` 打开。`.` 和 `..` 拒绝。

```text
s11-mailbox-probe writer --dir <绝对目录> --role host|guest --count N --timeout SEC
s11-mailbox-probe reader --dir <绝对目录> --role host|guest --count N --timeout SEC
s11-mailbox-probe c2 file-sync|dir-sync|rename --dir <绝对目录> --role host|guest --count N --timeout SEC
s11-mailbox-probe c8 --dir <绝对目录> --role host|guest --count N --timeout SEC
s11-mailbox-probe fill --dir <绝对目录> --role guest --mode data|small --timeout SEC
s11-mailbox-probe hold --dir <绝对目录> --role guest --kind file|dir --secs SEC
s11-mailbox-probe c7ops --dir <绝对目录> --role guest --ms MILLISEC
s11-mailbox-probe c11 --dir <绝对目录> --role host|guest --mode fd|reopen --count N --timeout SEC
```

`hold` 和 `c7ops` 只在客机跑，各打一行到标准输出，不写结果文件。`hold` 的 `file` 握住预建的 `c6-held`，`dir` 握住邮箱目录；`--secs` 是 0 到 300，打出 `held=1` 之后才睡。`c7ops` 对预建槽位做 4 次读和 4 次写（staging 后 rename），每次单独线程，主线程按 `--ms`（1 到 5000）超时；挂起后不再做后面的操作。宿主编排不拿这个二进制做卸载。

`fill` 只在客机跑。`data` 最多 80 个 1MiB 文件，`small` 最多 8192 个 1KiB 文件，遇到 ENOSPC（或测试用的更小配额）就停，并在 `result-guest` 报告已写字节和文件数。邮箱里要先有 `payload/` 和 `progress`。

`N` 是 1 到 8。`SEC` 是 0 到 120。标志顺序不限。每个子命令有自己的进程内绝对截止（`Instant`）。到点写 `unknown` / `deadline` 后退出。没测到的时间写成 `-`，不写成 0。

结果是固定格式的一行，不超过 4096 字节，写到 `result-host` 或 `result-guest`。读取同一目录 fd，并对这个 fd 做 fstat：普通文件、`nlink == 1`、大小不超过上限，读前后的 dev/inode 一致。符号链接、硬链接、FIFO 和超大文件都判失败。撕裂帧和混合帧是 `failed`。没有对端摘要不能报 `ok`。

C1 是乒乓：先 host→guest，再 guest→host，各 `N` 轮。每个方向只用固定槽位 `hg-ping`、`hg-pong`、`gh-ping`、`gh-pong`、`hg-ready`、`gh-ready`，外加发布用的同名 `.tmp`。bash 在启动 VM 之前把这些槽位写进 `host-write` 和 `guest-write`，并在邮箱根放 `summary-host` 与 `summary-guest`，内容都是 `v=1 op=empty seq=0`。客机只读已经存在的名字，不再轮询一个还不存在的名字。每轮发布是 staging 写入后 `renameat` 覆盖该槽位。ping 仍是 4096 字节帧，帧内带方向和 seq。pong 是 `v=1 op=pong dir=hg|gh seq=<n> digest=<sha256>`。就绪是 `v=1 op=ready dir=hg|gh`。读方看见占位或更小的 seq 就继续等；更大的 seq、方向不符或畸形是 `failed`；占位不是 pong。响应方把 `*-ready` 从占位覆盖成就绪内容，发起方等到内容变成就绪，上限是本命令剩余的绝对截止，不另起计时；一直是占位则 `unknown/peer-not-ready`，内容不符是 `failed`。读槽位时 ENOENT 和 inode 被替换都只是再读一次，该次字节作废；只有字节本身撕裂、畸形、方向或 seq 不符才是粘性 `failed`。结果字段 `retry=absent:N,replaced:M` 是该进程读槽位时遇到的这两类次数。然后发起方才发布 ping，对端读到完整匹配的 ping 后发布 pong。每一轮仍最多等 3 秒。往返从该次 ping 发布完成起记在发起方自己的单调时钟上，不比较两端时钟，也不计入对端启动。`rtt_us` 是每个方向的 `n/min/p50/max`（样本少于 20 不报 p99）。`vis_us` 写成 `≤rtt`。两方向都完成且完整性 ok 才是 `latency=measured`。某一轮超时是 `unknown`，并保留已测样本。预建槽位上的瞬时 ENOENT 在本轮或命令截止内重读；截止仍空着时，就绪是 `peer-not-ready`，回合是 `timeout`，其余是 `deadline`。撕裂或畸形字节是 `failed`。最大往返 ≥2s 时记录 `gate=exceeds-2s`，判定本身不因此改变。

C11 不改名字、不 `rename`。邮箱根上要先有两个 64 字节文件 `ip-hg` 和 `ip-gh`。bash 在 VM 启动前，用有界执行器在私有导出目录里以 `O_EXCL|O_NOFOLLOW` 直接创建这两个全 0 文件，不经过临时文件再移动或复制。宿主写 `ip-hg`、读 `ip-gh`；访客反过来。记录固定 64 字节：`v=1 seq=<十进制> crc=<8 位小写十六进制>`，左对齐，空格补齐，末字节是换行。crc 是 seq 十进制 ASCII（无前导 0）的 SHA-256 前 8 个十六进制字符，例如 seq 1 的 crc 是 `6b86b273`。`seq=0` 的规范记录只用作访客就绪，写在预建的 `ip-gh` 上。crc 不符或 seq 解析不出是撕裂，计入 `torn` 后重读，不算失败；seq 回退或跳号是 `failed`（`rollback` / `skip`）。`--mode fd` 读方打开一次并一直持有 fd，每 5 毫秒 `pread` 64 字节；`--mode reopen` 每次轮询都打开、读取、关闭。写方每次 `pwrite` 64 字节后 `fsync`。第 1 轮开始前，访客先把 `ip-gh` 写成 seq=0，宿主读到这条就绪记录后才发布 seq=1 并开始该轮计时；这段等待不计入 8 个往返样本，一直等不到则是 `unknown` / `peer-not-ready`。往返从这次 `pwrite` 之前算起，到读到同号 pong 为止，包含写入、`fsync`、访客反应和读回，记在写出 ping 的宿主自己的单调时钟上，不和访客对时。`fsync` 返回之后到读到 pong 的残差另记为 `resid_us`，不代替 `rtt_us`。单轮上限 5 秒，截止在该轮开始时定下，读到合法记录之后、以及访客写完 pong 并 `fsync` 之后都按原来的截止复查，过点不算这一轮成功，也不把截止往后推。整条命令另有 `--timeout`。宿主结果行是 `v=1 op=c11 role=host mode=<fd|reopen> n=… torn=… rtt_us=… resid_us=… verdict=… reason=…`。访客另报 `polls`（每轮从开始读到看见新 seq 的次数，含撕裂）和 `gap_us`（上次 pong 的 `fsync` 返回到看见下一个 ping）。宿主编排只接受访客的一行摘要：`v=1 op=c11 role=guest`、模式与本子例一致、轮数和 polls/gap 互相符合、裁决只是 `ok|unknown|failed`。访客的数字不替换宿主测得的 `n`、`torn`、`rtt_us`、`resid_us`。摘要缺失或畸形则该子例是 `unknown`；访客写明 `rollback` 或 `skip` 则仍是 `failed`。

C2 的三个边界都是“写者在该点退出”。file-sync 完成的重命名是 `N-1`，第三次只 fsync 文件 `part`，不重命名，也不 fsync 数据目录；进度写在旁边的 `evidence` 目录。C2 的结论是 `not-ack` 或失败，不会报 `ok`。内容不对的 `part` 是 `failed` / `bad-part`。C8 只数非管理员主体。0 个是 `unknown` / `no-non-admin-principal`（`n=0`）。1 个是 `unknown` / `single-principal`。两个及以上是 `unknown` / `mapping-not-measured`。数不清是 `unknown` / `principal-unknown`。不建账户，不提权。

退出码：`ok` 和 `not-ack` 是 0，其它测量结果是 1，用法错误是 2。

`cargo test` 使用真实临时目录覆盖帧格式、撕裂、符号链接、FIFO、硬链接、超大文件和截止。测试不调用 tart 或 ssh。
