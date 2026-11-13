# S11-0 无模型探针手册

状态：**脚本与本手册已写，真实 VM 未运行（not_run）**。这与 ADR 0018 §10 的记录一致。Linux 上的桩测试只检查参数、清理和报告形状，不能当作 Softnet 或 Tart 的测量。本手册不构成运行授权。

## 给 owner 的一句话（首次运行前必须先发）

将在 Mac mini 上对固定 golden `macos-s2-eval` 做一次无模型探针：阶段 A、阶段 B 各 `tart clone` 一台新的 `s11-0-probe-<阶段>-<pid>-<随机>`。只有 `tart clone` 退出码为 0，并且随后一次可信的 `tart list` 里出现该完整名字，才算本次创建，也才可以停止并删除它。探测名义预算 1500 秒；清理另有一份 90 秒的累计清理额度。中途清理计入探测计时。不承诺精确的总硬上限。运行期间 Mini 上不要做 GUI 测试，不改防火墙、路由或 DNS，不会启动、修改或删除 golden，也不得按前缀删除其它 VM。

## 运行前要确认

- golden `macos-s2-eval` 已存在且是 `stopped`。脚本不会停止它。
- golden 里的用户 `eval` 已经可以用现有公钥登录。默认密钥是 `~/.ssh/id_ed25519_tart`，可用 `--ssh-key` 换成另一把已授权的密钥。脚本不改用户、不收集主机名。
- `softnet` 已由 owner 安装。脚本不执行 softnet，也不接收 `--softnet` 路径。它把 PATH 里的 `softnet` 符号链接解析到最终文件，用 `stat -f '%Su %Sp'` 检查属主是 root 且模式含 SUID。缺了就写原因并退出 2，不改任何东西。
- 带 `--net-softnet` 的 `tart run` 会由 Tart 去调用 Softnet。脚本自己不调用它。
- 阶段 B 或 `all` 必须显式给出 `--endpoint-ip <获批公网 IPv4>`，或显式给出 `--endpoint-placeholder`（固定文档地址 `203.0.113.10/32`）。只有后者时报告写 `endpoint_source=placeholder`，这不是获批端点的证据。两个都不给，脚本拒绝跑阶段 B。

## 在 Mac mini 上怎么跑

非交互 SSH 的 PATH 里可能没有 Tart。在 Mini 上：

```bash
export PATH="$HOME/bin:$PATH"
# 示例，替换为获批地址。--tart-home 的参数换成本机目录。
bash computer-use/scripts/s11-0-probe.sh --dry-run --stage all \
  --endpoint-ip 203.0.113.10 --tart-home '<路径>' --out ./s11-0-dry-out
bash computer-use/scripts/s11-0-probe.sh --stage all \
  --endpoint-ip 203.0.113.10 --tart-home '<路径>' --out ./s11-0-out
```

没有获批地址、只做文档地址对照时：

```bash
# 示例。报告会写 endpoint_source=placeholder，不能当作端点证据。
bash computer-use/scripts/s11-0-probe.sh --stage B --endpoint-placeholder --tart-home '<路径>'
```

常用参数：`--stage A|B|C|M|V|R|all`（默认 all；`all` 不含 M、V、R），`--out`（必须是全新不存在的目录；默认 `./s11-0-out`。脚本拒绝已存在的目录或符号链接，避免把陈旧报告当成这次的证据），`--lan-ip <IPv4>`（不给则 LAN 记为未测量；不扫描），`--ssh-key`，`--tart-home <路径>`，`--dry-run`。阶段 M 另要 `--case C1|C2|C8` 和 `--pinned-host-key <文件>`；非 dry-run 还要 `--probe <测量二进制>`。阶段 V 的 `--case` 是 `C4|C1|C2|C9|C5|C6|C7|C10`。C5 还要 `--fault F1|F2|F3|F4|F5`。阶段 R 要 `--receipt <volume-receipt>`，并且拒绝 `--dry-run`。这些标志不能和 A/B/C 一起用。没有 `--golden`、`--clone-name`、`--guest-user`、`--hostname`、`--softnet`，也没有可复用的 clone 名字参数或环境变量。拒绝 root。dry-run 走同一套 argv 构造，只打印，不 clone。阶段 M 的 `--out` 要是绝对路径，路径上每一级都不是符号链接（Mini 上放在家目录；测量核心会拒绝符号链接组件）。

