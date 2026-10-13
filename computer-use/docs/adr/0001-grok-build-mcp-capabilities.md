# 0001 Grok Build CLI 1.0.52 的 MCP 能力

状态：提议
日期：2026-10-13

## 背景（事实 + 证据链接）

被测对象：`grok 1.0.52 (0920c8169e27) [stable]`（`/home/claude/.local/bin/grok`）。隔离 `GROK_HOME=/tmp/s01-home`（stdio）与 `/tmp/s01-home-http`（HTTP），工作目录 `/tmp/s01-work`。探针是 `computer-use/spikes/s01/mcp-server.mjs`（NDJSON stdio，另有 `--http PORT` 的 streamable HTTP）。该目录只进本地 `.git/info/exclude`，不进仓库。

为了让隔离 home 能调模型，把 `~/.grok/auth.json` 复制到上述两个 home，权限 `0600`。没有读取、打印或记录凭据内容，也没有改 `~/.grok`。实验结束后已删除这两个副本。`--always-approve` 只用于这两个隔离 home。

下文 headless 命令都带：`--model grok-4.7 --leader-socket <home>/leader.sock --cwd /tmp/s01-work --disable-web-search --no-plan --verbatim --output-format json`。看图试验另加 `--tools search_tool,use_tool`，并拒绝 `read_file,grep,list_dir,search_replace,run_terminal_cmd,web_search,web_fetch,task`。图里的字符是 `K7NP4R`，只画在像素里。

### 1. image content 是否到达模型 — 否（无条件）；超过实测尺寸下限后为是

`344×120`（631 字节）的 MCP `image` 块被摊成工具结果字符串里的 `data:image/png;base64,...`，`chat_history` 里没有 image part。`grok-4.7 --effort xhigh --max-turns 4` 重复 3 次，没有一次读出 `K7NP4R`：

| 会话 | stopReason | 模型可见结果 |
|---|---|---|
| `01a1361c-9f7a-7610-a659-a198597b9b30` | `max_turn_requests`（4 轮） | 数据 URL。回复要自己解码，没有读出字符 |
| `01a1361f-2ae7-7f02-a975-968327f53992` | 同上 | 同上，最终文本为空 |
| `01a13620-9c97-7bf1-9b20-bd5f411806b1` | 同上 | 「I'll decode the attached PNG…」，没有读出字符 |

同一宽度下，白边垫高后的分界（`padRows`，文件仍只有约 0.7–1 KB）：

| 尺寸 | 像素 | 模型侧 |
|---|---|---|
| 344×128、344×160 | 44,032、55,040 | 仍是 data URL，无 image part |
| 344×190、344×191 | 65,360、65,704 | 工具文本变成 `[image content will be provided separately]`，随后有 PNG image part |
| 344×228 及以上 | ≥78,432 | 同上 |

因此下限落在 `344×160` 与 `344×190` 之间（约 5.5 万到 6.5 万像素），不是文档里的 20 KB 文本帽。`344×920`（2445 字节，effort low）和带噪点的 `344×280`（289,343 字节，xhigh，会话 `01a1362a-1f08-7d21-bd50-8d43a10033cf`）都被读出 `K7NP4R`。

其它形式：

- 只有 image 块、以及 `resource.blob`（`probe_embedded`，同样是 344×120）也变成 data URL，没有视觉 part。
- `expose_image_base64 = false` 与默认相同。`true` 只是在文本里再包一层 `<mcp_image_base64 mime="image/png">`，小图仍然没有视觉 part。
- `read_file` 对 `/tmp/s01-run/small.png`（344×120）和 `pad.png`（344×920）都会在 `tool_result.images[]` 里附上图片。模型读出 `K7NP4R`（会话 `01a1362c-bd53-7fe0-a861-7a921a475815`，推理原文 “The image shows the characters K7NP4R.”）。这是小图的可用路径。

### 2. `elicitation/create` — 协议上是；headless 不会收集决定

真实 `grok -p` 与 ACP agent 的 `initialize`（协议 `2025-11-25`，client `grok-shell-s01` / `grok-shell-s01http` `1.0.52`）声明：

```json
{"extensions":{"io.modelcontextprotocol/ui":{"mimeTypes":["text/html;profile=mcp-app"]}},
 "elicitation":{"form":{"schemaValidation":true},"url":{}}}
```

`grok mcp doctor` 的 initialize 只有 `extensions`，没有 `elicitation`，不能拿来当证据。`probe_elicit` 在 headless 下 9 ms 收到 `{"action":"cancel"}`（会话 `01a13626-0c57-7f33-b0b6-1d730d2bb745`）。没有交互 TTY，没有点过 TUI 卡片。文档 `03-keyboard-shortcuts.md` 描述了 elicitation 卡片（`x.ai/mcp/elicit`），本 spike 没有打开 TUI。

### 3. HTTP / streamable HTTP，以及 ACP `session/new` — 是（`type:"http"`）

