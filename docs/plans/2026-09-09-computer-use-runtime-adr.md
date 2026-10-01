# ADR：Computer Use 发行 runtime

日期：2026-09-09  
阶段：S2.4  
工作区：`H:\aicoding\grok-app-computer-use`  
分支：feat/computer-use-implementation

## 决定

**预编译 App 只使用 App 私有目录里、经 manifest 校验的 runtime。禁止 `which node`、禁止 PATH 上的系统 Node、禁止下载 latest。**

MCP 会话注入继续跑现有 stdio JS 脚本（Host IPC 仍是 Rust），但解释器必须是安装包展开到 `{GROK_APP_HOME}/computer-use/runtime/<pack>/` 的私有 JS runtime。Playwright worker 与可选 driver 二进制走同一份 bounded manifest。

不在本阶段把 MCP 整段改写成 Rust sidecar。Playwright 仍需要 JS；先固定一份私有 runtime，比并行维护两套解释器更安全。若日后 MCP 改为纯 Rust stdio，manifest 删除 `js-runtime` 即可，不得回退到系统 Node。

## 布局

```
{app_data}/computer-use/runtime/
  current.json          # { "active": "pack-id", "previous": "pack-id"|null }
  packs/<pack-id>/
    manifest.json
    bin/node[.exe]
    mcp/server.mjs
    mcp/protocol.mjs
```

`manifest.json` 每个 component 含：id、version、arch、relpath、sha256。启动前校验 arch、文件存在和 sha256。不匹配则 fail closed，并提示修复/重装 App，而不是“请安装 Node.js”。

更新：写入 `packs/<new-id>/`（完整新目录）再切 `current.json`。失败保留 `previous`。回退只切回 previous，不写 PATH、不写全局目录。

## 不做

- 不把 Cua clone 当 runtime
- 不在用户机器执行 `which node` / `npm install`
- 不从网络拉 latest Node/Playwright
- 本 ADR 不完成 S7 Playwright 产品接线；只规定它必须出现在同一 manifest