## 时长与清理

探测预算 1500 秒（bash `SECONDS`，不看墙钟）。每次非清理命令的限时会被夹到剩余探测时间。清理使用一份累计清理额度 90 秒，阶段之间的清理和退出时的再清理共用这一份，不按次重置。单次清理命令的限时先扣掉 1 秒，把 TERM 之后的短宽限留在额度里。实际宽限是若干次 0.05 秒轮询作业表，到点仍在跑的直接子进程会被 KILL。中途清理计入探测计时：清理进行时 `SECONDS` 继续走，后面的阶段只剩更少的探测时间，同时这笔时间也从累计清理额度里扣。宽限、写报告和最后一次回收仍可能超出这两个名义数字，因此不承诺精确的总硬上限。

版本、列表、帮助、`tart ip` 每次最多 10 秒，clone 180 秒，`tart exec` 20 秒，就绪 ssh 20 秒，采集 ssh 50 秒，等地址最多 240 秒，stop 和 delete 各 30 秒。这些都再被剩余时间夹住。有限命令的标准输出和标准错误写入普通临时文件，脚本每 20 毫秒检查这些文件的大小，达到 64KiB 就终止该命令并记为 `capture-overflow`（不用 `ulimit -f`：它会限制被调用程序写的每个文件，在真机上直接杀掉了 `tart ip`），不会把截断的 `tart list` 当成“不存在”。`tart run` 的标准输出和标准错误直接丢弃，不留原始日志。

清理期间忽略 INT、TERM、HUP，避免第二次信号打断 stop/delete。清理结束后用可信的 `tart list` 确认该名字已经不在，而不只看命令退出码。Ctrl-C 在清理开始前会停下后续阶段，并只清掉本进程已经确认创建的 clone。阶段 A 的 stop、delete 或确认失败时不进入阶段 B，也不覆盖 A 的记录。

脚本不处理 SIGKILL。被 SIGKILL 或断电杀死的运行可能没有最终报告。没有这份报告时，不能用输出目录里的旧 json 认领 clone。owner 只对照自己通知过的那一个完整名字，手工看 `tart list`；不得按前缀、通配符或旧报告删除。clone 失败、超时或结果不明确时，脚本自己也不会删除，原因是 `ambiguous-destination`，并停在后续阶段之前。

## 测什么、不测什么

阶段 A：`--net-softnet`。阶段 B：再加 `--net-softnet-block=0.0.0.0/0` 和 `--net-softnet-allow=<ip>/32`。报告里的 argv 就是传给 `tart run` 的那一个数组。阶段 M 见下一节，不进 `all`。

每次测量：gateway 的 TCP 22/53/80/443，gateway 与一个公网解析器的 DNS（guest 上只跑 `dig -r +notcp +ignore +time=2 +tries=1 @<解析器> example.com A`；`-r` 忽略 `.digrc`，`+notcp` 与 `+ignore` 禁止截断后改走 TCP），metadata `169.254.169.254` 的 TCP 80，是否有全局 IPv6 地址，以及到一个公网 IPv6 的一次 TCP。没有 `dig` 时该观察是 `tool-error`，`dns-udp-gateway` 和 `dns-udp-public` 进入 `unmeasured`，不换别的解析工具。阶段 B 另加获批或占位端点的 TCP 443。`tart ip` 没有地址、或 sshd 尚未接受固定的 key-only 就绪命令时，在启动预算内重试；采集命令只在就绪后跑一次。A 和 B 另外各给 `tart exec <clone> /usr/bin/true` 一段有界的启动机会，不依赖 ssh 是否成功，成功也不说明它不经 IP。阶段 C 只记录 `tart --help` 和 `tart exec --help` 里的子命令/选项事实，不 clone；其中任一失败则这次采集失败，退出码非零。

库存来自 `tart list --format json`：空数组 `[]` 表示没有 VM；缺 `]`、缺 Name 或 State、重复 Name、未知 State，或任何其它偏离，都是 `list-untrusted`（不是“不存在”），不用 Running 顶替 State；`inventory_columns` 只记录 `json` 和出现过的白名单键名（Name、State、Source、Disk、Size、Running、Accessed），不记录值。