`grok mcp add --help` 的 `-t http` 说明为 “Connect to a remote server over streamable HTTP”，另有 `sse`。`GROK_HOME=/tmp/s01-home-http grok mcp doctor s01http --json`：`transport: http`，handshake `2025-11-25`，12 tools，`healthy: true`。随后 `s01http__probe_ping` 返回 `ping-ok`（会话 `01a13628-f5dd-7152-a99f-45b35ed91bac`）。请求带 `Accept: text/event-stream, application/json`、`mcp-session-id`，以及配置的 `x-s01-token`。

ACP：`grok agent --no-leader stdio` 的 `initialize` 给出 `mcpCapabilities: {http:true, sse:true}`。`session/new` 的 `mcpServers[]` 接受：

```json
{"type":"http","name":"s01http","url":"http://127.0.0.1:8766/mcp",
 "headers":[{"name":"X-S01-Token","value":"s01-test"}]}
```

会话 `01a1361f-fcae-7f23-8000-e52ac853a61a`。8766 上的服务器不在 config 里，仍收到 initialize、`notifications/initialized`、GET SSE 和 `tools/list`。连接前会先发 `server/discover`（`_meta` 协议 `2026-07-28`）；探针返回 method not found，ACP stderr 有 `failed to decode None: Invalid params`，随后正式 initialize 仍成功。

05 写的 `{transport:"http", ...}` 被拒：`Invalid params` / `data did not match any variant of untagged enum McpServer`。SSE 只看到能力位，没有单独握手。

### 4. `instructions` — 是

不在 `system_prompt.txt`。注入为 synthetic user：

`MCP server connected: - s01 (11 tools): S01_INSTRUCTION_TOKEN=kestrel-4401. ...`

同一会话里模型的最终行含 `TOKEN=kestrel-4401`（`01a13626-0c57-7f33-b0b6-1d730d2bb745`）。

### 5. turn 结束钩子 — 是（Stop，不是 Codex `notify`）

`$GROK_HOME/hooks/s01-turn.json` 的 `Stop` 在 headless 正常结束时触发。payload 摘录：`hookEventName: stop`，`reason: end_turn`，`lastAssistantMessage: ping-ok`，`permissionMode: bypassPermissions`（因为当时带了 `--always-approve`）。随后还有 `SessionEnd` `reason: shutdown` 和第二次 `Stop` `reason: shutdown`。`max_turns` 走 `StopCancelled` `reason: max_turns`，不是 `end_turn`。没有发现单一的 `notify` 配置项。`gcu turn-ended` 应安装 `Stop` 钩子并只处理 `reason == end_turn`。

### 6. 工具结果大小 — 文本默认 20_000 字节，只留前缀

`probe_size` 100 字节：全文保留，`STARTMARK` 与 `ENDMARK` 都在，末尾还拼着 `{"bytes":100}`。25_000 与 100_000 字节：模型侧都是 20,371 字符，通知原文是

`[MCP output truncated: showing first 19.5 KB of 97.7 KB. Full output written to: <session>/mcp/<call>.txt ...]`

前缀里有 `STARTMARK` 和偏移 15_000 的 `MIDMARK`，没有 `FARMARK` / `ENDMARK`。完整正文在 session `mcp/` 溢出文件里。这不是“掐掉中间、保留头尾”。溢出结果末尾的 `structuredContent` 会跟着尾巴一起离开模型视野。

图片：过了第 1 条的尺寸下限后，即使 PNG 只有 797 字节也会抽成 image part，而不是按 20 KB 文本帽截断。`targetBytes: 1500000` 生成 1,611,788 字节 PNG（344×1560），客户端改写成 `data:image/jpeg`（852,898 字节，344×1560）并附上。没有出现 `[large image removed]`。289 KB 的 PNG 仍以 `image/png` 附上。JPEG 重编码的可读性没有再测。

### 7. 工具调用并发 — 是

同一条 assistant 消息里的两个 `probe_wait`（各睡 2.5 s）：

`A` start `16:43:49.439Z`，end `+2501 ms`；`B` start `16:43:49.440Z`，end `+2502 ms`。

随后 6 个工具（structured、b64、elicit、三个 size）也在同一条消息里发出，服务器时间戳落在 11 ms 内。stdio 服务端必须能并发应答。

### 8. `structuredContent` — 是，但是拼进文本

模型看到的是一整段字符串，不是单独的 typed 通道：

`Tool completed. This sentence does not contain the secret. {"secret":"struct-qq8w","ok":true}`

模型复述了 `struct-qq8w`。见第 6 条：结果一旦被 20 KB 帽截断，挂在末尾的 JSON 会丢失。

### 9. annotations 是否影响权限提示 — headless 下否

`probe_ro`（`readOnlyHint: true`）、`probe_rw`（`destructiveHint: true`）、`probe_plain`（无 annotations）在同一次 headless 调用里都返回了 `ro-ok` / `rw-ok` / `plain-ok`，`permission_resolved` 全是 `allow`，`wait_ms: 0`。Stop 钩子记录的 `permissionMode` 是 `default`。

