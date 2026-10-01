# Computer Use：Windows 便携包完整运行时

日期：2026-09-30（本机 +08:00）；工作区 `H:\aicoding\grok-app-computer-use`。
分支 `feat/computer-use-implementation`；HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`。

**完整 goal 仍为 active / partial — not releasable。** 前一轮与本轮均为 progress。
本阶段修复真实发布脚本只复制主 EXE、遗漏 Computer Use runtime 的分发缺口；不是全部功能、安装态、跨平台或最终版本验收。

## 实现与发现

1. 新增 `scripts/stage-windows-portable.mjs`，按 `tauri.windows.conf.json` 的 7 条资源映射保留 `Grok.exe` 旁的 `resources/computer-use/seed`。包含私有 Node、Playwright、worker 全部模块、Chromium、锁文件和 manifest；不打包其他平台、缓存、profile、账号或私有构建锁元数据。
2. 仅接受显式 `grok-app.exe` / `Grok.exe`，校验 Windows x64 PE 形状与 seed target。资源/输出拒绝链接、大小写别名、越界和缺失；不猜测另一个 EXE、不删除已存在输出。Cargo 的主 EXE 可以是构建产物硬链接，但复制后的 EXE 必须独立，所有资源/输出继续单链接校验。
3. `portable-files.json` 记录精确路径、大小和 SHA-256；校验完整文件集合、必要组件和 manifest 不变。它是**复制完整性清单，不是发行方签名或可信版本证明**。
4. wrapper 复用现有 Rust runtime checker，先验证源，再验证 UUID 私有完整镜像，最后只复制声明的资源到交付目录。checker 合法持久化的 `.cu-mutation.lock` / `.owner.json` 留在私有验证目录；不删除共享 seed 的锁、不把机器路径装入包、不放宽精确文件集校验。hygiene 增加两种私有元数据拒绝项。
5. 新增 PowerShell ZIP 打包/解压回读。450 个文件全部回读后才发布本地 archive；拒绝覆盖，清理只针对本次 UUID 临时路径。实际 Windows PowerShell 5 暴露长路径清理问题，改用 Node 文件系统；`-sdel` 等形似 7-Zip 开关的目录名强制作为路径，反例确认不会删除源目录。
6. `--upload` 必须显式指定，不再根据环境 token 隐式上传；自定义 EXE/output 的本地探针不能上传。release workflow 已改为显式上传，CI 增加 Linux mapping / Windows ZIP fixture 测试；**没有执行远程 CI 或上传**。
7. 新增 App runtime 两项布局测试，确认生产 seed 发现函数能找到完整便携目录、EXE-only 目录不能通过。未修改生产 runtime 查找逻辑；布局测试不代表完整 App 启动。
8. 实际全流程首先被现有 checker 拦截：生成 seed 的 worker 已落后于当前源。核对本地三份缓存 archive 的锁定 SHA/大小后，离线 prepare 刷新 seed，再按原规则验证；没有跳过检查、改变依赖版本或发起新的下载。
9. `docs/BUILD.md` 和包内 README 说明完整目录、WebView2/Grok CLI 前提、数据目录，以及便携版手动整目录替换/回退。CI 默认 release 路径与 `build:win` 显式 target 路径不同，后者须传明确 EXE；不会自动选择任意 EXE。

## 已执行验证

证据根：`tools/computer-use-probe/.run/windows-portable-runtime-20260930/`。

| 项目 | 结果 | 边界 |
| --- | --- | --- |
| 打包/完整性/producer fixtures | **54/54，0 skipped** | portable 29、hygiene 12、已有 inventory producer 13；Windows Node 24.15.0；fixture EXE 均未执行 |
| App runtime 选定测试 | **4/4，1731 filtered** | 两个便携布局、既有 env guard、真实私有 seed repair/PATH decoy；不是全 App 测试 |
| App lib 编译 / Clippy | 通过，Clippy `-D warnings` | offline + locked；编译测试程序，没有重建或启动主 App |
| 实际完整 wrapper | exit 0 | `real-probe-v3`；现有 debug EXE + 当前真实 runtime，不是签名发行包 |
| 最终审查后的 ZIP 脚本 | exit 0，ZIP 与前项 **逐字节相同** | 修复开关式路径后重新对真实 450 文件打包/回读；没有把较早失败的 v2 当成功 |
| ZIP 解出 runtime 的真实浏览器测试 | **3 passed，7 filtered/skipped，0 failed** | 用解出的 Node 20.18.0、worker、Chromium；PATH 仅 System32，`where node` exit 1；只访问 loopback fixture |
| 浏览器测试范围 | 3 项实际执行 | 同时 managed profile 的 PID 所有权；click/set_value/type_text/select/key 身份矩阵；pause 排空真实 wait、resume 拒绝迟到旧请求 |
| runtime 测试后回读 / hygiene | 清单不变；450 文件，0 links，0 hits | 所有被启动的自有 worker/browser 已由原测试确认退出和清理；不操作用户浏览器 |
| 最后静态检查 | 通过 | Bash/PowerShell/Node syntax、Rust fmt、选定 tracked diff whitespace、CI/release YAML 结构 |
| 依赖/前轮回执 | SHA 不变 | Cargo.lock、前轮 receipt；不重新封存或改写前轮结果 |

原始日志：`packaging-tests-reviewed.log`、`app-runtime-tests.log`、`app-clippy.log`、`real-package-frozen.log`、`reviewed-real-archive.log`、`packaged-runtime-live.log`、`static-checks-final.json`、`final-source-artifact-review.json`。
真实浏览器用例输入为普通 ASCII 文本，**不是中文 IME、native Desktop、App WebView 或 Grok E4**。没有完整 10 项 browser suite 通过的结论。

## 产物与可复查证据

- 本地探针 ZIP：`real-probe-v3/Grok_0.2.33_x64-portable.zip` 与 `Grok_0.2.33_x64-portable-reviewed.zip`；各 **198,441,320 bytes**。
- 两者 SHA-256：`6feaa2955a666f534e4d020d2965c2809af7aedddda06a4abe0b1624424fc5b2`。
- **449 payload files + 1 integrity manifest = 450 archive files**；payload 515,290,881 bytes，整个目录 515,391,226 bytes。
- portable manifest SHA：`e8761e1cefa0a09be662f37eb8e383246f4aeb49a68a259d0e68f2b5c76f200a`。
- hygiene 路径/类型/大小/内容摘要：`38140e0e68315e42ac1230ecf71cc474624a095532aa6372a7aaa4c8e8e16982`。
- 刷新后的 seed manifest SHA：`001cf9617ce64b6cb6448c38a848533577a6116f33138bbc242e8e03e873b5ec`；seed tree SHA：`6fdd22b23b1798cde0aa8d2c82784e8abbcf0967741cbc5c355ffa8b93d102d8`。
- 前轮 receipt 保持 `d650ddc13f44206b025e676cf38f879628f8ce6eaa2eb28570f805436cbba315`；前轮 checkpoint/产物未作为本轮测试结果复用。
- `sealed-input-freeze.json` 是较早快照，最终审查明确记录 4 项后续变更（PS1 路径前缀、对应反例、release 注释、BUILD 说明）；**不能把较早快照说成最终零漂移**。最终 `receipt.json` 和只读 `audit.mjs` 绑定当前选定输入、本文/索引和原始日志，不代表整个产品冻结。
- `phase-start.json` 和 red/failure 日志保留历史；当前阶段结果以本文及最终 receipt 为准。真实完整 wrapper 成功后仅 archive 脚本上述修正及测试/文档变化；修正后 54 tests 和真实重打包已覆盖，原成功 ZIP 与重打包 ZIP hash 相同。

## 未完成项与继续方向

1. 此 ZIP 使用已有 **debug 主 App**，没有签名、最终 release build、App 实际启动或安装版界面验收；不能发布为最终版。
2. 便携手动整目录替换不等于自动更新完成：`Grok.exe` 与安装版 `grok-app.exe`/注册安装路径仍需完整 identity 和更新流程处理。禁止为了通过而绕过 signed inventory/journal。
3. Windows 安装完成/失败/回滚的权威对账、journal 退休、下轮更新恢复与签名安装态仍开放；不以永久 blocked 代替可用更新。下一阶段继续这一条主线。
4. 原始目标仍包含 Windows x64、macOS arm64/x64、Linux X11 **与原生 GNOME Wayland**；Desktop/managed/existing tabs/App WebView；App/ACP/MCP；中文/IME/clipboard/取消/恢复；签名安装更新回滚；窄窗/DPI/权限 UI；真实 Grok E4；**同一最终冻结候选 12h active soak**。本轮不缩小或替代这些要求。
5. 没有 commit/push/PR/tag/release、发行私钥读取、安装器执行、账号/系统权限变更或用户 App/浏览器进程终止。只运行自有 headless loopback fixtures 和构建验证。

**阶段判断：progress；完整目标仍 active，尚未满足 complete 或 blocked 审计。**