`measured` / `unmeasured` 来自各阶段实际观察，不是事先填好的清单。dry-run、前置检查失败、ssh 未连上时，`measured` 为空或不含这次没采到的网络角色。宣布某阶段网络采集完成前，预期角色必须齐全且每个只出现一次。`route` 或 `ifconfig` 管道失败是 `tool-error`，不是“无网关 / 无地址”。

明确不测，并写在 `unmeasured`：UDP/443、QUIC、IPv6 的 gateway/ULA/link-local/UDP、DoT、DoH、公网 TCP/53。未提供 `--lan-ip` 时 LAN 也在 `unmeasured`。报告不写 LAN 地址，只写角色 `lan`。gateway、guest 地址、解析器地址、IPv6 字面量也不写入报告。

## 怎么读报告

只有 `report.json`（schema `s11-0-probe/v2`）。传输结果只用这些词：

| result | 含义 |
| --- | --- |
| connected | 连接成功，或 DNS 工具退出 0（含 NXDOMAIN 这类应答）。不是“策略允许” |
| refused | 有东西拒绝了连接。不是 Softnet 拒绝 |
| no-route | 观察到无路由或主机/网络不可达 |
| no-address | 没有可用地址（例如没有 gateway，或没有全局 IPv6） |
| timeout | 这次调用超时 |
| tool-error | 命令缺失、参数错误、ssh 失败或无法分类。不是 timeout |

`ipv6-global-address` 还可以是 `yes`：本机配置里看到了全局 IPv6 地址。`yes` 不是上面六个传输词，也不表示出网成功。

`h1_independent_of_ip` 固定是 `unknown`。`tart exec` 成功只说明 Tart 的这条命令当时能跑，不说明它不经 IP。

有限命令的输出留在私有临时目录里，结束时删掉。报告只出现白名单原因码。`tart run` 不留原始日志。

## 退出码

| 码 | 含义 |
| --- | --- |
| 0 | `complete`，或 dry-run 的 `not-run` |
| 2 | 用法或前置检查失败（含输出目录已存在、库存不可信、golden、softnet、`tart` 超时）。没有改 VM |
| 3 | 创建被拒绝且库存可信：clone 前该名字已在列表里，或 clone 非零且列表确认该名字不存在。不删除 |
| 4 | 采集不能跑、非零退出、超时、没有地址，或阶段 C 的帮助采集失败。此时状态不是 `complete` |
| 5 | 清理失败，或 `ambiguous-destination`（clone 超时、输出超限、clone 非零但名字已出现或列表不可信、clone 退出 0 但名字不在）。不自动删除。owner 按该完整名字人工核对 `tart list`。后续阶段停止 |

采集失败优先于清理失败。`ambiguous-destination` 优先于这两者，退出码仍是 5。

## 决策规则（ADR 0018 §10）

若这份实测显示 Softnet 单独已满足 §5 的拒绝类别，则 §5 第 3 点的宿主网络 guard 不进入范围。若不满足，先把具体 guard 方案（影响范围、回滚、是否只作用于该 Tart/Softnet 实例）提交 owner 审阅，批准前不安装任何网络规则。若 H1 没有可用的非 IP 通道，暂停 S11 并回到 owner，不为凑通道去开放 gateway 上的 IP 服务。脚本不下这些结论。

不要改宿主的 PF、防火墙、路由或 DNS。

## 阶段 M（邮箱机械，C1 / C2 / C8）

阶段 M 复用本脚本的 clone、启动、取地址、清理和报告，不起阶段 A/B 的网络采集。`tart run` 使用 `--no-graphics --net-softnet`，并加上 `--dir s11mbx:<本次导出>/mailbox`。这是 DEFAULT 网络的实验室例外，不加 block，也不加 allow。报告增加 `evidence` = `DEFAULT-network/mechanics-only`，以及 `export` = `host-private-directory (not the capped HFSX volume)`。导出目录是脚本在 `--out` 下用 `mktemp` 新建的 0700 私有目录，不是限容卷。

真机跑阶段 M 之前必须先发一句，确认这次是 DEFAULT 网络机械实验、网关会暴露。没有这句话就不要跑。本段不构成运行授权。真实 VM 上的阶段 M 仍是 not_run。

