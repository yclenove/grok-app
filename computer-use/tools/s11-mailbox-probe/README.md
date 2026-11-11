# S11-0c 邮箱机械探针

只在 Mini（macOS）上跑真实路径。其它平台会打印 `unsupported` 并以 2 退出，而且不会创建 `--out`。本目录的 `cargo test` 不调用 tart、hdiutil 或 ssh。

## 跑之前

所有者先发一句话，确认这次是 **DEFAULT 网络**机械实验、网关会暴露。没有这句话就不要跑。不要 sudo，不要改 golden `macos-s2-eval`，不要给 tmutil 加 `-p`/`-v`。

## 命令

在 `computer-use/` 下：

```text
cargo run --manifest-path tools/s11-mailbox-probe/Cargo.toml -- \
  host-run --case C1 --out /绝对路径/新目录
```

`--case` 只能是 `C1`、`C2`、`C4`、`C8`。`--out` 必须是还不存在的绝对路径。tart 默认是 `$HOME/bin/tart`，可用 `--tart` 指定。`--tart-home` 会作为 `TART_HOME` 传给 tart。`--dry-run` 只写计划，不起 VM。

每个子例是一次独立 attempt：新卷、新 clone、负载不超过 60 秒、清理不超过 60 秒。C1 一轮大约数分钟；C2 三个子例；C4 两个子例；C8 通常是 unknown。四个 case 分开跑，不要并行。

## 怎么读报告

`<out>/report.json` 是唯一结论。`evidence` 固定为 `DEFAULT-network/mechanics-only`。看每个子例的 `verdict`、`samples`、`max`、`errors`、`unknown_reason`。`ok` 只表示这一轮机械观察完成。C2 的 `not-ack` 表示故障前没有 ACK，不能当成断电持久。`block` 是配方或时延挡住了，不能靠多发几次绕过。报告里没有宿主路径和个人信息。

`<out>/ledger/events.jsonl` 在卷外，先记意图再记效果。清理只认账本里的精确身份。

## 清理和隔离

`cleanup_absent: true` 才算这一例的 VM、挂载和镜像都已确认消失。`verdict` 为 `quarantine` 或 `ambiguous` 时，程序不会 detach、不会 unlink、不会删除对不上的 VM，并停止后面的子例。目录和镜像留在 `--out` 里。不要按名字或前缀手工删 VM，不要 detach 一个没有绑定设备号的盘，不要对残留目录做递归删除。把 `--out` 整目录留给所有者，下一次用新的 `--out`。
