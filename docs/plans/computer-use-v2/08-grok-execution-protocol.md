# 08 · Grok 执行规程

> 写给执行本计划的 Grok（或任何 coding agent）。读完本文再动手。本文的规则优先于你的默认习惯；与仓库 `AGENTS.md` 冲突时以 `AGENTS.md` 为准并在 PR 中指出冲突。

---

## 1. 你的工作循环

```
1. 打开 STATUS.md，选第一张 status=todo 且所有依赖为 done 的卡（同阶段内按卡号顺序；P1/P2/P3 可按你所在平台选择）。
2. 在 STATUS.md 把该卡改为 in_progress（同一 PR 内改，不单独提交）。
3. 从最新 main 拉分支：cu2/<卡号>-<短名>，例如 cu2/P0-06-tree-pipeline。
4. 读卡 + 03 架构中被引用的小节 + 卡里提到的移植来源（git show archive/cu-v1-2026-10-13:<path>）。
5. 先写测试（至少写出用例名与断言骨架），再写实现。
6. 本地跑：cargo fmt / clippy -D warnings / test / deny / xtask check-deps golden budget（computer-use workspace）；涉及前端再跑 npm 的 lint/test。
7. 自审：对照卡的 DoD 与 07 的相关条目逐条打勾，写进 PR 描述。
8. 开 PR → CI 绿 → 请求 review → 合入后 STATUS.md 该卡改为 done（填 PR 链接）。
9. 回到 1。
```

一张卡做不完（超过估算 2 倍）：停下，在 STATUS.md 备注 `blocked: <一句原因>`，开一个 issue 描述阻塞点与已尝试方案，转去下一张独立的卡。**不要**合入半成品。

---

## 2. 硬性规则

1. **不合入 partial。** 卡的 DoD 未全部满足就不合入。未完成的纵向能力用 feature flag 关闭（`cfg(feature)` 或运行时开关），但代码本身必须完整、有测试。
2. **不写检查点文档。** 进度只写 STATUS.md 的一行（状态、PR、日期、一句话备注）。不新建 `docs/plans/*checkpoint*`、不写"本批……保持 active/partial"。
3. **不做环境记账。** 不在仓库里记录缓存清理、GiB 回收、PID、本机路径。
4. **证据 = 测试 + CI。** PR 描述引用测试名和 CI 链接，不写"实测通过"散文。需要人工验收的（视觉、真实 App），在 PR 中贴截图/录屏并说明机器与版本。
5. **不扩大范围。** 发现卡外问题：开 issue 或在 STATUS 的"发现"区追加一行，不顺手改。
6. **不 fork 上游依赖。** 不新增 `[patch.crates-io]`、不 vendor 第三方源码。需要时写 ADR。
7. **不随包携带第三方运行时**（浏览器、Node、Python、.NET）。
8. **不碰用户真实数据。** 测试只用夹具与系统自带 App；不登录任何账号；不操作 IM/邮件/支付。
9. **不改宪法。** 03 架构的接口变化需要 ADR + 维护者批准；不能在实现卡里悄悄改。
10. **遵守 AGENTS.md**：i18n（15 语言同 PR）、无 `window.confirm`、无原生 `<select>`、不增长 App.tsx/AppWorkbench.tsx、设置登记、功能三件套、提交后推送、分支卫生。
11. **一次只做一张卡。** 例外：同一平台的两张无依赖小卡（各 ≤1d）可在一个 PR，标题写两个卡号。
12. **失败首因保留。** CI 首次失败的原因写进 PR 评论（一句话 + 修复方式），不靠重跑刷绿。

---

## 3. 代码规范（computer-use workspace）

- Rust 2021，`rust-toolchain.toml` 固定版本；`#![deny(unsafe_op_in_unsafe_fn)]`；FFI 全部在 `ffi.rs`/`unsafe` 模块，每个 `unsafe` 块有 `// SAFETY:`。
- 错误：库 crate 用 `thiserror`，二进制用 `anyhow`；不吞错误；驱动错误必须区分"确定未执行"与"可能已执行"。
- 不 `unwrap()`/`expect()`（测试与 `const` 初始化除外）；不 `panic!` 跨 FFI 边界（`catch_unwind`）。
- 异步：tokio；阻塞的 OS 调用放专用线程或 `spawn_blocking`，均带超时。
- 单文件 ≤ 600 行（测试文件 ≤ 1000 行）；超出就拆模块。
- 公共 API 有文档注释，说明阻塞、超时、取消语义。
- 日志用 `tracing`，默认不记载荷。
- 测试命名：`<行为>_<条件>_<期望>`，如 `click_stale_index_returns_fresh_state`。
- 快照测试用 `insta`，快照文件随 PR 提交且在描述中说明审阅过。