每个子例用 `/usr/bin/ssh-keygen` 在宿主私有目录生成一把一次性 ed25519。现有 eval 密钥只做两步：就绪命令 `/usr/bin/true`，以及一次固定初始化（客机新建 0700 目录、写入固定 dispatcher、客机 `shasum -a 256` 核对宿主算出的摘要、追加带 `restrict,command=` 的一次性公钥）。任何一步失败都停。之后的 upload、verify、run 只用一次性密钥。dispatcher 只按字面接受 `upload`、`verify` 加 64 位十六进制、`run C1`、`run C2`、`run C8`、`run C9`、`run C6 file`、`run C6 dir`、`run C7`。`--pinned-host-key` 是 golden 的 ssh-ed25519 公钥，clone 继承同一把主机密钥。SSH 使用 `StrictHostKeyChecking=yes`、独立 known_hosts、只认这一把私钥、关闭转发、不用 TTY。每次连接前，本次 clone 必须是唯一正在运行的 VM。

C2 对 `file-sync`、`dir-sync`、`rename` 各起一台新 clone。每个边界计划发布 3 次。file-sync 完成两次重命名；第三次只把文件 fsync 到 `part` 后退出，不重命名，也不同步被测数据目录。三个子例都是 `not-ack`，并且 VM 清理被证实时，聚合才是 `not-ack`、退出码 0。`unknown` 或没跑到的子例优先于 `not-ack`。C8 若客机只有一个非管理员主体，结果是 `unknown` / `single-principal`。不建账户，不提权。

结束时只删除脚本自己创建的已知文件名，空目录用 `rmdir`。出现未知名字或符号链接时，一个都不删，整目录留存并记 `quarantine`，退出码非零。不递归删除。白名单里的客机帧可以留下，状态记 `retained`，这不是 quarantine。报告没写成功时，脚本非零退出（测量本来可以是 0 时，退出码是 2）。

阶段 M 在 quarantine（含审计超量、超时、未证明 VM 已退出）时退出码是 5，即使采集结果同时是 unknown；测量原因仍留在该 case 行。A/B/C 仍是采集失败优先于清理失败。

测量二进制是 `tools/s11-mailbox-probe`。它只接收目录、角色和计数/超时，子命令是 `writer`、`reader`、`c2 <boundary>`、`c8`，以及只在客机上跑的 `fill --mode data|small`。另有客机子命令 `hold --kind file|dir --secs <秒>` 和 `c7ops --ms <毫秒>`：各打一行测量结果。`hold` 在打印 `held=1` 之后才睡眠。文件操作走目录 fd。它不负责 VM、SSH 或磁盘镜像，宿主也不用它卸载卷。

## 阶段 V（限容卷，C4 / C1 / C2 / C9）

阶段 V 在每次子例里新建一个 64MiB 的 HFSX 稀疏镜像，挂到本次尝试目录里的空挂载点，用完后卸载并删除。C4 只做卷的创建、挂载、读回和删除，不起 VM。C1、C2、C9 把阶段 M 的邮箱放到这个卷的 `mailbox` 上（槽位仍在启动前写好），每个子例一台新 clone、一个新卷。C2 的三个边界、C9 的 `data` 和 `small` 各是一次独立尝试。报告里的 `export` 是 `capped HFSX volume`。C4 的 `evidence` 是 `volume-only`；C1、C2、C9 仍是 `DEFAULT-network/mechanics-only`。

真机跑阶段 V 之前必须先发一句：这次会在 Mini 上创建、挂载、卸载并删除一个 64MiB 的专用镜像。没有这句话就不要跑。本段不构成运行授权。真实机器上的阶段 V 仍是 not_run。C5、C6、C7、C10 和阶段 R 见下文。

创建用 `/usr/bin/hdiutil create -size 64m -fs HFSX -type SPARSE -layout NONE -volname GCU-MAILBOX`。macOS 27 上这条命令退出码是 0，stderr 有弃用警告（`'hdiutil create ...' is deprecated. Please use 'diskutil image create ...'`）。`diskutil image create blank` 不能做 HFSX，也不能做单文件 sparse，所以仍用 hdiutil create。警告记成观测，不当错误。

挂载不用 hdiutil 的旧 attach 参数（macOS 27 报 unknown option）。改用 `/usr/sbin/diskutil image attach --plist --nobrowse --mountPoint <挂载点> --mountOptions nosuid,noexec,nodev,owners <镜像>`。不要用 `diskutil image info`（对这份 sparseimage 返回 Operation not supported）。

