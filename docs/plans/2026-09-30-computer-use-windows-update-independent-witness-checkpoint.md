# Computer Use：Windows 更新独立进程见证

日期：2026-09-30（本机 +08:00）。工作区：`H:\aicoding\grok-app-computer-use`。
分支：`feat/computer-use-implementation`；HEAD：`30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。

**完整 goal 继续 active / partial — not releasable。** 上一阶段为 progress：原进程退出证据已持久化，但仅由重启的 App attach，不能覆盖 App 离线期间。本阶段实现并实测交接 ACK 后的独立句柄保管，不冒充完整安装结果对账。执行范围仍以 `2026-09-09-computer-use-grok-step-plan.md` 和执行状态账本为准，旧 P0–P9 文档只作历史。

## 生产改动

- 启动安装器前复制当前绑定 App 映像到受保护 journal 目录，严格核对源/副本 hash，文件名由原候选 UUID + ticket UUID 派生并保留文件/目录 lease。独立 helper 不把安装路径上的 App 映像长期映射，避免妨碍文件替换。
- worker 使用 `CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW`，不退化成 job-bound helper。预检失败发生在派发前；未声称实测所有宿主 job 配置。
- OS 接受安装器后，以 query/synchronize-only、不可继承的 duplicate 把**原 OS 进程句柄**交给 worker，不用 PID 重新打开替代交接。只授权一次的 ticket 绑定候选与 helper PID/创建时间。
- 私有 stdin 帧有长度/等待上限。worker 从自身规范 UUID 文件名确定 store，验证 DPAPI record、原 App 映像 hash、自身创建身份和安装器句柄身份；不从 WebView IPC 或任意路径选择安装器，**没有启动/重放安装器的能力**。
- 原 helper 发布 root/ticket-bound DPAPI `ready` 后，父 App 才能经此派发边界进入退出流程。helper 独立等待原 handle signal，原子发布不可替换的退出回执。新 App 的生产 `pending_install` 验证回执、拒绝冲突并写回原 journal。
- **LaunchIntent 与 Accepted 都能覆盖事务层的 retryable 错误。** 接受后缺句柄、helper 失败或 ACK 未确认，不能走普通 launch-error 路径重放。超时不当作死亡，不杀/替换 helper、不换候选。退出 0/259/失败码都只是进程证据；仍为同 nonce、Accepted、blocked，不叫已安装、不清 journal。
- Windows App main 在 Tauri 初始化前接入受限入口；保留 Linux keeper。未增加 App/AppWorkbench 状态、新 UI 文案或权限，15 locales 不变。

## 实测中修复

首次真实交接失败：`Stdin` 缓冲预读吸收帧正文，后续 `PeekNamedPipe` 看不到剩余数据，导致 ACK 超时。改为 owned stdin duplicate 上的**非缓冲 File 读取**，父写端持有到 ACK。相同测试、全库及实际 App 副本入口复测通过；初始失败日志保留，未将失败当通过。

## 当前冻结验证

证据：`tools/computer-use-probe/.run/windows-update-independent-witness-20260930/`。

| 项目 | 结果 | 范围 |
|---|---:|---|
| 默认 updater 原生库 | **47 passed，5 ignored** | ignored 为父测试实跑的受控进程入口，不是待实现能力豁免 |
| 独立 no-zip 原生库 | **46 passed，5 ignored** | artifact features 仅 `rustls-tls`，同跑所有适用持久化/见证测试 |
| 真实父进程死亡 | 通过 | 私有父进程 `process::exit(0)` 后，helper 仍保存夹具真实退出码 1603；不是只 drop 对象 |
| 独立 custody / pending | 通过 | 真实 0/259，无 App 轮询时保存；旧 owner 消失后生产 pending 对账，同候选仍 blocked |
| 异常与防重放 | 通过 | 精确终止测试 helper、缺句柄、伪造/超长引导、跨 ticket 密文、ready 冒充 exit、冲突/明文篡改均不授予新安装 |
| 真正 App debug EXE | 构建及入口实跑通过 | 新编译完整 App 的私有同字节副本；额外参数退出 65，实际 duplicate/ready/259 回执，helper 退出 0 |
| App lib | 编译 + **选定 38/38** | 1733 可用测试，只跑 updater 7 / shutdown 3 / browser owner 7 / MCP owner 21 |
| 前端 | **135/135，5 文件** | Hook、协议/helper、About/Sidebar、locale；退出 0 不触发安装/恢复 |
| 严格 Clippy | 两套无 warning | 默认 App+vendor lib/tests 与单独 no-zip vendor lib/tests，检查日志不只看退出码 |
| TS / 8 文件 ESLint / Rust 格式 / diff | 5 项通过 | 不冒充安装版/完整 UI 原生验收 |
| 选定源 | **78 项零漂移** | 包括本次 App main、vendor 和 updater 直接依赖，不是最终全产品冻结 |
| 来源、权限与依赖 | 通过 | 19 pinned 原件、JS API bundle 不改；生成 ACL 仍仅本地 main/session；App 673 包 lock hash 不变 |

no-zip consumer 为 420 包，其中 419 个依赖的版本/source/checksum 均与 App 锁一致，唯一新增的是本地 consumer。Cargo 全程 offline + locked；新增的 `Win32_System_Pipes` 是既有 windows-sys feature，不引入新版本。

生产副本探针在新建私有目录生成仅供观察的测试 journal，未证明更新签名校验、文件安装或回滚。1603/259 是私有测试 EXE 的真实退出码，**不是 MSI/NSIS 安装验收**。所有清理只使用自己的 exact Child/Process 对象，不按名称找/杀进程；私有副本结束后删除，未替换或终止用户 App。

首个局部快照未覆盖 main，保留为 `source-freeze-before-entrypoint-scope.json`；最终明确扩展为 78 项。冻结后源码无改动，最终全部检查对应当前源。新 receipt 封存源、文档、日志/来源审计并独立复核；前一 receipt/检查点不回写。

## 尚未完成与下一依赖

1. **ACK 前崩溃窗口仍开放。** 派发、OS 接受、Accepted 写入、句柄转移、ready 不是原子事务；只证明 ACK 后连续 custody，没有完成可持续 dispatcher。helper 死亡/断电也不能靠 PID 消失推断结果。
2. **权威安装结果未闭环。** installer delegation、签名候选对应实际安装文件、完成/失败/回滚 receipt、最终对账、journal 退休和下一轮更新释放仍阻断发布。安全 blocked 是临时保护，不能只比较版本或把 exit 0 当成功来解除它。
3. **真实签名安装态未验收。** NSIS/MSI/UAC、完成重启、修复/回滚/重装、安装版 WebView；helper 的发布依赖/签名/job 环境需要安装版实测。debug 副本能运行不代表上述全部成立。
4. **其余生命周期开放。** 半清理服务恢复、普通 Quit、native input/IME/clipboard、journal/helper orphan 的身份安全 GC。DPAPI 不防同用户/admin 任意代码或恶意历史回滚，write-through 不证明所有断电行为。
5. **完整原始范围不变。** Windows x64、macOS arm64/x64、Linux X11 与 native GNOME Wayland；Desktop/managed browser/existing tabs/App WebView；App/ACP/MCP；完整输入/取消/恢复；签名交付/更新/回滚；原生窄窗/缩放/权限 UI；真实 Grok E4；**同一最终冻结候选 12h active soak**。

下一步完成可持续派发/权威安装结果收敛，再证明 journal 退休与真实签名安装；不得退回仅 PID 轮询、仅版本号比较或自动重放制造通过。
未提交/推送/发版、改账号、操作用户剪贴板/权限或运行真实安装器，没有开始新 soak，没有改变 goal 为 complete/blocked/paused。

前一阶段：`2026-09-30-computer-use-windows-update-process-evidence-checkpoint.md`。
前一 receipt SHA-256：`2214aa19f0fe76e9817a0c17bc239760e3bac803574f79185ed4a26d0e62b4d9`。
