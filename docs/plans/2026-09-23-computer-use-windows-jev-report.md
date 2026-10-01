# Windows Computer Use 这一轮能做什么

日期：2026-09-23  
仓库：`H:\aicoding\grok-app-computer-use`  
总发布状态仍是 **partial — not releasable**。这一轮交付的是本机 Windows 上的授权循环和 Jev 选题，不是三系统安装版。

## 现在在这台 Windows 上可以做什么

Computer Use 默认关闭。未打开开关时，Broker 拒绝 `open_run`，证据是两次运行都先打印 `gate: feature_default_off`。

打开开关并只授权自建测试窗口之后，同一条已发布路径会：

1. 观察该窗口，从这次观察里的元素选题。
2. 点击 Count 一次。窗口计数和独立文件都是 `1`，不是 `2`。
3. 把编辑框设成调用方事先给出的 `Ada`。控件文字和独立文件都正好是 `Ada`。
4. `request_stop` 之后再来的点击被拒绝，计数停在 `1`，没有重放。
5. 授权窗口关闭后，原点击被拒绝，旁边另一个窗口的计数保持 `0`，没有退回全桌面。
6. 焦点被另一个窗口抢走时，点击被拒绝。`gate: windows_focus_drift_pauses` 已通过。

两次先点击再输入的记录相同：`windows-use-1.txt` 与 `windows-use-2.txt`。停止、失效目标和焦点记录在 `windows-stop.txt`。

另一次把已捕获的 `set_value` 响应经 `choose_with` 注进同一条 Windows 循环，没有新的 TypeSafe 请求。记录在 `windows-captured.txt`：先写入 `Ada`，当时点击计数是 0；随后才点击，计数变成 1。`set_value` 没有被当成 Count 点击。

环境变量 `TYPESAFE_API_KEY` 存在时，循环调用 `choice::choose`，再按 `accept_step` 分发。没有密钥、也没有注入响应时，顺序仍是先点击再写入。没有密钥时产品不会自己联网。

## 吸收了什么

八个仓库在 `vendor/jev-sources/`，不进入产品树。逐仓决定见 `docs/plans/2026-09-23-jev-computer-use-absorb.md`。

吸收进 `grok-computer-use-core::choice` 的只有这个循环：一次观察、并行的操作题和目标题、只执行被选中的目标、文字必须是调用方绑定的原文、置信度低于 `0.6` 或目标不在本次观察里就停止。`choose` 只在环境变量有密钥时向 `https://api.typesafe.ai/v1/systemone` 发 `model=jev-latest`。

拒绝了这些仓库自带的浏览器、macOS/Swift worker、第二套 Windows MCP/UIA 代理，以及没有密钥时静默改走聊天模型。那些路径不会经过本仓库的单目标授权、exclusive lease 和默认关闭。

另外修了一处本机双击：带窗口句柄的按钮以前同时发送 `BM_CLICK` 和鼠标按下/抬起，一次动作变成两次命令。现在只发送 `BM_CLICK`。

## 真实 Jev 结果（已去掉密钥）

两次请求都由已发布的 `fetch_decision` 发出，选择一致：

| 调用 | 模型 | operation | 目标 | 置信度 |
| --- | --- | --- | --- | --- |
| 1 | jev-1.13.0 | set_value | e-name | 0.84 |
| 2 | jev-1.13.0 | set_value | e-name | 0.85 |

已发布的 `choose_with` 从 `prepare` 进入，并喂入第一次捕获的响应。`accept_step` 把它收成 `ReplaceText`，元素是 `e-name`，文字只有调用方绑定的 `Ada`，不是点击。把目标重绑定到当前观察里的编辑框后，同一函数仍然是 `ReplaceText`。把置信度改成 `0.1`，或把目标改成观察里不存在的元素，同一函数停止。该测试跑了两次，都通过。没有留下被忽略的在线测试。

密钥没有写入仓库、下载说明或证据文件。

## 还不能当成 Codex 那样交付的部分

- 没有把真实 Grok 会话接上这个循环，所以还不能在 App 里用一句话驱动任意已打开的软件。
- 没有操作本机日常 Chrome、Edge 或已保存密码。
- macOS、Linux、签名安装包和 12 小时长稳都没有新证据。
- Jev 只在配置了密钥时选题；密钥不在产品里。
