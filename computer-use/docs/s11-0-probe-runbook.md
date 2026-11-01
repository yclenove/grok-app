# S11-0 无模型探针手册

状态：**脚本与本手册已写，真实 VM 未运行（not_run）**。这与 ADR 0018 §10 的记录一致。Linux 上的桩测试只检查参数、清理和报告形状，不能当作 Softnet 或 Tart 的测量。本手册不构成运行授权。

## 给 owner 的一句话（首次运行前必须先发）

将在 Mac mini 上对固定 golden `macos-s2-eval` 做一次无模型探针：阶段 A、阶段 B 各 `tart clone` 一台新的 `s11-0-probe-<阶段>-<pid>-<随机>`，只停止并删除本进程在 `tart list` 里确认是自己创建的那一台。上限 26 分 30 秒（探测 1500 秒，另加清理 90 秒）。运行期间 Mini 上不要做 GUI 测试，不改防火墙、路由或 DNS，不会启动、修改或删除 golden，也不会按前缀删除其它 VM。

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

常用参数：`--stage A|B|C|all`（默认 all），`--out`（默认 `./s11-0-out`），`--lan-ip <IPv4>`（不给则 LAN 记为未测量；不扫描），`--ssh-key`，`--tart-home <路径>`，`--dry-run`。没有 `--golden`、`--clone-name`、`--guest-user`、`--hostname`、`--softnet`。拒绝 root。dry-run 走同一套 argv 构造，只打印，不 clone。

## 时长与清理

探测预算 1500 秒（bash `SECONDS`，不看墙钟）。清理另有 90 秒，不占这 1500 秒。强制总上限 1590 秒（26 分 30 秒），含清理。版本、列表、帮助、IP 每次最多 10 秒，clone 180 秒，`tart exec` 20 秒，ssh 50 秒，等 IP 240 秒，stop 和 delete 各 30 秒。这些短上限都受总预算约束。输出用 `head -c` 截到 64KiB。

清理期间忽略 INT、TERM、HUP，避免第二次信号打断 stop/delete。清理结束后用 `tart list` 确认该名字已经不在，而不只看命令退出码。Ctrl-C 在清理开始前会停下后续阶段，并清掉本进程已经确认创建的 clone。

SIGKILL 或断电可能来不及清理。那时只允许对报告里 `created` 为 `yes` 且 `absent` 不是 `yes` 的那一个完整名字手工 `tart stop` / `tart delete`。不要按前缀删，不要用通配符。

## 测什么、不测什么

阶段 A：`--net-softnet`。阶段 B：再加 `--net-softnet-block=0.0.0.0/0` 和 `--net-softnet-allow=<ip>/32`。报告里的 argv 就是传给 `tart run` 的那一个数组。

每次测量：gateway 的 TCP 22/53/80/443，gateway 与一个公网解析器的真实 DNS（UDP/53，guest 上的 `dig`，否则 `host`，否则 `nslookup`），metadata `169.254.169.254` 的 TCP 80，是否有全局 IPv6 地址，以及到一个公网 IPv6 的一次 TCP。阶段 B 另加获批或占位端点的 TCP 443。A 和 B 在 VM 起来之后各做一次有界的 `tart exec <clone> /usr/bin/true`，即使 ssh 失败也做。阶段 C 只记录 `tart --help` 和 `tart exec --help` 里的子命令/选项事实，不 clone。

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

clone/tart 的原始输出留在私有临时文件里，报告只出现白名单原因码。

## 退出码

| 码 | 含义 |
| --- | --- |
| 0 | `complete`，或 dry-run 的 `not-run` |
| 2 | 用法或前置检查失败，没有改 VM |
| 3 | 创建被拒绝或不明确（名字已被占用、clone 未确认）。不删除 |
| 4 | guest 采集不能跑、非零退出、超时，或没有地址。此时状态不是 `complete` |
| 5 | 采集成功，但 stop/delete 失败，或删完后 `tart list` 仍能看到 / 无法确认 |

采集失败优先于清理失败。

## 决策规则（ADR 0018 §10）

若这份实测显示 Softnet 单独已满足 §5 的拒绝类别，则 §5 第 3 点的宿主网络 guard 不进入范围。若不满足，先把具体 guard 方案（影响范围、回滚、是否只作用于该 Tart/Softnet 实例）提交 owner 审阅，批准前不安装任何网络规则。若 H1 没有可用的非 IP 通道，暂停 S11 并回到 owner，不为凑通道去开放 gateway 上的 IP 服务。脚本不下这些结论。

不要改宿主的 PF、防火墙、路由或 DNS。

## 桩测试

```bash
bash computer-use/scripts/test_s11_0_probe.sh
```

这是手动脚本，已从 `scripts/verify.sh` 撤下，不是 CI 门禁。它不使用 GNU `timeout`。通过只说明桩行为，不说明 Mini 上的网络事实。
