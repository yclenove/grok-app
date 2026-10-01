# Jev Computer Use 仓库：吸收或拒绝

日期：2026-09-23  
下载目录：`vendor/jev-sources/`（gitignore，不进入产品提交）

八个仓库都是浅克隆的默认分支。Jev 是 TypeSafe 的决策接口，不是可以替换本仓库 Broker 的运行时。

| 仓库 | HEAD | 决定 |
| --- | --- | --- |
| browser-use/jev-ultrafast | `1231850a0bf1a0c0341fe408ef1668dbbfdfac46` | 吸收决策形状，拒绝浏览器驱动 |
| awlevin/typesafe-computer-use | `cc7b5066ae1a07b5e3182e8f87a9b5b6dfdcffc1` | 吸收「不把截图交给决策模型、低置信停止」，拒绝 macOS 驱动 |
| kofanlabs/typesafe-computer-use-windows | `df0e6a98efe877524567af89ca036e882414e704` | 吸收「宿主供文本、完成信号不算成功」，拒绝第二套 MCP/UIA 代理 |
| paulsmith/computer-use-jev | `ff0ad8ba8e25755d37fb8d93718144c4568a596b` | 吸收封闭 token 和置信阈值，拒绝 Swift worker |
| savka777/jev-use | `b22e568adf110eeb64f89bf4d731a69d23c461dc` | 拒绝语音桌面代理；只保留「输入文字来自用户原话」 |
| max1874/jev-computer-use | `0e3b82e6aa65427b03dd345bbd04fd598aefa0ec` | 拒绝。它是 ultrafast 的 macOS 移植，并允许无 key 时静默改走聊天模型 |
| yikangy873-gif/jev-desktop | `9b02783ed96a81f2529827492de708ca1956c265` | 吸收分层：规划者限定范围，Jev 只选题，执行前重新观察 |
| jkudish/jev-browser | `f731c7aa773f55a30dad7175bbb358b2d8529e57` | 拒绝独立 Playwright MCP |

## browser-use/jev-ultrafast

吸收：一次观察变成元素表；同一次请求里并行问操作和各个操作的目标；只执行被选中的那个目标头；`DONE` / `BLOCKED` 不产生点击；自由文本不由 Jev 生成。这些已经在 `grok-computer-use-core::choice`。

拒绝：Browser Harness、它自己的 Chrome 配置、自动执行循环，以及用另一个模型现写输入文字。那会绕过本仓库的授权、租约和默认关闭开关。

## awlevin/typesafe-computer-use

吸收：决策模型只看结构化文本，不接收截图；置信度不够就停，不盲目点击。

拒绝：macOS OCR、Accessibility 和 Quartz 输入。本机执行仍走现有 Windows adapter。

## kofanlabs/typesafe-computer-use-windows

吸收：调用方提供要输入的文字；Jev 说完成并不等于独立后置条件通过；停止后不再发下一步输入。

拒绝：把它的 MCP 服务器、PrintWindow/UIA/发送输入循环装进产品。那是另一套代理，不会经过本仓库按目标授权和 exclusive lease，也会在焦点离开已授权窗口后继续输入。

## paulsmith/computer-use-jev

吸收：目标 token 只能来自当前快照；动作置信度低于阈值时返回分布并停止；代码把选择题映射成工具调用。

拒绝：Swift Accessibility worker，以及让另一个大模型拆解复合目标后再直接操作桌面。

## savka777/jev-use

拒绝整仓。它是 macOS 语音代理，按前台应用的辅助功能树直接操作，没有本仓库的单目标授权。

保留的规则：要输入的文字必须是用户已经给出的字面量，不由决策模型编写。安全输入框不进入状态。

## max1874/jev-computer-use

拒绝。动作空间和 ultrafast 重复。没有 Jev key 时它改走聊天模型，这会被当成静默替换，不能接到默认路径。

## yikangy873-gif/jev-desktop

吸收：外层代理保持目标、权限和最终核对；Jev 只在允许的控件里选题；执行前核对观察是否仍新鲜；敏感控件交还外层，而不是自己扩大权限。

拒绝：Codex 插件、macOS Computer Use 运行时，以及它的 loopback 执行桥。本仓库继续用自己的 Broker。

## jkudish/jev-browser

拒绝。它自带 Playwright 和 MCP，会另开浏览器会话。页面上的 goal/stuck 分数不能代替本仓库的授权和后置条件。
