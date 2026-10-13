# CU 通用有界运行时合同检查点

2026-10-13；`feat/computer-use-implementation`；active / partial。
本批是 R2 通用归档/树合同，不是维护版来源选定或整套 CU 发布。
App 继续 macOS 11+，仅 CU 受管浏览器要求 macOS 14+；没有改 App 的安装底线。
不提交、推送、提 PR 或发布，不晋升 compiled pins、四 source locks、seed 或用户 App144。

## 生产接线

`scripts/computer-use-runtime-limits.json` 是唯一编译/审计额度源，Rust 通过
`include_str!` 编入 Core，Node 审计直接读同一合同；manifest/lock 无权扩大额度。
它只限制资源，绝不替代精确 hash、版本、架构、布局与五个 Mac 链接合同。

| 合同 | 总条目（含目录/链接） | 文件/链接 | 单件 | 总树 | 压缩包 | 相对路径 | 压缩率 |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Chromium | 2048 | 1024 | 600 MiB | 1 GiB | 512 MiB | 320 字符 | 20 |
| Playwright | 4096 | 2000 | 8 MiB | 32 MiB | 16 MiB | 240 字符 | 20 |
| Node | 10000 | 8000 | 128 MiB | 512 MiB | 128 MiB | 240 字符 | 20 |

- 去掉 prepare 专属的 768/600 MiB 覆盖，ZIP、实际树、manifest 生成、RuntimeStore
  repair/resolve/diagnose/rollback、copy 和 bundle audit 都执行共同上限。
  文件型普通 App/MCP 组件没有被误当作 Chromium 树限制。
- 树根和空目录也计条目，链接计文件及目标原文字节，仍只接受 compiled Mac 五链接。
  ASCII 大小写别名、列表分隔符/越界路径、特殊文件和 reparse point 拒绝。
  有效树 listing 格式/digest 保持兼容；不靠删文件或省略目录刷绿。
- 文件计数/字节使用 checked arithmetic。hash 与复制改为分块读取，文件增长、缩短、
  原句柄大小/mtime 变化失败；不跟随源链接，不把单件大文件整体装进 Vec。
- ZIP 的选择/未选择文件及空目录都读到 CRC/精确长度校验终态。
  只有整包验证完成才替换目的目录；部分写入失败不发布旧目录的替代品。
- TAR raw pass 补计普通 iterator 隐藏的扩展头，单扩展头限 4096 bytes；所有压缩展开
  （包括元数据/零填充）有额外硬上限，末尾非零数据/额外 gzip member/CRC 错拒绝。
  Node 未选文件仍完整读取/计额，npm/corepack 链接从不提取或执行。
- 非法/超额 source lock 在 seed recovery 之前拒绝，保留原 seed/publication journal；
  超额 bundle 在 RuntimeStore 清除 interrupted staging 之前拒绝，保留 active pointer/证据。
  缺树错误保持 `missing_file`，不是泛化 IO 错误。

## 现有四来源容量复验

固定官方未品牌化 157/position1716663 四 ZIP 再次完整 CRC/树清点，均落在新额度内：

| 源 | 条目 | 文件/链接 | 树 bytes | 最大单件 bytes | 最大包内路径 |
| --- | ---: | ---: | ---: | ---: | ---: |
| Windows x64 | 258 | 258 | 859583753 | 368524800 | 53 |
| Linux x64 | 483 | 478 | 709988858 | 532813424 | 78 |
| Mac ARM | 683 | 352 | 382083296 | 270687952 | 193 |
| Mac Intel | 683 | 352 | 420425816 | 320288168 | 193 |

最大包 362195251 bytes，压缩率最高 2.828093。包内路径包含 archive root，而合同
路径相对 component root；上表更保守。归档内没有显式根目录时，磁盘树另计根目录和
实际父目录，不能把 ZIP 条目数直接当作磁盘条目数。只是容量适配，不是生产 prepare、
native browser 或通知通过。R1 四包 sample Credits 仍不允许发行。

## 本批验证与原失败