设备节点和挂载点只从 `/usr/bin/plutil` 读出。先 `plutil -lint` 确认整份 plist 有效，再用 `plutil -type` 和 `plutil -extract <键> raw -o -` 取得数组的权威长度（`images` 或 `system-entities` 的 raw 输出是元素个数，例如 0 或 2；type 输出 `array`）。长度设有上限，超限就 quarantine。然后对 `0..count-1` 的每个元素抽取全部必需字段；任一抽取失败、类型不对或互相冲突，都 quarantine。不存在的键会报 “Could not extract value…”，这不是空数组。只有完整校验后的清单（含显式的空数组）才能当作没有这份镜像。路径里的 `$` 等字节是数据，不能当成数组结束。

attach 的 plist 顶层是 `system-entities`。元素里的 `dev-entry` 没有 `/dev/` 前缀（例如 `disk4`），还有 `filesystem-name`、`filesystem-type`、`mount-point`、`size`、`volume-name`、`content-hint`。`filesystem-name` 为 `Case-sensitive HFS+` 时记成 `case-sensitive-hfs`。读不到文件系统名、容量或镜像大小时，C4 不能记成完整通过。`hdiutil info -plist` 里的 `dev-entry` 带 `/dev/` 前缀。比较前把两边都规范成 `/dev/diskN`，并核对镜像的设备号和 inode。对不上、读回不完整或超时，脚本不做卸载、不删镜像，把尝试目录留在 `--out` 下，报告 `quarantine`，退出码 5。

创建之前先检查 `--out` 自己和它的每一级祖先：不能是符号链接，属主必须是当前用户或 root，并且没有组写和其他写。同一条路径再用 `/bin/ls -lde` 读 ACL。`deny` 条目留下（Mini 家目录上常见 `0: group:everyone deny delete`）。把 `write`、`append`、`delete`、`delete_child`、`add_file` 或 `add_subdirectory` 授给属主和 root 以外的人，包括新建的尝试目录和挂载目录上继承来的条目，就拒绝。读不到或解析不了也拒绝。然后在 `--out` 下记下本次尝试目录、挂载目录和镜像的 dev/inode，并先写 `--out/volume-receipt`（镜像路径、dev/inode、设备、挂载点、生命周期）。每个阶段更新这份凭据。它在卷外，不在临时目录里。quarantine 时保留它。人工处理以这份凭据为准，不要凭目录名删除。

每次删除文件或空目录之前，重新核对父目录和目标的 dev/inode，并且目标不能是符号链接。对不上就 quarantine，不按路径兜底删除。创建失败和半路失败同样适用。

卷上的写入、建目录和删除（含 `.metadata_never_index`、槽位预建、删除文件和空目录）都放进有界子进程，不在 bash 主进程里直接对卷做文件 IO。卷准备阶段的绝对截止是 40 秒。清理开始时算一个绝对截止，之后每条命令连同 TERM、KILL 和回收都夹在剩余时间里，不把额度重新加满。子进程无法确认退出就 quarantine，并且不再做后续动作。客机里卡住的系统调用仍由本次尝试的 VM 停止时限盖住；客机自己的计时器单独不构成硬上限。

卸载绑定的是只属于本次尝试的挂载点路径。新鲜的 `hdiutil info -plist` 必须同时满足：`image-path` 等于这份镜像且 dev/inode 一致，并且该镜像的挂载点等于本次在 0700 私有目录里创建的挂载点 M，且该镜像只有这一条挂载或 attachment。然后执行 `/usr/bin/hdiutil detach <M>`。失败时，在再次确认 VM 已停止并且同一份 info 仍然成立之后，最多执行一次 `/usr/bin/hdiutil detach -force <M>`。force 跨调用也至多一次：发出之前先把 `force=intent` 追加进凭据，写不进去就不 force；任何调用读到已有的 force 意图就不再 force，卷仍挂着则 quarantine。然后再做一次 info，确认清单里已经没有这份镜像。用过 VM 的 case 还要有 VM 已停止的证明。未知或仍有 attachment 就 quarantine，不删镜像。删除镜像之前，M₀ 的 dev/inode 必须仍与凭据一致且目录为空，尝试目录里只能有 `mailbox.sparseimage` 和挂载点目录。否则留下镜像并 quarantine。同一用户的其它进程若同时改这个私有目录，不在这层保护范围内。同一 UID 有意改写凭据（包括改写到目录里仅剩的那个 `s11v.*` 上）同样不在防护范围内。

