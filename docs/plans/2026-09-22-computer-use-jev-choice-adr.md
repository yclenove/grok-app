# ADR：只吸收 Jev 的封闭决策，不吸收它的桌面运行时

日期：2026-09-22  
状态：已接入 Core 提案层，默认不联网、不执行、不授权

## 调研结论

Jev（TypeSafe，2026-09-15 发布）不是 Computer Use 产品。它是一次并行返回
Choice / Score / Noul 的决策模型，接口是 `POST https://api.typesafe.ai/v1/systemone`。
社区随后用它做了多套操作循环，都是外部项目：

| 项目 | 它实际做什么 | 和本仓库的关系 |
| --- | --- | --- |
| browser-use/jev-ultrafast | DOM 元素表 + 一次 Choice 选操作和元素；只有 TYPE_TEXT 才叫小模型写字 | 决策形状可借鉴。浏览器驱动是 Browser Harness，不能替换受管浏览器或 ExistingTab |
| awlevin/typesafe-computer-use | macOS OCR + Accessibility，Jev 选下一步 | 只覆盖 Mac，且把屏幕文本送出本机 |
| paulsmith/computer-use-jev | macOS Swift Accessibility worker + 封闭 token | 动作循环的拒绝规则可借鉴。Swift worker 不能接 Windows Broker |
| yikangy873-gif/jev-desktop | 留在 Codex Computer Use 里，让 Jev 选动作 | 最接近我们要的分层，但是 Codex 插件，不是可嵌入的运行时 |
| jkudish/jev-browser、MahmoudAdelbghany/jev-browser | 独立 Playwright / MCP 循环 | 会另起一套浏览器和权限，和现有 lease 冲突 |

这些项目的共同有效部分是：观察结果先变成封闭元素表；一次请求同时问操作和各操作的目标；
代码只执行被问到的那一个目标；目标必须来自当前观察；置信度不够就停；自由文本不由决策模型发明。

不能吸收的部分：它们的浏览器、macOS worker、截图循环、以及“选中就点击”。那些路径没有
我们的 surface、授权、generation、exclusive lease 和 unknown 不重放。

## 本仓库怎么接

`grok-computer-use-core::choice` 只做两件事：

1. `prepare` 从当前 `Observation` 生成 state 和 TypeSafe Choice questions。
2. `decide` 把一份 Choice 答案收成 `Proposal`，或明确 `Stop`。

`bind_proposal` 只在调用方另外给出 run、target、snapshot 和 generation 时，才拼出
可交给现有 `ActionRequest::validate_schema` 的请求。它不调用 Broker，也不产生坐标、
key 或 drag。

约束：

- 元素表最多 32 项，只含 ref、role、name 和已声明动作。不写入截图、坐标、页面正文、
  Cookie、URL 或输入值。
- 操作只从当前 surface 已有的语义能力里取：`click`、`set_value`、`type_text`、
  `scroll_up`、`scroll_down`、`wait`，外加 `done` 和 `blocked`。Desktop 的滚动是坐标能力，
  所以不会出现在提案里。WebView 当前能力是 NONE，因此没有可执行提案。
- `set_value` / `type_text` 只有调用方已经按元素绑定了字面文本时才进入选项。答案不能
  自带要输入的文字。
- 置信度默认低于 0.6、选项不在本次封闭集合、或概率和选择互相矛盾时，结果是 Stop。
- 本模块不读取 API key，不发起 HTTP。以后若要调用 Jev，必须是用户显式打开的独立开关，
  并且仍只返回提案；执行继续走现有授权和 lease。

## 没有改变的发布状态

总状态仍是 **partial — not releasable**。这次没有闭合完整浏览器退出、工具栏手势、
安装版、真实模型或三 OS。Jev 提案层不是 Computer Use 已经可用的证据。
