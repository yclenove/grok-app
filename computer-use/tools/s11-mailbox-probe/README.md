# S11-0c 邮箱测量核心

宿主编排在 `scripts/s11-0-probe.sh` 的阶段 M。这个二进制只测量一个目录。它不 clone，不做 SSH，也不调用 tart。只做 C1、C2、C8。只用标准库。测试另计。

导出目录是宿主上的普通私有目录，不是限容卷。HFSX 上的差异由后续 M2c/M7 补测。

## 子命令

目录必须是绝对路径。每一级都用目录 fd 加 `O_NOFOLLOW` 打开。`.` 和 `..` 拒绝。

```text
s11-mailbox-probe writer --dir <绝对目录> --role host|guest --count N --timeout SEC
s11-mailbox-probe reader --dir <绝对目录> --role host|guest --count N --timeout SEC
s11-mailbox-probe c2 file-sync|dir-sync|rename --dir <绝对目录> --role host|guest --count N --timeout SEC
s11-mailbox-probe c8 --dir <绝对目录> --role host|guest --count N --timeout SEC
```

`N` 是 1 到 8。`SEC` 是 0 到 120。标志顺序不限。每个子命令有自己的进程内绝对截止（`Instant`）。到点写 `unknown` / `deadline` 后退出。没测到的时间写成 `-`，不写成 0。

结果是固定格式的一行，不超过 4096 字节，写到 `result-host` 或 `result-guest`。读取同一目录 fd，并对这个 fd 做 fstat：普通文件、`nlink == 1`、大小不超过上限，读前后的 dev/inode 一致。符号链接、硬链接、FIFO 和超大文件都判失败。撕裂帧和混合帧是 `failed`。没有对端摘要不能报 `ok`。

C1 是乒乓：先 host→guest，再 guest→host，各 `N` 轮。响应方打开目录后，在自己的写目录用 staging 加 renameat 发布固定就绪标记 `hg-ready` 或 `gh-ready`（内容 `v=1 op=ready`，不超过 64 字节）。发起方先用安全读等到该标记，上限是本命令剩余的绝对截止，不另起计时；没等到是 `unknown/peer-not-ready`，内容不符或读取时被替换是 `failed`。然后发起方才发布 `ping`，对端读到完整 ping 后发布带 seq 和 ping 摘要的 `pong`。每一轮仍最多等 3 秒。往返从该次 ping 发布完成起记在发起方自己的单调时钟上，不比较两端时钟，也不计入对端启动。`rtt_us` 是每个方向的 `n/min/p50/max`（样本少于 20 不报 p99）。`vis_us` 写成 `≤rtt`。两方向都完成且完整性 ok 才是 `latency=measured`。某一轮超时或缺失是 `unknown`，并保留已测样本。撕裂或替换是 `failed`。最大往返 ≥2s 时记录 `gate=exceeds-2s`，判定本身不因此改变。C2 的三个边界都是“写者在该点退出”。file-sync 完成的重命名是 `N-1`，第三次只 fsync 文件 `part`，不重命名，也不 fsync 数据目录；进度写在旁边的 `evidence` 目录。C2 的结论是 `not-ack` 或失败，不会报 `ok`。内容不对的 `part` 是 `failed` / `bad-part`。C8 只数非管理员主体。0 个是 `unknown` / `no-non-admin-principal`（`n=0`）。1 个是 `unknown` / `single-principal`。两个及以上是 `unknown` / `mapping-not-measured`。数不清是 `unknown` / `principal-unknown`。不建账户，不提权。

退出码：`ok` 和 `not-ack` 是 0，其它测量结果是 1，用法错误是 2。

`cargo test` 使用真实临时目录覆盖帧格式、撕裂、符号链接、FIFO、硬链接、超大文件和截止。测试不调用 tart 或 ssh。