本实验探针依赖所有者保留的排他窗口：运行期间不对这些私有目录做手工或自动的磁盘操作。这是 ADR 0019 §2 规则 1 允许的形式，不是生产实现。报告字段 `exclusive_window` 为 `assumed`。按路径删除沿用现有的先核对身份再删除，不把父目录 fd 绑到这次删除上。

凭据本身必须是当前用户拥有的普通文件、不是符号链接、模式 0600。凭据所在目录及其祖先和 `--out` 的祖先用同一套规则：不跟随符号链接，属主是当前用户或 root，没有组写或其他写，没有写类 ACL。尝试目录基名是 `s11v.*`，镜像基名必须是 `mailbox.sparseimage`，挂载点基名是 `mnt.*`，并且凭据目录里恰好有一个 `s11v.*`。阶段 R 每次 normal detach 和 force 之前都重做上面的完整映射核验。F3 凭据没有设备时，从这份新鲜清单里唯一的匹配项取出设备。映射变了或多出条目就 quarantine，不再 force。

阶段 R 整段共用一个 90 秒绝对截止。剩余时间不够下一步自己的上限（含回收余量）就不启动这条命令，改为 quarantine，不把命令改短。normal detach 仍是 20 秒上限。读凭据走有界子进程 `/usr/bin/head -c 8193`，超过 8192 字节即拒绝；控制器不直接读原文件。

开始前宿主所在卷的可用空间必须至少 256MiB，否则不创建镜像，结果是 unknown。这次观测记在 `host_pre`。C9 在放量开始前用同一条 256MiB 再查一次，记在 `host_fill`；不够则 unknown，原因 `host-space`，不发 `run C9`。客机报告 ENOSPC 时，卷上剩余必须不超过 8MiB，否则 unknown，原因 `enospc-origin`，以免把宿主卷写满当成卷满。C9 记录宿主可用空间下降的峰值，以及镜像逻辑大小（st_size）和占用（blocks×512）的峰值。`ok` 要同时满足：客机报告到达 ENOSPC、两个峰值都不超过 80MiB、宿主可用空间下降不超过同一时段镜像占用增量（峰值减去放量基线）再加报告里的 `host_margin`（1048576 字节）。基线时已经占用的块不算进允许下降。没观测到的 C4 大小、宿主下降和 enospc 记 unknown，不记 0。C9 终态记录必须是唯一、完整、以换行结束的一行，字段不重复，并且 v/op/role/mode 与本次边界一致。矛盾、截断，或客机非零、中止、溢出、超时，都记 unknown；留下结果文件也不能记 ok。后台 SSH 的标准输出和标准错误各自最多保留 65536 字节，到顶或收到取消就结束并回收，这两项观察在等待结束后仍然有效。

真实运行拒绝 `S11_HDIUTIL`、`S11_DISKUTIL`、`S11_PLUTIL`、`S11_MOUNT`、`S11_MDUTIL`、`S11_TMUTIL`、`S11_MDFIND`、`S11_STAT`、`S11_DF`、`S11_VOLIO`、`S11_LS` 这些工具路径覆盖，除非显式设置 `S11_TEST_MODE=1`。该变量只给桩测试用。

`mdutil -s` 只认原文里的 `Indexing disabled.` 或 `Indexing enabled.`。读不到就保持 unknown，不把没观测写成已经关闭。`tmutil isexcluded` 对着镜像路径，认 `[Included]` 或 `[Excluded]`。`tmutil addexclusion` 没测，失败或要权限就记 unknown。

quarantine 之后由人处理，脚本不会补一次删除。打开该次 `--out/volume-receipt`，按其中的镜像路径、dev/inode、设备、挂载点和生命周期核对，不要凭目录名直接删：

- `hdiutil info -plist`，看 `image-path` 是不是凭据里的镜像，挂载点是不是凭据里的 M。`dev-entry` 先规范成 `/dev/diskN`
- `stat` 凭据里的镜像，设备号和 inode 要和凭据一致
- `mount`，看该挂载点上有没有 nosuid、noexec、nodev、nobrowse。出现 noowners 时 owners 记 unknown，不因此跳过卸载

