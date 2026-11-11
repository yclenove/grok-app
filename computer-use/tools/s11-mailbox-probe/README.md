# S11-0c 邮箱机械探针

只在 Mini（macOS）上跑真实路径。其它平台打印 `unsupported` 并以 2 退出，而且不会创建 `--out`。`cargo test` 不调用 tart、ssh、ssh-keygen 或 ssh-keyscan。

本卡只做 C1、C2、C8。导出目录是宿主上的普通私有目录，不是 hdiutil 限容卷。报告字段是 `export: host-private-directory (not the capped HFSX volume)`。HFSX 上的差异由后续 M2c/M7 补测。卷、detach、C4 不在本卡。

## 跑之前

所有者先发一句话，确认这次是 **DEFAULT 网络**机械实验、网关会暴露。没有这句话就不要跑。不要 sudo，不要改 golden `macos-s2-eval`。

`--pinned-host-key` 是总控一次性从已经验证过的 golden 访问里提取的同一把 ssh-ed25519 公钥。每个 clone 继承这把主机密钥。探针用 `StrictHostKeyChecking=yes`，只认这个文件，不做 TOFU，也不跑 ssh-keyscan。

## 命令

在 `computer-use/` 下：

```text
cargo run --manifest-path tools/s11-mailbox-probe/Cargo.toml -- \
  host-run --case C1 --out /绝对路径/新目录 --pinned-host-key /绝对路径/golden_ed25519.pub
```

`--case` 只能是 `C1`、`C2`、`C8`。`--out` 必须是还不存在的绝对路径，父目录要满足私有根（叶子当前用户 0700，祖先由 root 或当前用户所有且没有组/其他写权限、没有 ACL 写授权）。tart 默认是 `$HOME/bin/tart`，可用 `--tart` 指定。`--tart-home` 会作为 `TART_HOME` 传给 tart。`--dry-run` 只写报告，不起 VM，也不读探针或公钥。

整次运行有 240 秒绝对截止，其中 60 秒留给清理。命令截止是 `min(命令上限, 剩余时间)`，不会重新计时。四个 case 分开跑，不要并行。

## 怎么读报告

`<out>/report.json` 是唯一结论。`evidence` 固定为 `DEFAULT-network/mechanics-only`。`ok` 是声明的观测完整完成。`failed` 是观测到违反。`unknown` 是证据缺失、不完整或超时。`quarantine` 是清理归属或 VM 缺席没有证实，之后的子例停止。没跑的子例是 `not_run`。`block` 只用于 C1 的 RTT。C2 的 `not-ack` 是一次完成的观测（故障边界上没有 ACK），进程退出码是 0；报告仍是结论。退出码：`ok` / `not-ack` / `dry_run` 为 0，`failed` / `unknown` / `quarantine` / `block` 为 1，用法错误为 64。

## 清理

清理只对本次创建并记录了 dev/inode 的文件和目录，通过目录 fd 做 `unlinkat` / `rmdir`。遇到未知条目、符号链接或身份不符就不删，整个目录留着，并记 `quarantine`。绝不递归删除。邮箱目录故意保留，客机写入的帧留在里面，这本身不是 quarantine。

VM 只有在 clone 退出码 0 且 `tart list --format json` 出现这个名字时才算本次所有。有歧义就不删。stop 之后要看到状态正好是 `stopped`。delete 之后要用一份完整、可信的清单确认缺席。清单不完整或解析失败不算缺席。过了绝对截止就不再 delete、不再 unlink。

`quarantine` 之后操作者留着 `--out`，先看 `tart list --format json`，再决定是否手工删除。不要按名字前缀删 VM。
