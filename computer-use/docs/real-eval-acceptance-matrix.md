# real-eval 首个平台验收矩阵（ADR 0017 S7）

> 状态：**草案，真实运行仍停止。** 本文只汇总已合入的离线实现与构建机证据，列出真实运行前必须补齐的原生证据、环境回执和所有者操作。它不批准任何真实运行，也不把 fake 通过当成原生保证。
> 依据：[ADR 0017](adr/0017-real-eval-fixture-scope.md)（§3 审批与 Stop、§4 环境契约、§5 分卡及 macOS/Windows 安全声明）。已合入：S1（#168）、S2（#175）、S3（#172）、S5（#177）、S6（#180）、S9（#181）、S10（#182）。**S4（专用 helper 与连接装配、已交付缓存回收）待合入**，下表凡依赖 S4 的项一律记 `pending`。

## 1. 判定规则

- 每一行只能是：`offline`（已合入、离线 fake 测试与三平台构建机/CI 通过）、`native-verified`（在符合 §4 的一次性环境里有原生证据，附回执）、`not_run`（缺环境或缺设备）、`pending`（依赖未合入的卡）、`closed`（scoped 模式按安全声明关闭，报告为不可用）。
- `closed` 不是失败：依赖它的任务在 scoped 下记 `not_run`，不计入通过率，也不回退到非 scoped 或整桌面。
- 只有 `native-verified` 能支撑真实运行；`offline` 只说明逻辑与离线门禁成立。目前**没有任何一行是 `native-verified`**。

## 2. 共同前提（三平台一致）

| 项 | 状态 | 说明 |
| --- | --- | --- |
| scope 封闭包装与单一原子提交点（S1） | offline | 所有发布路径经 `ScopeLease::commit_if_current`；Linux 1936、Windows 473、macOS 473 |
| 私有 bootstrap、引擎状态机与数据 relay（S3） | offline | scope 权威只来自继承的私有控制通道；Windows 私有管道运行时 **not_run**（Windows 下 scoped 引擎 fail-closed） |
| 审批拆分（S5） | offline | `auto-allow-*` 只替代范围内 app grant；敏感/前台确认默认拒绝；不写 Always |
| 一次性环境 provider 契约（S6） | offline | §4 五行 `checked/attested/unknown`；任何进程只能凭 `AdmittedLaunch` 启动；**当前没有接受型 provider，所有真实路径一律拒绝** |
| 专用 helper、最终 native ACK、已交付缓存随 lease 回收（S4） | pending | 真实运行在 S4 合入并完成原生验收前不可能开启 |
| Stop：热键/钩子、100 ms 锁存、500 ms 释放、真实用户恢复 | not_run | 常量未改；离线测试覆盖锁存与释放顺序，真实热键与钩子摘除需原生证据 |

## 3. 平台矩阵

硬保证在三平台相同：**进程实例**（pid + 启动/创建时间 + boot 会话标识），在每个出口复验，缺一即固定 `ScopeDenied`。窗口级为尽力而为、失败即关闭。

| 能力（scoped 模式） | macOS（S2，S4 pending） | Windows（S9） | Linux（S10） |
| --- | --- | --- | --- |
| 进程实例证明 | offline：`p_starttime` + `kern.bootsessionuuid` | offline：`GetProcessTimes` + boot-session GUID | offline：`/proc` start-time + boot_id |
| 应用/窗口发现 | offline：窗口表以 CGWindowID 为 id，连续性 token 证明同一对象 | offline：PID + 创建时间 + HWND + owner | offline：唯一总线连接核验后才读名；名称读取绑定当前 lease epoch；窗口列表只用已验证证据 |
| 无障碍树读取 | offline（AX 元素绑定原进程） | offline（UIA 每页先核对进程实例） | **closed**（AT-SPI 无窗口 incarnation 与逐节点 owner 证据） |
| 截图 | offline（核心层复核，S1/S2）；遮挡/他窗像素 not_run | offline（scoped 下证明失败即拒绝）；遮挡 not_run | **closed**（live X11 无 incarnation；Wayland 无对端凭据） |
| 定向投递（不经前台） | offline：pid 定向 CG 事件出口前复验 | offline：定向窗口消息与 UIA pattern，作业内复验接收者实例 | **closed** |
| 前台全局输入 | **closed**（全局 HID、整进程激活） | **closed**（SendInput 指针/键盘，含接近与拖拽采样） | **closed** |
| 剪贴板 | **closed**（随全局 HID 关闭） | **closed**（经前台按键粘贴与全部 scoped 暂存） | **closed** |
| 应用级写入 | **closed**（应用元素级 AX 写，含观察期开关） | 仅 UIA Value/Range 等可证明 pattern | **closed** |
| doctor / capabilities | offline：关闭项标为不可用 | offline | offline：scoped doctor 不做同步探测；缺平台绑定即拒绝 |
| 构建机证据 | Linux 2029、macOS 599 | Linux 2080/393/445、Windows 786 + pointer_gate 8/8、macOS 772 | Linux 2112/393/445、Windows 527、macOS 559 |