前端（路线 B）遵循仓库既有规范（TypeScript strict、Vitest、组件位置、i18n）。

---

## 4. PR 规范

标题：`cu2(<卡号>): <动词开头的一句话>`，例如 `cu2(P0-06): add tree transform pipeline and text renderer`。

描述模板：

```markdown
## 卡
P0-06 树管线与渲染（docs/plans/computer-use-v2/06-roadmap.md#p0-06-树管线与渲染）

## 做了什么
- 3–6 条，面向 reviewer

## 接口
- 新增/变化的公开类型或函数（若改了 03 中定义的接口：链接 ADR）

## 测试
- 新增测试：`tree::transforms::merge_single_item_groups::*`（12）…
- CI：<链接>
- 人工验收（如有）：截图/录屏 + 机器/系统版本

## DoD 自查
- [x] 卡内 DoD 第 1 条 …
- [x] 通用 DoD（fmt/clippy/deny/check-deps/golden/budget）
- [x] 07 §x 相关条目

## 移植来源（如有）
archive/cu-v1-2026-10-13:src-tauri/src/computer_use/windows_uia.rs（取了 … 部分）

## 未做（明确不在本卡）
- …
```

提交信息末尾按仓库要求添加 attribution（如有）。

---

## 5. 何时停下来问维护者

- 需要证书、Apple 开发者账号、自托管 runner、付费 API 密钥。
- spike 结论与 03 架构的假设冲突（例如 Grok Build 不传图片给模型）。
- 需要修改 03 中的接口或宪法原则。
- 安全清单中任一项无法满足。
- 同一卡连续两次 CI 失败且根因不在本卡范围。

提问方式：在 STATUS.md 对应卡备注 `needs-decision: <问题>`，并在 PR/issue 中给出 2–3 个选项与推荐。

---

## 6. ADR 何时写、怎么写

位置：`computer-use/docs/adr/NNNN-<slug>.md`。写 ADR 的情形：spike 结论、改 03 接口、新增依赖（非 dev）且为非常见库、使用私有 API、放宽任何门禁、引入新平台路径。

模板（≤1 页）：

```markdown
# NNNN 标题
状态：提议 | 接受 | 取代(NNNN)
日期：
## 背景（事实 + 证据链接）
## 决定
## 备选与为什么不选
## 影响（对接口、测试、用户）
## 退出条件（什么情况下推翻本决定）
```

---

## 7. 汇报格式

每合入一个 PR，在 STATUS.md 更新一行即可。每完成一个阶段（P0、P1…），在 STATUS.md 的"阶段记录"区写 ≤5 行：
退出条件是否满足（逐条 是/否）、关键指标（如契约测试通过数、bench p50）、遗留 issue 链接。不写其他总结文档。

---

## 8. 常见陷阱（来自 v1 与调研）

| 陷阱 | 正确做法 |
|---|---|
| 让模型回传身份字段来做一致性校验 | 服务端持有身份；错误时附新状态 |
| 动作失败后按名字找"相似元素"重试 | 返回 stale + 新状态，让模型决定 |
| `unknown` 自动重试 | 永不；返回新状态 |
| 后台投递失败静默改前台 | 返回错误，前台需策略允许，结果写明 `delivery: foreground` |
| macOS 权限挂在终端上 | 路线 A 用 helper .app；路线 B 引擎由 App 启动，归属 App |
| 路线 B 里直接 `use gcu_core` 图省事 | src-tauri 只能用 gcu-protocol / gcu-host-protocol；需要新能力就加 Host 协议消息 |
| 每次调用冷启动 UIA/COM/AT-SPI | 常驻连接 + 专用线程（Windows worker 也是常驻） |
| 截图全屏 | 窗口截图 + 缩放 + 坐标映射 |
| 树节点上限过低 | 默认 1200，弹窗/焦点优先预算 |
| 为了"可控"打包浏览器 | 用系统浏览器的无障碍树 / App 内 WebView / 扩展 |
| 写大量散文检查点证明进展 | 合入 PR + 测试 + STATUS 一行 |
| 把 Windows 说成后台无打扰 | 能力表如实声明；前台时有守卫与接管 |