- Windows 最终完整 Core：session64148，1000 passed / 0 failed / 0 ignored，191.73s。
  CLI session30608，3 passed / 0 failed；最终 Core all-targets Clippy `-D warnings` exit0。
- Ubuntu22/glibc2.35 新冻结 `/work/core-runtime-bounds-06`：原 session8488最终exit0，
  本批1000 Core（278.99s）及3 CLI通过；all-targets strict（19.93s）和冻结输入SHA复验通过。
  Linux 实际 Node20/PW1.48 source import 回执 `passed`，`runtimePrepared:false`。
- Node 限额/audit/CLI/targets/before-build：35 passed / 0 failed / 0 skipped。
  定向 ESLint、22件 owned source LF/末尾换行/零 trailing whitespace/hash 检查通过。
- 首次 Windows full Core session55065 真实 exit101：993 passed / 1 failed。
  提前预检把缺失 bundle 树变成 IO 分类，修复分类后重跑；保留 `windows-core-first.log`，
  未改原断言。最初9项限额试跑有 unused Read warning，移除导入后严格检查通过。
- 首轮 Node 19/1 失败是 test fixture 在 seed-only 模式也放了 7-byte `Grok.exe`，
  实际总字节正确，修正 fixture 的布局后原断言通过。原结果在工具流，没有冒充本地 raw log。
  定向 ESLint 13个 no-undef 首败通过明确 Node globals imports 修复，未关闭规则。
- Windows共享 seed 前后445文件/440975863 bytes/digest
  `d002913fc9d6741c89ebdd57841f9ab3083cec0b49f8aa07a0d7bb3cba887696`完全相同。
  限额合同 SHA256 `ae4c9d4e4844f1bd0003c850f862dd75982e975df04f381088f226bee3112120`。

证据在 `.run/runtime-contract-20261013-01/` 和 `.run/runtime-contract-jammy-20261013-01/`。
首败、最终 logs、22件源码 SHA、四源容量、共享 seed 指纹与 Linux 冻结源清单全部保留。
最终 FINAL质量门禁exit0，App+Workbench13573/≥1000源码80未增长。cargo fmt、diff与
定向ESLint通过；不复记历史 App/前端/native browser 验收。原三份其它WIP的CRLF→LF
warning保留，未顺手改写 `.github/workflows/release.yml`、`.gitignore`、Linux配置。

## 清理

所有本批原编译/test owners已终态后，只删除两件准确旧incremental generations；
路径/时间/条目/大小、保留的新代次及再生命令在 `cache-retirement.json`，执行脚本为
`tools/computer-use-probe/retire-runtime-contract-cache.ps1`，它拒绝links/reparse、身份漂移或活跃owner。
逻辑603201547 bytes（约575.3MiB），H即时可用空间观察增477073408 bytes（约455.0MiB），
不把逻辑字节当实际回收，也不声称排他归因；hardlinks/压缩/并发写入可能不同。
可用原cargo test/clippy命令重新生成，原首败/最终logs未删除。
Linux06当前snapshot/target、所有其它不确定cache、current binary/依赖/seed/source与App/home
全部保留，不删除整workspace、target、node_modules或共享cache，不停止任何无关进程。

## 交付边界与下一步

当前产品仍 Node20/PW1.48/Chromium130，App144 没有重打；最后 CIM确认 PID59356/原EXE
与创建身份一致，用户 home/login/历史/未发送草稿未动，本批不声称视觉验收。
R2 通用合同已接，最终来源格式/布局/版本、五链接迁移、四锁/资源/NOTICE/hash/import/
架构及新完整 candidate prepare/check 仍开放，必须等 R1 来源/完整通知通过。
继续处理 R1 的56 yes metadata错误、153目录错误、4异常字段、477 raw引用的准确分类，
实际公共运行源/平台条件与源码提供义务；它们不是 binary 数量，不能为了计数归零采无关源。
R3两 Mac 原生和最终四架构、R4安装/签名/更新/修复/回滚/卸载、所有四surface/UI UX、
真人原模型/ACP/MCP/picker、同一最终候选12h/D00–D31范围全部保持。