`--permission-mode dontAsk`（放在 `-p` 前、换新的 `--leader-socket`）以及把 `ui.permission_mode` 设成 `auto` 或 `dontAsk`，钩子仍是 `default`。唯一能改变钩子字段的是 `--always-approve` → `bypassPermissions`。配置表里的合法值是 `default / ask / auto / always-approve`，没有 `dontAsk`。没有打开 TUI，不能说明交互权限卡是否看 annotations。headless 下不能靠 `readOnlyHint` 做 v2 的策略。

### 10. grok-4.7 + xhigh 读图稳定性 — 小图否（0/3）

第 1 条的三轮 xhigh 都是 `max_turn_requests`，输出里没有 `K7NP4R`。抽成视觉 part 之后的那一次 xhigh（289 KB PNG）读出了 `K7NP4R`，但那不是这三轮的重复。

## 决定

v2 对 grok 1.0.52 的默认：

1. 截图仍走 MCP image content，但短边至少 256 像素（实测 344×160 失败、344×190 成功，256 是留余量）。窗口截图（03 默认长边 1568）自然满足。低于下限的裁剪要先放大，或改走下面的文件路径。单图继续按 03 的 1.5 MB 上限缩小；1.6 MB PNG 会被客户端转成 JPEG，细字号有风险。
2. 工具文本同时给出临时文件路径。提取失败或客户端更老时，模型用 `read_file`。`read_file` 对 344×120 已经读出字符。
3. ACP / 路线 B 注入 `{type:"http", name, url, headers:[{name,value}]}`。不要发送 05 字面量里的 `transport` 键。现有 `mcp_def_to_acp` 已经写成 `type`。自定义 header 可用。`server/discover` 失败可忽略。
4. `gcu install grok` 安装 `Stop` 钩子，只处理 `reason == end_turn`。空闲 45 s 仍是装不上钩子时的后备。
5. 用 initialize `instructions`（≤1200 字符）。客户端会把它放进 system-reminder。
6. `structuredContent` 可以继续作为精简事实来源，但要假设它被拼进文本，并且必须落在 20 KB 前缀内。长正文放文件路径，不要放在工具结果尾部。
7. elicitation 可以先试。headless 的 `action:cancel` 以及任何错误都走 helper 审批窗。不要把 headless / ACP 无 UI 会话的授权押在 elicitation 上。
8. 不把 `readOnlyHint` 当作 Grok 的权限边界。v2 自己的 Policy 决定敏感动作。服务端要允许同一 stdio/HTTP 会话上的并发 `tools/call`。

## 备选与为什么不选

- 只靠 MCP image、不设尺寸下限：344×120 的三轮 xhigh 已经失败，模型会把 data URL 当文本并耗尽 `--max-turns`。
- 永远只给文件路径、不附 image：`read_file` 已验证，但每张图多一轮工具调用。窗口大图本来能直接进视觉，没必要默认多这一轮。
- image 和文件路径每次都同时给：最不容易丢图，大图会在上下文里出现两份。只在提取没有发生时再 `read_file`。
- 用 `expose_image_base64` 或 resource blob 代替视觉：小图上两者都还是文本，模型读不出像素。
- 用 05 的 `transport:"http"`：ACP 直接拒绝该变体。

## 影响（对接口、测试、用户）

- 03 §4.4「默认附图」在 grok 1.0.52 上成立的条件是图片超过上述下限。未超过时，image content 不会变成视觉输入。坐标仍必须对应该截图像素；这一点不变。
- 03 §4.3 的 `structuredContent` 会到达模型，但是文本后缀，不是独立通道。评测器若解析 MCP 原文，不受影响；模型只能看见拼进去的 JSON。
- 03 §11.3 instructions 可用，不必改成只靠 skill 文件。
- 04 §7：附图默认保留，并加上尺寸下限和文件路径后备。elicitation 保持“失败则 helper”。turn 钩子用 `Stop`/`end_turn`，不是 Codex `notify`。文本上限按 20_000 字节前缀设计 `text_limit`。
- 05 §1 方案 1 成立，但线上 JSON 必须是 `type:"http"`。方案 2（stdio `gcu mcp --connect`）仍是 HTTP 不可用时的后备，不是 1.0.52 的必选项。SSE 未测，不作为默认。
- 用户可见：大窗口截图可以在工具结果后直接被看见。小裁剪若没放大，模型会空转去“解码 base64”，而不是看见图。JPEG 重编码可能让小字变糊；保持 PNG 小于约 289 KB 这一档（该档仍是 PNG）。

## 退出条件（什么情况下推翻本决定）

- 新的 Grok CLI 把任意大小的 MCP image / resource 都作为视觉 part 送达，且 `344×120` 这一档 xhigh 能稳定读出。那时可以去掉放大和文件路径后备。
- 新版本把 `--permission-mode dontAsk` 真正应用到 headless，并且 `readOnlyHint` 与 `destructiveHint` 产生不同的允许/拒绝。那时可以让 v2 策略与 annotations 对齐，而不是完全自管。
- ACP 开始接受 `transport:"http"`，或停止接受 `type:"http"`。注入代码要跟着改。
- 文本帽不再是 20 KB 前缀，或大 PNG 不再被转成 JPEG。`text_limit` 和截图字节上限要重测。
