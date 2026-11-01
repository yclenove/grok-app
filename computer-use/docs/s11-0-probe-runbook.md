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
  --endpoint-ip 203.0.113.10 --tart-home '<路径>'
bash computer-use/scripts/s11-0-probe.sh --stage all \
  --endpoint-ip 203.0.113.10 --tart-home '<路径>' --out ./s11-0-out
```

没有获批地址、只做文档地址对照时：

```bash
# 示例。报告会写 endpoint_source=placeholder，不能当作端点证据。
bash computer-use/scripts/s11-0-probe.sh --stage B --endpoint-placeholder --tart-home '<路径>'
```

常用参数：`--stage A|B|C|all`（默认 all），`--out`（必须是全新不存在的目录；默认 `./s11-0-out`。脚本拒绝已存在的目录或符号链接，避免把陈旧报告当成这次的证据），`--lan-ip <IPv4>`（不给则 LAN 记为未测量；不扫描），`--ssh-key`，`--tart-home <路径>`，`--dry-run`。没有 `--golden`、`--clone-name`、`--guest-user`、`--hostname`、`--softnet`，也没有可复用的 clone 名字参数或环境变量。拒绝 root。dry-run 走同一套 argv 构造，只打印，不 clone。

## 时长与清理

探测预算 1500 秒（bash `SECONDS`，不看墙钟）。每次非清理命令的限时会被夹到剩余探测时间。清理使用一份累计清理额度 90 秒，阶段之间的清理和退出时的再清理共用这一份，不按次重置。单次清理命令的限时先扣掉 1 秒，把 TERM 之后的短宽限留在额度里。实际宽限是若干次 0.05 秒轮询，到点仍在的子进程会被 KILL。中途清理计入探测计时：清理进行时 `SECONDS` 继续走，后面的阶段只剩更少的探测时间，同时这笔时间也从累计清理额度里扣。宽限、写报告和最后一次回收仍可能超出这两个名义数字，因此不承诺精确的总硬上限。

版本、列表、帮助、`tart ip` 每次最多 10 秒，clone 180 秒，`tart exec` 20 秒，就绪 ssh 20 秒，采集 ssh 50 秒，等地址最多 240 秒，stop 和 delete 各 30 秒。这些都再被剩余时间夹住。有限命令的输出在产生时用 `head -c` 限到 64KiB；超过是 `capture-overflow`，不会把截断的 `tart list` 当成“不存在”。`tart run` 的标准输出和标准错误直接丢弃，不留原始日志。

清理期间忽略 INT、TERM、HUP，避免第二次信号打断 stop/delete。清理结束后用可信的 `tart list` 确认该名字已经不在，而不只看命令退出码。Ctrl-C 在清理开始前会停下后续阶段，并只清掉本进程已经确认创建的 clone。阶段 A 的 stop、delete 或确认失败时不进入阶段 B，也不覆盖 A 的记录。

脚本不处理 SIGKILL。被 SIGKILL 或断电杀死的运行可能没有最终报告。没有这份报告时，不能用输出目录里的旧 json 认领 clone。owner 只对照自己通知过的那一个完整名字，手工看 `tart list`；不得按前缀、通配符或旧报告删除。clone 失败、超时或结果不明确时，脚本自己也不会删除，原因是 `ambiguous-destination`，并停在后续阶段之前。

## 测什么、不测什么

阶段 A：`--net-softnet`。阶段 B：再加 `--net-softnet-block=0.0.0.0/0` 和 `--net-softnet-allow=<ip>/32`。报告里的 argv 就是传给 `tart run` 的那一个数组。

每次测量：gateway 的 TCP 22/53/80/443，gateway 与一个公网解析器的 DNS（guest 上只跑 `dig -r +notcp +ignore +time=2 +tries=1 @<解析器> example.com A`；`-r` 忽略 `.digrc`，`+notcp` 与 `+ignore` 禁止截断后改走 TCP），metadata `169.254.169.254` 的 TCP 80，是否有全局 IPv6 地址，以及到一个公网 IPv6 的一次 TCP。没有 `dig` 时该观察是 `tool-error`，`dns-udp-gateway` 和 `dns-udp-public` 进入 `unmeasured`，不换别的解析工具。阶段 B 另加获批或占位端点的 TCP 443。`tart ip` 没有地址、或 sshd 尚未接受固定的 key-only 就绪命令时，在启动预算内重试；采集命令只在就绪后跑一次。A 和 B 另外各给 `tart exec <clone> /usr/bin/true` 一段有界的启动机会，不依赖 ssh 是否成功，成功也不说明它不经 IP。阶段 C 只记录 `tart --help` 和 `tart exec --help` 里的子命令/选项事实，不 clone；其中任一失败则这次采集失败，退出码非零。

`tart list` 按表头列起始偏移切分，不按空白切（`Disk Size` 这类值含空格）。解析只要求存在 Name，以及 State 或 Running。空输出、缺这两列表头、或数据行短于状态列，都是库存不可信（`list-untrusted`），不是“VM 不存在”。报告里的 `inventory_columns` 是这次表头里通过白名单的列名原文。

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

## 桩测试

```bash
bash computer-use/scripts/test_s11_0_probe.sh
```

这是手动脚本，已从 `scripts/verify.sh` 撤下，不是 CI 门禁。它不使用 GNU `timeout`。通过只说明桩行为，不说明 Mini 上的网络事实。