只有这些都指向同一次尝试，才可以手工 `/usr/bin/hdiutil detach` 凭据里的挂载点路径，再确认 info 里已经没有这份镜像，然后删除镜像和空目录。对不上就停，留给下一次人工判断。不要用 `sudo`。不要 `detach -force`，除非已经证明没有 VM 还拿着这个卷，并且仍然只对凭据里的挂载点路径执行一次。

C9 让客机通过固定的 `run C9` 写满卷（`data` 最多 80 个 1MiB，`small` 最多 8192 个 1KiB，遇到空间不足就停）。判定用上面的峰值和 `host_margin`。客机没有报告空间不足是 `unknown`；镜像或宿主可用空间下降越界是 `failed`。判定之后仍然按上面的身份规则卸载和删除。

## 阶段 V 的 C5 / C6 / C7 / C10，以及阶段 R

跑这些 case 之前必须先发一句：C5 的 F2 预期会留下镜像，需要人工按 volume-receipt 核对后再处理；C7 会在 VM 仍在运行时卸载本专用卷；C10 只读观察索引和备份状态，不改设置。没有这句话就不要跑。本段不构成运行授权。真实机器上这些 case 仍是 not_run。

C10 不起 VM，并且和 C4 一样拒绝 `--probe` 与 `--pinned-host-key`。它在卷根写下 16 个含本次随机 token 的 1KiB 文件，等一段观察时间，然后只读 `mdfind -onlyin`（只数标准输出；stderr 上的语言环境日志不计入）、`mdutil -s`、卷根 `.fseventsd` 的 stat（不读内容；存在时记下 uid 和模式）、`tmutil destinationinfo` 和两次 `tmutil isexcluded`。目的地个数只有在看到明确的 no-destination 行、并且没有目的地记录时才是 0。正数要求每条记录完整（Name、Kind、URL、ID 都有值），按 Name 计数。空输出、只有分隔线、只有 Kind、截断的 Name，或 no-destination 与记录混在一起，都记 unknown，不记 0。名字、URL 和挂载点不进报告。不调用 `addexclusion`、`startbackup`，也不改备份设置。判定一律是 `unknown`，原因 `observed`：看不到系统服务读取，不能证明没有读取或备份。命令不可用或超时只把对应字段记成 unknown，清理照旧。

C6 起 VM，必须带 `--probe` 和 `--pinned-host-key`。子例 `file` 和 `dir` 各是一次独立尝试（新卷、新 clone）。客机经固定命令 `run C6 file` 或 `run C6 dir` 握住预建槽位或邮箱目录，直到 VM 停止。宿主看到 `held=1` 后按普通清理路径停止并删除 VM、证明缺席，再 normal detach（失败时按普通规则至多 force 一次）并删除。记录 `guest_held`、`detach_rc`、`detach_sec`、`detach_class`（按退出码：0 为 none，16 为 busy）和 `force`。没看到 `held=1` 为 `unknown` / `guest-not-held`。stop、delete 都成功、缺席已确认且卷已删除才判 `ok`。宿主进程占用卷时的卸载行为不在 C6 里测：已在 Mini 的临时卷上手工测过（普通卸载退出码 16、文件被占用时 hdiutil 自己重试约 10.7 秒、force 约 0.14 秒、占用进程不会被杀），见 ADR 0019。

C7 起 VM。先复用 C1 的就绪槽位：客机 `run C1` 把 `guest-write/hg-ready` 写成唯一一行 `v=1 op=ready dir=hg`，宿主看到这一行才继续。没握手成功是 `unknown` / `not-ready`，不提前 detach。握手成功后、VM 仍在运行时，对本卷做一次 normal detach，绝不 force。detach 成功则让客机跑固定命令 `run C7`（对预建槽位做 4 次读、4 次写，每次独立线程、主线程有超时）。`run C7` 必须有界执行成功，并且恰好一行客机记录：各计数是数字，`ok+enoent+eio+other+hang` 等于 `ops`，`ops` 不超过 8。同时 `tart_alive=yes`，并且之后 VM 清理与缺席确认成功，才是 `ok` / `measured`。SSH 失败或超时是 `unknown` / `guest-failed`；记录畸形是 `unknown` / `guest-malformed`；Tart 已不在运行是 `unknown` / `tart-dead`。`run C7` 返回后、VM 仍在运行时再核对一次 M₀ 的身份和内容；VM 停止后、任何 unlink 之前再核对一次。出现条目则 `failed` / `m0-write`，留下镜像和 M₀。detach 失败只记录，不 force，仍跑一次 `run C7`。之后按正常流程停止 VM；若卷仍挂着，这时才允许按卷清理规则 force 一次。