**诚实结论：** Linux scoped 目前几乎没有可用路线（真实 scoped 会话列不出窗口），只能作为“缺证据即关闭”的正确性样本；macOS 依赖 S4 才能装配真实连接；Windows 的离线面最完整，但私有管道运行时与原生证据仍 `not_run`。首个平台按 ADR 0017 推荐仍是 macOS 一次性访客机，开启前须 S4 合入。

## 4. 已接受残余与 OS 限制

这些不是待修缺陷，而是 OS 原语缺失导致的间隙，由“一次性、单用途访客机”兜底；复发或平台提供原语时重开。

- **macOS（所有者 2026-10-30）**：A1 pid 定向事件不能把接收者绑定到准入窗口；A2 两次相同读取不是原子观察；A3 AX 写/文本追加在检查与写之间元素可能被 reparent；A4 `AXRaise` 按 pid/窗口号重新解析。
- **Windows（所有者 2026-10-31）**：同进程窗口在最后一次检查后被替换或 HWND 复用；同进程 UIA 元素在进程复验后、缓存返回前 reparent；最后一次系统调用到投递的间隙。
- **Linux**：`_NET_WM_PID` 由客户端自报，不作身份；X11/Wayland 窗口来源不可证，故关闭对应路线。
- **共同**：夹具自身漏洞、被允许窗口触发的文件/网络副作用、模型 endpoint 本身作为泄漏通道，均不由 scope 阻止（ADR 0017 §6）。

## 5. 明确不支持（scoped）

前台全局输入（三平台）、剪贴板（三平台）、整进程激活（macOS）、应用级 AX 写（macOS）、ApplicationFrameHost 等无法证明接收者的窗口（Windows）、AT-SPI 树与语义动作（Linux）、live X11 与 Wayland（Linux）、浏览器 DOM 共享通道（scoped 下不装配）、模型启动应用（任何平台；夹具重启与新窗口只由可信 harness 登记）。

## 6. 环境回执模板（每个 attempt 一份，结束后归档）

每项填 `checked`（harness 机检，附摘要）/`attested`（provider 或所有者证明，附签名或日期）/`unknown`。关键项 `unknown` 即不产生 pass。不记录任何令牌、密码、个人路径或网络细节。

```text
run_id / attempt:
provider:                         # S6 provider 名称与版本
guest image id + digest:          # 基础镜像来源与摘要
guest OS build:
boot / desktop / UID:             # 本次新建，未复用
disposable user:                  # 无个人应用、会话与凭据
host integration:                 # 共享目录、剪贴板同步、拖放、设备/agent 转发：全部关闭
egress policy digest:             # 仅模型 endpoint；LAN/metadata/公网/DNS/IPv6/QUIC 拒绝计数
model endpoint:                   # 精确地址，由网关解析
eval credential:                  # 类型与到期时间；不打印内容
permissions granted:              # 授权给哪个 helper 身份（路径 + 内容哈希）
helper identity:                  # 路径 + 内容哈希；未签名（发布保持无签名）
fixture instance:                 # pid + 启动时间 + boot 标识，与 scope ACK 一致
scope ACK:                        # run id + generation
Stop check:                       # 热键触发、锁存、释放时限
cleanup receipt:                  # 进程树、临时目录、访客机回收
```

## 7. 真实运行前所有者需要做的事

仅限以下几项，其余由 harness/provider 机检：

1. 在一次性访客机内，为回执中列出的 helper 身份点击授予“辅助功能”和“屏幕录制”权限（macOS）。
2. 在访客机内完成一次性评测凭据的登录链接（设备码），凭据只存在于该访客机，随回收销毁。
3. 若 Stop 被锁存，在访客机内用真实用户操作恢复；harness 不能自动恢复。
4. 每次真实运行前重新给出同意；本文与任何回执都不构成长期授权。

## 8. 开启真实运行的门槛

S4 合入并通过评审 → 至少一个接受型 S6 provider（非 fake）→ 在一次性访客机里先做**无模型**的确定性原生验收，把第 3 节对应行改为 `native-verified` 并附回执 → 所有者审阅 → 再安排真实 LLM 评测。任一环节缺失即保持停止。
