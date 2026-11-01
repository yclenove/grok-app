# S11-0 无模型探针手册

状态：**脚本与本手册已写，真实 VM 未运行（not_run）**。Linux 上的桩测试只验证顺序、清理和脱敏，不能当作 Softnet 或 Tart 的测量。

## 给 owner 的一句话（首次运行前先发）

将在 Mac mini 上对 `macos-s2-eval` 做一次无模型探针：只 `tart clone` 出一次性副本，大约 25–45 分钟（硬上限 40 分钟），运行期间 Mini 上不要做 GUI 测试，不改防火墙、路由或 DNS，不会启动、修改或删除 golden 本体。

## 不必 owner 动手

- Softnet 已由 owner 安装并设置 SUID。脚本只 `ls -l` 检查属主 root 和 `s`/`S` 位，不执行 softnet，不用 sudo，不安装任何东西。
- 不输入、不保存密码或令牌。SSH 只用已有的公钥（默认 `~/.ssh/id_ed25519_tart`，可用 `--ssh-key` 覆盖）。
- 脚本自己完成 clone、探测、`tart stop` 和 `tart delete`。删除前再次核对名字前缀 `s11-0-probe-`，只删本进程创建的这一份。
- 不改 PF、防火墙、路由或 DNS，不用 `pkill`。

## 需要 owner 在场

- 收到上面那句话，并确认这段时间 Mini 不做 GUI 测试。
- 若阶段 B 要对照**已批准的模型端点**，事先给出一个公网 IPv4，运行时传 `--endpoint-ip`。未给则阶段 B 使用 RFC 5737 文档地址的 `/32` 作对照，报告里 `endpoint_source=placeholder`。该对照**不是**模型端点的证据。
- 跑完后由人读报告，按下方决策规则决定下一步。脚本不下结论。

## 在 Mac mini 上怎么跑

非交互 SSH 的 PATH 里没有 `~/bin/tart`。在 Mini 上：

```bash
export PATH="$HOME/bin:$PATH"
# 默认 TART_HOME 是 /Volumes/Mini扩展/VMs/tart，可用 --tart-home 覆盖
bash computer-use/scripts/s11-0-probe.sh --dry-run --stage all
bash computer-use/scripts/s11-0-probe.sh --stage all --endpoint-ip <获批IPv4> --out ~/s11-0-out
```

常用参数：`--stage A|B|C|all`（默认 all），`--golden`（默认 `macos-s2-eval`），`--ports`（默认 `22,53,80,443`），`--out`（默认 `./s11-0-<utc>/`）。拒绝 root。golden 与 clone 同名，或 golden 本身使用 `s11-0-probe-` 前缀，脚本直接退出且不做改动。前置检查失败（无 tart、softnet 不是 SUID、golden 不在 stopped、磁盘不足、密钥不可读）退出码 2，不 clone。

## 时长

正常大约 25–45 分钟。默认总时限 2400 秒（40 分钟）。clone、启动、等 IP、SSH、单次探测、stop、delete 都有单独上限。Ctrl-C 和异常走同一清理。

## 怎么读报告

`--out` 目录里有 `report.json`（schema `s11-0-probe/v1`）和 `report.md`。每条探针有 `probe_id`、`stage`、`target`、`result`、`elapsed_ms`、`detail`。

| result | 含义 |
| --- | --- |
| reachable | 明确连通或明确出现（连上、有应答、有地址、命令退出 0） |
| blocked | 明确拒绝（ICMP unreachable / refused / prohibited / no route）或明确缺失 |
| error | 超时、工具失败、SSH/IP 失败。**超时不是 blocked** |

地址只记角色和策略 CIDR：`gateway`、`lan-redacted`、`metadata`、`public`、`endpoint`，以及 `0.0.0.0/0` 与 `endpoint/32`。不记录宿主 LAN 的真实网段，不记录主机名以外的个人信息、时区、代理、家庭宽带或密钥路径。`cleanup.delete` 应为 `ok`，`clone_names` 每项都应以 `s11-0-probe-` 开头。阶段 B 另有两条事实：宿主到 guest 的 `tart ip` / SSH 在 block 之后是否仍可用；guest 到放行 `/32` 的 TCP/443 与 UDP/443 结果如何。阶段 C 只记录 `tart` 子命令、`tart exec /usr/bin/true` 和 guest 上 vsock/串口设备是否出现，不推断 H1 成立。

UDP/TCP 的 blocked 只表示当时观察到的拒绝文本，不是 Softnet 计数器。没有外部计数时，对应类别保持 unknown / not_run（ADR 0018 §5）。

## 决策规则（ADR 0018 §10）

若这份实测显示 Softnet **单独**已满足 §5 的拒绝类别，则 §5 第 3 点的宿主网络 guard 不进入范围。若不满足，先把具体 guard 方案（影响范围、回滚、是否只作用于该 Tart/Softnet 实例）提交 owner 审阅，**批准前不安装任何网络规则**。若 H1 没有不经 IP 的通道，暂停 S11 并回到 owner，不为凑通道开放 gateway 上的 IP 服务。