C5 不起 VM。`--case C5 --fault F1|F2|F3|F4|F5` 在凭据写到一半时用 `kill -KILL` 模拟崩溃，不跑退出陷阱，因此这次可能没有报告。F1 在 create 之前，F2 在 create 成功但还没记下镜像 dev/inode，F3 在 attach 成功但还没记 mounted，F4 在 detach 成功但还没记 detached，F5 在 unlink 成功但还没记 gone。`--fault` 只对 C5 有效。

阶段 R 是 `--stage R --receipt <volume-receipt>`。它只接受 `case=C5` 的凭据。C5 不起 VM，构造上没有 VM。其它 case 在任何破坏性动作之前拒绝，原因 `recovery-unsupported`，退出码 2，不写 `recovery=noted`。通过解析和上面的信任链之后，才追加 `recovery=noted`，再按破坏性清单处理：新鲜 info 里若还有这份镜像，身份、设备和挂载点都对得上、且没有额外挂载或 attachment，才 detach；对不上就 quarantine，并且不 force。镜像还在但凭据没有镜像 inode（F2）就 quarantine 并保留镜像，不 unlink。证明没有 attachment、M₀ 为空且身份一致、尝试目录只有允许的名字之后，inode 一致才 unlink；镜像已不在则跳过 unlink，但尝试目录里若有意外名字仍 quarantine。挂载目录身份一致且为空才 rmdir。尝试目录只作为证据保留，阶段 R 不删除它。判定 `ok` 或 `quarantine`，quarantine 的退出码是 5。F1、F3、F4、F5 预期恢复成功且没有镜像或挂载目录残留；F2 预期 quarantine 且镜像还在，要人工按凭据处理。

阶段 R 拒绝 `--dry-run`、`--case`、`--fault`、`--probe` 和 `--pinned-host-key`。凭据解析失败则拒绝，不做任何卸载或删除。

## C8 双主体：owner 一次性准备

C8 要第二个**标准（非管理员）**账号，才能测两个主体在邮箱上的 UID/权限映射。golden `macos-s2-eval` 不动；总控已从它克隆出准备副本 `macos-s2-eval-c8`，并在 Mini 的 `~/cu2-ci/s11-0/c8/` 生成了只给该账号用的 ed25519 密钥（私钥不离开 Mini）。下列步骤由 owner 完成，所有密码由 owner 输入，总控不经手：

1. 在 Mini 上启动副本：`TART_HOME="/Volumes/Mini扩展/VMs/tart" tart run macos-s2-eval-c8`。
2. 以 `eval` 登录客机，在终端执行：`sudo sysadminctl -addUser gcuprobe -fullName "GCU probe" -password -`（不加 `-admin`，即标准账号；新账号密码自定、不必告知）；建 `/Users/gcuprobe/.ssh`，把总控给出的公钥写入 `authorized_keys`，属主 `gcuprobe:staff`，目录 0700、文件 0600；如有远程登录用户限制，执行 `sudo dseditgroup -o edit -a gcuprobe -t user com.apple.access_ssh`。
3. `dscl . -read /Groups/admin GroupMembership` 的输出里不应有 `gcuprobe`。
4. `sudo shutdown -h now` 关机，告诉总控。

之后总控以 headless 启动该副本，核对 `gcuprobe` 非管理员、能用该密钥登录，再实现并评审 C8 的双主体映射，最后按阶段 M 的规则运行（每次用一次性 clone，准备副本本身不运行实验）。

## 桩测试

```bash
bash computer-use/scripts/test_s11_0_probe.sh
```

这是手动脚本，已从 `scripts/verify.sh` 撤下，不是 CI 门禁。它不使用 GNU `timeout`。通过只说明桩行为，不说明 Mini 上的网络事实。
