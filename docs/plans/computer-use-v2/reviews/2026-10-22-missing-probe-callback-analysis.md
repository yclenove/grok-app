# Windows 拖拽期间探针鼠标回调缺失：根因假设与可证伪实验

日期：2026-10-22。任务：RCA missing probe callback。交付性质：只读源码分析与实验设计；本报告没有执行 Windows 实验，没有修改实现、测试、Stop 常量或 STATUS。

## 1. 结论与证据边界

**目前不能认定是 Windows 丢掉了 LL 回调。可以认定的是：被 SendInput 接受的某次探针，没有成为 Host 当前尝试的鼠标证明。** 需要先区分四层：SendInput 接受、进入原始 hook ABI、通过 Rust 上下文/标签过滤、成为双钩子证明。现有 `complete=true, callback=false` 只直接描述最后一层的输入状态。

优先调查两个方向：本地回调接收/重装生命周期，以及拖拽状态下绝对同位置输入的处理。前者可用少量诊断插桩快速排查；后者需要在独立、无产品证明状态机的最小 Win32 夹具上复现。两者目前都只是候选。尤其不能把“窗口端 WM_MOUSEMOVE 合并”直接当成“WH_MOUSE_LL 未被调用”的解释。

一次重试后的 400 次拖拽零锁存，证明缓解有效，不能证明第一次输入为什么缺失。24/1080 是**动作窗口**发生故障的比例，不是每个探针的丢失率；400 次零锁存也不是零次原始探针缺失。不能据此预先设定每个探针有 2% 的独立丢失概率。

### 1.1 本次核对的仓库证据

- [真机记录](../../../../computer-use/crates/gcu-driver-windows/diagnostics.md)，2026-10-21 指针回拉、2026-10-22 拖拽缺陷及后来重试三节；[指针闸 ADR 增补](../../../../computer-use/docs/adr/0013-addendum-cross-process-pointer-gate.md)。认可题目给出的 24/1080、400 次零锁存、契约表 20/9/1/0，未将其当作本次运行结果。
- `src/input/ffi/safety/mouse.rs`：物理光标读取、发送线程 PMv2 DPI 上下文、绝对移动标志。
- `src/input/hook_probe.rs`：`normalize`（250 行）、`dispatch_inner`（284 行）、`MouseProof`（345 行起）。标记范围从 `0x47435532` 开始；第一次请求实际先递增。`last_seen` 仅在 `MouseProof::callback` 识别到已发标签后更新，**不是 ABI 入口日志**。其中 `flags=1` 可能属于上一个标签，不能归给缺失的标签。
- `src/input/ffi/safety/probe.rs`：动作中一包 mouse + F24 up（373 行起）；键盘和鼠标要求同一尝试标签。源码关于“SendInput 返回前交付两个回调”的注释是实现假设，不能升格为 Win32 文档保证。
- `src/input/ffi/safety.rs`：两个 ABI 回调均丢弃 `catch_unwind` 结果（133、147 行）；`callback` 在 `try_borrow_mut` 失败时无记录返回（159 行）；重装先安装新句柄，再 Drop 旧句柄（215 行附近）。`watchdog.rs` 的 `REINSTALL_INTERVAL` 是 1s。
- `tests/contract/live.rs`：`live_click_probe`、`live_drag_stress`；`unsafe_click_probe.rs::HookLog` 的鼠标日志上限 600 条，回调内持 Mutex 并格式化 String，安装失败被 `.ok()` 隐去；`KeyLog` 上限 800 条且不保留标签/时间。`START` 是跨 HookLog 实例的 OnceLock。原样复用无法可靠证明高频长跑中的逐标签缺失。
- `tests/contract/unsafe_mouse_probe.rs` 复用生产的坐标构造，但自身只有诊断钩子；空闲矩阵不是拖拽状态覆盖。它按路径导入 `hook_probe.rs` 后处于 `cfg(test)`：**其中闸会走测试线程本地 Mutex，不能当作真实跨进程命名闸**。实验只复用其 normalize/构造公式；真实闸结论必须来自正常链接的产品库，或明确实现相同命名闸的诊断进程。

### 1.2 文档保证与推测

以下是对公开 Microsoft 文档的离线知识核对，链接供后续复核；本次没有联网检索，也没有查询 win-test 的注册表或 OS 配置。

| 编号 | 可依赖的文档语义 | 不能由此推出的结论 |
| --- | --- | --- |
| D1 | [SendInput](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput) 返回成功插入的记录数；同一次调用的输入按序插入，不被其他 SendInput/硬件输入插入其中；受 UIPI 约束，错误信息不能可靠指明 UIPI。 | 返回 2 不保证两个应用消息、两个指定 hook 回调或回调一定在函数返回前发生。 |
| D2 | [MOUSEINPUT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-mouseinput) 定义 0..65535 绝对坐标、VIRTUALDESK 和 `MOVE_NOCOALESCE`；后者针对 WM_MOUSEMOVE 不合并。 | NOCOALESCE 不是“每个输入必有一个带标签的 LL 回调”的契约；文档未明确同位置、按键按住时的逐条交付。 |
| D3 | [LowLevelMouseProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelmouseproc) 在安装线程上下文执行，通过向该线程发送消息实现，线程必须泵消息；超出 LowLevelHooksTimeout 会被跳过，Windows 7 起可能静默移除；Windows 10 1709 起有效超时最大 1000ms。键盘有相应 [LowLevelKeyboardProc](https://learn.microsoft.com/en-us/windows/win32/winmsg/lowlevelkeyboardproc) 语义。 | 1000ms 是该版本上的上限，不是已测得的本机设置；泵仍活着不证明 hook 仍在。没有可靠“是否已被静默移除”的查询 API。 |
| D4 | [Hook 链](https://learn.microsoft.com/en-us/windows/win32/winmsg/about-hooks) 中较新的全局 hook 位于链前；不调用 CallNextHookEx 或返回非零可能阻断后续交付。 | 两个进程的 hook 是同一条链上的节点，不是两个互相独立的硬件观测器；都沉默不能单独证明内核丢事件。 |
| D5 | [SetCapture](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setcapture) 改变鼠标消息接收窗口，通常用于按住按钮后的拖拽。 | 窗口的捕获/拖拽循环按文档本身不提供跳过全局 LL hook 的权限；是否触发更早的优化是推测。 |
| D6 | [GetPhysicalCursorPos](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-getphysicalcursorpos) 提供物理坐标；[MSLLHOOKSTRUCT](https://learn.microsoft.com/en-us/windows/win32/api/winuser/ns-winuser-msllhookstruct) 定义点位、注入标志、时间和 extraInfo。 | 物理读数相同不证明中间没有移动；文档没有保证本仓库的整数归一化反算式，也不能把不同 DPI 上下文的 GetCursorPos 直接相减。 |

下文“前 LL 去重”“捕获状态优化”“F24 导致批处理边界”“User32 调用重入造成借用冲突”等均是**待证伪推测**，不是 Microsoft 已确认缺陷。

## 2. 原因假设清单

“否定”只针对表中明确的机制；未复现和零样本均不能否定一个任意低概率的机制。

| 假设 | 机制、与事实的吻合度 | 已知事实能否否定 | 对应实验 |
| --- | --- | --- | --- |
| H1 同位置绝对移动在按键/捕获状态下被前 LL 优化 | Windows 可能识别零净位移，在左键按住、capture 或拖拽内部状态转换时省略移动记录/回调。空闲成功、拖拽偶发、同类型重试有效与状态相关优化相容。**无公开逐事件保证，也无已确认的这项优化。** | 不能；空闲矩阵只否定“同位置绝对移动一律没有回调”。若 Host 原始入口看见该标签，则否定该次故障由此造成。 | E0、E2、E3 |
| H2 合并/去重 | 下游消息队列合并 WM_MOUSEMOVE 可使应用少收到消息；另一种推测是输入进入 LL 链前已被合并/去重，标记可能由后一移动覆盖，NOCOALESCE 未必约束该层。20ms 间隔无效削弱“仅爆发过密”版本，但不排除状态相关合并。 | **纯应用端 WM_MOUSEMOVE 合并不能直接导致已经更早执行的 LL 回调消失，该版本否定。** 前 LL 去重仍未否定；必须排除 hook 链截断。 | E2、E3 |
| H3 LowLevelHooksTimeout / 静默摘钩 | 本地工作、链后 hook、线程调度等可能拖延回调处理，引起跳过/摘除。随后每秒重装恢复，表面成为短暂缺失。pump_poll_gap 主要衡量 poll 起点，不等于每次 ABI 至 CallNext 返回的耗时。 | 不能完全否定；已知无长停顿、发送耗时短使长期停摆版本很弱。若同一安装代在故障后持续收到新标签且无重装，可否定“该代已永久摘除”，仍不排除一次跳过。 | E0、E4 |
| H4 每秒重装与在途交付交错 | 安装新 hook、旧 hook 仍在、旧句柄 Drop、在途 LL 消息可交错；重装还会改变与测试 hook 的链序。静态 callback 和共享 CONTEXT 本身不能标识来自哪一安装。可能发生重复、延迟或上下文重入。 | 不能；先装后卸排除显式主动空窗，**不证明**没有在途交错。源码没有在重装时清空 MouseProof；“重装必清证明”这一具体说法否定。 | E0、E4、E5 |
| H5 CONTEXT RefCell 借用冲突吞回调 | ABI 已进入，`try_borrow_mut()` 返回 Err 后直接跳过；不 panic，故 catch_unwind 也无感。Sender::poll 持借用时调用诊断/状态方法，或 Win32 同线程重入是候选触发源。源码并未显示借用期间必经 PeekMessage/SendInput，不能声称已找到触发点；跨线程本身也不可能冲突同一个 TLS RefCell。 | 不能；会精确留下旧 last_seen、complete=true、无泵停顿。若原始入口没有标签则不是该次原因；入口有而 borrow_fail=0 且同次正常完成则否定。 | E0、E1 |
| H6 catch_unwind / 标签、flags、generation 接收错误 | panic 被丢弃、CONTEXT 为 None、当前标签已推进、flags 被剥离、旧 generation 的 report 被忽略，都可导致“已进入而未证明”。重试令 active_tag 提前改变，可能把迟到旧回调变成仅诊断事件。`last_seen` 只记录已识别标签，flags=1 是历史值。 | 不能统统否定。但**同一个缺失标签若只是 flags=0**，正常进入 `MouseProof::callback` 仍会更新 last_seen，所以无法单独解释“last_seen 始终是旧标签”；已进入的正确标签只缺键盘，也不能单独解释 mouse callback=false。 | E0、E1、E7 |
| H7 坐标归一化、DPI、裁剪或异步位置竞态 | PMv2/物理读取虽正确，虚拟桌面变更、clip、边界整数映射或前一移动未完全落地，可能使“同位置”实际不同；也可能短暂移走再回来，绕过 before==after。位置变化通常应得到移动回调，故它本身并不解释为何标记消失，需与 H1/H2/H6 联合。 | **稳定 DPI 虚拟化错误作为唯一原因很弱**：生产进入 PMv2 且读物理点、异常不是 cursor changed、空闲矩阵正常；可见最终位移版本被这些失败状态否定。瞬态、显示变更和 clip 组合未否定。旧的 control_move 使用 65535/(size-1)，不等于当前 normalize 的像素区间中心公式，不能当作逐点等价证明。 | E2、E6 |
| H8 NOCOALESCE 与 SetCapture / 拖拽循环交互 | NOCOALESCE 明确改变下游消息数量，可能间接改变捕获、命中测试、WM_SETCURSOR、UI 线程负载，或触发更早的未文档化路径；自带 Win32 简单 capture、模态拖拽与 egui/winit 的行为可能不同。 | 不能；单纯“应用拿走了 LL 事件”不成立，但间接状态/负载机制仍可能。必须将按钮、capture、消息消费频率分别控制，不能只对比空闲和完整拖拽。 | E2、E3 |
| H9 mouse + F24 up 同包/顺序 | 混合 INPUT 类型可能暴露批处理边界；键盘事件可能与鼠标迟到、应用拖拽状态或其他过滤器相互作用。mouse-first 的插入顺序由 D1 保证，两个 hook 的可观测完成顺序和产品配对并非同一保证。 | 不能；原故障未记录对应 F24 标签。**“另一进程输入插进这一个 SendInput 的两条记录之间”与 D1 冲突，否定。** 两次调用的变体不再有同包原子性，须单独报告。 | E0、E7 |
| H10 LL hook 链截断/另一个测试 hook 干扰 | 链前第三方 hook 可跳过 CallNext；测试 HookLog 自己持锁、分配和格式化可改变时序。它正常代码确实总是 CallNext，因此不能无证据指控它主动吞输入；安装先后及每秒重装会改变故障被哪个观察者看见。 | 不能；另一 hook 正常时看到所有探针不代表在缺失那一次也看到了。两者是链式观察，须同一标签对齐；若不挂测试 hook 仍复现，否定“测试 hook 是必要条件”。 | E0、E5 |
| H11 迟到/异步完成被误判永久缺失 | SendInput 接受、hook 调用、光标生效不是应混为一谈的时间点。未观察足够长或重试换标签后，只看当前 MouseProof 会忽略迟到。真机的 100ms 内无回调已经足以安全失效；这与“以后任何时刻也不会到”不同。 | 不能用计时正常完全否定；题目报告的“始终没出现”在既有观察窗口内成立。独立原始日志完整观察发送后 1s 仍无该标签，可否定 ≤1s 迟到版本；再长的理论延迟不作为延长 Stop 的理由。 | E0、E7 |
| H12 UIPI、桌面/会话切换、外部输入过滤 | 前景完整性变化、非 Default 输入桌面、远程接入/输入设备或安全软件过滤可能形成选择性交付。普通 UIPI 阻止注入通常表现为 SendInput 未插入，不能只凭 GetLastError=0 认定没有 UIPI。 | **静态完整性不匹配/普通 UIPI 拒绝是唯一解释**与全包 2/2、同标准用户夹具前后成功不符，优先排除。瞬时环境变化、插入后过滤仍未否定，但没有直接证据；不应通过提权/关安全软件验证。 | E0、E8 |
| H13 观测/归因伪影 | HookLog 日志封顶、安装失败未报错、日志时间原点不一致、未按进程/标签/代区分，可能使外部观察“没看见”。它不能解释产品本身锁存，但会把本地证明接收问题误归因于 Windows。 | 未否定；现有 cap 是确定的工具限制，不是已证明此次日志丢失。借用成功后记录的 last_seen 也不能代替入口观测。 | E0、E5 |

已由真机记录排除的主因不重测：长时间等闸、泵线程长停摆、探针周期过长、发送作业不返回、20ms 以下爆发节奏。E0 仍保留这些元数据作为分层关联，不关闭指针闸，也不回退 #155/#156。

## 3. 实验公共约定与 Rust 草稿

所有实验为**后续实现建议**，本任务没有把草稿写入源码。建议新增 `tests/contract/unsafe_probe_rca.rs`（Win32 FFI）和 `probe_rca.rs`（分析），由 `live.rs` 声明模块。测试统一 `#[ignore]`、显式 `GCU_DESKTOP=1`、单线程运行；原始实验的输入只指向自建空白 Win32 夹具，不包含账号/系统设置/IM 操作。真正的产品复现仍走 `WindowsDriver::monitored`，保持生产命名闸、100ms 租约和一次重试原样。

### 3.1 必须先补齐的观测

每个发送记录：run、case、动作序号、sender pid/tid、attempt/generation/tag、INPUT 的所有字段、QPC 发送前后、返回数、立即 LastError、模式、重试序号、指针闸进入/退出、发送前后物理点/归一化值/桌面尺寸/DPI context/clip、夹具 capture/前景窗口、按钮状态。DPI/GUI 查询在 sender 或专门采样线程，**不在 LL 回调里查询**。LL 结构没有 sender pid，必须从发送账本关联，不能将观察者 pid 当作注入来源；每轮只运行一个产品守卫，诊断输入使用独立标签域，检测到标签冲突即判该轮无效。

每个回调记录：observer、pid/tid、安装 epoch、hook kind、code、wParam、tag、flags、pt、系统 time、入口/本地处理结束/CallNext 返回 QPC、borrow_ok/none/fail/panic、active_tag 与 generation、是否进入 MouseProof、是否被接受。键盘保留测试标签的 vk/scan/flags/tag；不记录任何真实用户键内容，遇到非测试输入只记“外部输入”并终止该 case。真实 Host 的静态 callback 只能记录“当前安装 epoch 与是否正在重装”，不能据此认定调用来自新句柄；只有 E4 的诊断 A/B callback 能区分来源。

仅用 QPC 对齐同一 Windows 机器上的进程，不混用各进程 Instant 起点或 `MSLLHOOKSTRUCT.time` 计算延迟；后者保留原始 u32 供对齐输入，不当作高分辨率单调时间。每批结束后才写 JSONL/summary；固定容量满时明确 `invalid: trace_overflow`，不能静默丢弃或当作 callback missing。长寿命产品 hook 不为排空日志而重启；单独 collector 线程通过下面的发布协议排空，磁盘写入永远在 hook 线程外。

原有公共 API 无法从 integration test 读取私有 CONTEXT/ABI 入口。E0/E1 的**真实 Host 内部归因**需要后续另行实现只读、debug/test 专用诊断点（下列草稿），或在同模块内添加测试；仅复制 callback 到 contract 测试只能证明机制可行，不能证明产品实际触发。本报告不假装这些诊断点已存在。

为避免诊断本身复现 H10，以下是固定容量 SPSC 原子队列草稿；字段可按上表扩展。**每个生产线程有独立队列**：Host listener 的 mouse/key/mark 共用一条，sender、其他观察线程各一条，禁止多线程写同一条。`raw` 内没有 FFI/回调/锁/分配，不跨重入点；完成写入后 Release 发布 head，collector 复制完后 Release 发布 tail。不要将多字段的 Relaxed 读取误当作完整记录。

```rust
use std::sync::atomic::{AtomicU64, Ordering::{Acquire, Relaxed, Release}};
const CAP: usize = 16_384;
struct Slot { at: AtomicU64, tag: AtomicU64, meta: AtomicU64, point: AtomicU64 }
impl Slot {
    const fn new() -> Self {
        Self { at: AtomicU64::new(0), tag: AtomicU64::new(0),
               meta: AtomicU64::new(0), point: AtomicU64::new(0) }
    }
}
struct Trace {
    head: AtomicU64, tail: AtomicU64, overflow: AtomicU64, rows: [Slot; CAP],
}
impl Trace {
    const fn new() -> Self {
        Self { head: AtomicU64::new(0), tail: AtomicU64::new(0),
               overflow: AtomicU64::new(0), rows: [const { Slot::new() }; CAP] }
    }
    fn raw(&self, at: u64, tag: usize, meta: u64, x: i32, y: i32) {
        let h = self.head.load(Relaxed); // 唯一 producer
        if h.wrapping_sub(self.tail.load(Acquire)) >= CAP as u64 {
            self.overflow.fetch_add(1, Relaxed); return;
        }
        let s = &self.rows[h as usize % CAP];
        s.at.store(at, Relaxed);
        s.tag.store(tag as u64, Relaxed);
        s.meta.store(meta, Relaxed);
        s.point.store(((x as u32 as u64) << 32) | y as u32 as u64, Relaxed);
        self.head.store(h.wrapping_add(1), Release);
    }
    fn pop(&self) -> Option<[u64; 4]> { // 唯一 collector
        let t = self.tail.load(Relaxed);
        if t == self.head.load(Acquire) { return None; }
        let s = &self.rows[t as usize % CAP];
        let row = [s.at.load(Relaxed), s.tag.load(Relaxed),
                   s.meta.load(Relaxed), s.point.load(Relaxed)];
        self.tail.store(t.wrapping_add(1), Release);
        Some(row)
    }
}
static HOST_TRACE: Trace = Trace::new(); // raw/mark 只由 Host listener 调用
// qpc() 是 QueryPerformanceCounter 的只读 FFI 包装；初始化时缓存 frequency。
// 实际实现用更多原子字段保留 code/wp/epoch，不能为了打包截断 tag。
// collector 周期性 pop 后落盘；结束先停止 producer、排空，再检查 overflow==0。
```

独立观察 hook 与 Host 入口诊断点采用同样模式，原始记录必须位于 CONTEXT 访问和 catch_unwind **之前**；每个错误分支另记一条。这里展示未来在 `safety.rs` 的插桩位置，不是已可从 integration test 调用的函数：

```rust
// 草稿：沿用原 ABI、有效 code/lp 检查及 SAFETY 注释。
// 此段 raw/mark 是 HOST_TRACE.raw 的无分配包装，不访问其他线程的队列。
let s = unsafe { &*(lp.0 as *const MSLLHOOKSTRUCT) };
raw(qpc(), s.dwExtraInfo, mouse_entry_meta(s.flags), s.pt.x, s.pt.y);
let caught = std::panic::catch_unwind(|| {
    CONTEXT.with(|cell| match cell.try_borrow_mut() {
        Err(_) => mark(BORROW_FAIL, s.dwExtraInfo),
        Ok(mut slot) => match slot.as_mut() {
            None => mark(CONTEXT_NONE, s.dwExtraInfo),
            Some(cx) => {
                mark_with_active(BEFORE_POLICY, s.dwExtraInfo, cx.mouse_proof.current_tag());
                // 此处保留原 callback 的完整逻辑，不用复制后的简化逻辑替代产品。
                original_callback_body(cx, HookEvent::Mouse(s.flags), s.dwExtraInfo);
                mark(AFTER_POLICY, s.dwExtraInfo);
            }
        }
    });
});
if caught.is_err() { mark(CAUGHT_PANIC, s.dwExtraInfo); }
mark(BEFORE_NEXT, s.dwExtraInfo);
let answer = unsafe { CallNextHookEx(None, code, wp, lp) };
mark(AFTER_NEXT, s.dwExtraInfo);
// keyboard_hook 同样插桩；返回 answer。mark/raw 不访问 CONTEXT，不 panic。
```

共用 sender 构造必须保留原样和单变量变体。下面是具体 FFI 记录草稿；`norm` 在真实 helper 内复用当前 `hook_probe::normalize`，而不是旧的 control_move 公式。不要通过按路径导入 policy 来验证命名闸。

```rust
use windows::Win32::{Foundation::*, UI::Input::KeyboardAndMouse::*};
fn abs(nx: i32, ny: i32, tag: usize, no_coalesce: bool) -> INPUT {
    let mut flags = MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK;
    if no_coalesce { flags |= MOUSEEVENTF_MOVE_NOCOALESCE; }
    INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT {
        dx: nx, dy: ny, dwFlags: flags, dwExtraInfo: tag, ..Default::default()
    } } }
}
fn f24_up(tag: usize) -> INPUT {
    INPUT { r#type: INPUT_KEYBOARD, Anonymous: INPUT_0 { ki: KEYBDINPUT {
        wVk: VK_F24, dwFlags: KEYEVENTF_KEYUP, dwExtraInfo: tag, ..Default::default()
    } } }
}
fn send_owned(packet: &[INPUT]) -> (u32, u32) {
    // SAFETY: sender 拥有初始化的 INPUT，传准确 cbSize；只在获准测试夹具内使用。
    unsafe {
        SetLastError(ERROR_SUCCESS);
        let n = SendInput(packet, std::mem::size_of::<INPUT>() as i32);
        (n, GetLastError().0)
    }
}
```

后续片段中的 `Rig` 是**待实现的测试 helper 接口**，不是仓库已有 API。最小实现合同如下，避免将关键步骤藏在名称里：

- `Rig::new()`：检验显式桌面 opt-in、当前进程标准用户/Default/input session；专用 UI 线程 CreateWindowExW 创建无菜单空白 client rect，专用 hook 线程先初始化队列再装鼠标/键盘 hook，安装失败立即报错；sender 是第三个线程，禁止 hook 线程调用 SendInput。
- `configure(state)`：向夹具 UI 线程 PostMessageW 一个 WM_APP 命令并等有界 ack。`Held` 先发送左键 down 并确认；`Capture` 在该 UI 线程调用 SetCapture(hwnd) 并确认 capture HWND；`Modal` 的窗口过程运行有界的 PeekMessage/TranslateMessage/DispatchMessage 循环；`SlowQueue` 降低窗口取消息频率但 hook 独立泵。用 GetGUIThreadInfo(target_tid) 读跨线程 hwndCapture，不能用 sender 的 GetCapture 代表目标 capture。无捕获的 held 状态也必须实测确认框架未隐式捕获。
- `trial(case, tag)`：sender 进入 PMv2；采物理点/虚拟桌面/clip；按 case 构造输入并记录发送；每次独立调用/批次都记录 returned count；不做隐式重试。一次 1px 差异移动仅用于空白夹具诊断，随后点位重新采样；不补偿用户移动。`Original` 模式一包 `[mouse, f24_up]`，其他模式见 E7。
- `collect(duration)`：持续泵独立 hook，将所有标签保留到最后一次发送后至少 1s；不继续持左键等 1s，发送块先释放。迟到只用于诊断，绝不回填生产租约。
- `Drop/finish`：只释放本测试自己按下且尚未释放的左键、在夹具线程 ReleaseCapture、Unhook、退出并 join 独立线程，再落盘；独立清理线程以每次持键 ≤2s 为上限执行 up-only 清理。外部物理输入、目标失焦、桌面变化或无法确认 owned state 立即终止 case；不自动恢复产品 Stop，不继续产品动作，不恢复旧光标位置。
- `compare()`：按发送账本的 `(run,sender_pid,tag,kind)` 关联各观察者入口/处理/发送，分别保留 observer pid/epoch；窗口消息通常不直接携带完整探针标签，只按时间/点位辅助对齐，不虚构一一映射。输出 missing/late/mismatch/overflow 分项，而不是只看测试进程退出码。差异实验随机分块、固定 seed、一次只改变一个因素。计数分母分别列动作数和 sent==requested 的探针数。

## 4. 可证伪实验（按共享观测可合并运行）

### E0：三层逐标签时间线，先确定丢在哪一层

覆盖 H1、H3–H6、H9–H13。在现有 `live_drag_stress` 的动作体外围加低扰动独立 mouse/key observer，Host 加 §3.1 的只读诊断点；保留一次重试，记录 first/retry 两个标签，统计 first 缺失而不是只数 Stop。对照组不装外部观察器，仅保留 Host 原子记录。

```rust
#[tokio::test]
#[ignore = "not_run: disposable unelevated interactive Windows desktop"]
async fn live_probe_rca_layers_classify_missing_first_attempt() {
    let mut rig = ProductRig::new(/* debug trace enabled, gate ON, retry unchanged */)
        .await.unwrap();
    // ProductRig 包装现有 driver()/Fixture/window/node/perform，动作体照抄
    // live_drag_stress 的 PointerDrag；不访问其他桌面，不自动 resume。
    for i in 0..120 {
        if let Err(error) = rig.run_existing_drag_once(i).await {
            rig.record_terminal_error(i, &error); // 仍保留尾部并落盘，不重放动作
            break; // 包括 Stop；不自动恢复、不继续注入
        }
    }
    rig.finish_and_join().await.unwrap();
    rig.classify_by_full_tag_and_epoch().unwrap();
}
```

采集：§3.1 全字段，特别是**缺失鼠标的同标签 F24**、Host entry、borrow/panic、重装起止、重试换标签时间。新 helper 不得沿用 600/800 条静默 cap。只产生观测的 stress test 必须在 summary 标记 `reproduced=false` 或缺失次数，不能把 “test ok” 当成已证伪。

判读：

| 对同一标签的观测 | 可得结论 / 下一步 |
| --- | --- |
| Host mouse entry=有，borrow_fail/panic=有，policy=无 | H5/H6 得到直接证据；Windows 未调用这一说法对该次否定，转 E1。 |
| Host entry=有，policy 收到的 active_tag 不同或 flags 异常 | 转 H6/E1；若只在切换后迟到则 E7。 |
| 外部 mouse=有、Host entry=无 | 不是全局前 LL 消失；优先 H3/H4/H10，按链顺序解释，转 E4/E5。 |
| 所有 mouse=无，同标签 F24=有 | 支持鼠标专属路径/鼠标链问题；弱化整包拒绝/整体停泵，转 E2/E5；不能仅凭两个链节点沉默认定 OS 丢失。 |
| 两种 hook 均无 | 优先整链/上下文/环境/观测失败；发送 2/2 仍不能授权，转 E4/E8。 |
| 原标签后来出现 | 直接支持 H11；明确多少超过 100ms，不能放宽租约。 |
| overflow/observer 未安装/时间原点不明 | 本轮无效，否定“缺失证据有效”，先修 H13。 |

耗时：120 个动作约 5–15 分钟（先测前 10 个 setup 耗时再估算）；若未复现，预先限定扩展至 600 动作或 30 分钟，先到停止，不重试业务动作。风险：**会移动桌面并在夹具内按住/释放鼠标**，与现有 drag stress 相同；Stop 后立即结束，不能为凑样本自动恢复。

### E1：RefCell / panic / 证明接收机制的正反控制

覆盖 H5/H6。先用实际 Host E0 判断是否发生；再在 contract helper 中复制原始借用模式做可控正例，验证探针分类器确能识别“已进 ABI 但未处理”。控制实验**不是**实际根因证据。

```rust
thread_local! {
    static TEST_CONTEXT: std::cell::RefCell<()> = const { std::cell::RefCell::new(()) };
}
// 在安装诊断 hook 的线程执行；另一 sender 收到 channel 命令后调用 SendInput。
fn pump_while_borrowed(tx: &std::sync::mpsc::Sender<ProbeJob>) {
    TEST_CONTEXT.with(|c| {
        let _held = c.borrow_mut();
        tx.send(ProbeJob::SamePositionTagged).unwrap();
        let end = std::time::Instant::now() + std::time::Duration::from_millis(30);
        while std::time::Instant::now() < end {
            // SAFETY: 当前诊断线程自己的有效 MSG；刻意重入仅用于正例。
            unsafe { let _ = PeekMessageW(&mut MSG::default(), None, 0, 0, PM_REMOVE); }
            std::thread::yield_now();
        }
    });
}
// helper hook：entry 先 raw；TEST_CONTEXT.try_borrow_mut() 的 Err 单独计数，仍 CallNext。
// 阴性对照：先 drop(_held)，再发送并泵；其余输入与观测完全相同。
// panic 正例：仅在这个复制的诊断 callback、专用标签上 catch_unwind(|| panic!(...))；
// 安装临时测试 panic handler，禁止在产品 hook 注入 panic/延时。
```

另加纯策略回放，使用 `unsafe_mouse_probe.rs` 的 path-import 方法仅复用 MouseProof（不拿它验证闸），枚举 callback-before-complete、complete-before-callback、wrong tag、flags=0、begin_cycle、retry 后旧标签到达。每步记录 current_tag/debug_state；这无需桌面，测试运行仍交由 rcargo/wcargo。

```rust
let mut p = policy::MouseProof::new(Some(policy::MouseProbe::AbsoluteSamePosition));
p.begin_cycle(true);
let first = p.next().unwrap();
p.callback(first.tag, 1, std::time::Instant::now());
p.complete(Ok(())).unwrap();
assert_eq!(p.proven_tag(), Some(first.tag)); // 回调可以先于 sender report
p.begin_cycle(true);
let second = p.next().unwrap();
p.complete(Ok(())).unwrap();
let retry = p.next().unwrap();
p.callback(second.tag, 1, std::time::Instant::now());
assert!(p.proven_tag().is_none()); // 迟到旧标签不能证明新尝试
p.callback(retry.tag, 1, std::time::Instant::now());
p.complete(Ok(())).unwrap();
assert_eq!(p.proven_tag(), Some(retry.tag));
```

采集：entry/borrow/panic/active_tag/generation 与状态机步骤。支持规则：真实故障同标签 entry→borrow_fail 或 panic，且缺口一致；或真实 entry 的标签在接收前被错误推进。否定规则：至少一次真实缺失中该标签从未进入 Host，便否定本地处理作为**那次**主因；入口有、正常完整处理且无冲突，则 H5 对那次否定。只在正例成功不支持真实根因。

耗时：纯策略/合成控制 1–2 分钟；真实归因随 E0。风险：合成同位置探针可能触发 hover/F24，不做按钮操作；panic 与借用重入只在复制 helper 内，不能破坏真实守卫。

### E2：按钮、capture、模态循环与移动类型的正交矩阵

覆盖 H1/H2/H7/H8。独立最小夹具，不运行产品动作状态机；先固定 PMv2、稳定显示布局、无 ClipCursor，不装旧 HookLog。每一输入状态均验证实际左键/capture 状态。下列 `CaptureOnly` 指在无按键时由夹具自己的线程捕获并验证；不符合 Windows 实际状态的 case 标 invalid，不能硬归为无缺失。

```rust
let states = [Idle, HeldNoCapture, CaptureOnly, HeldCapture, HeldCaptureModal];
let moves = [AbsoluteSamePixel, AbsoluteAdjacentPixel, RelativePlusOne];
let mut r = Rig::new()?;
for case in seeded_shuffle(states.into_iter().flat_map(|s|
    moves.into_iter().map(move |m| Case::new(s, m, /* no_coalesce */ true)))) {
    // 每块 <=2s 持键，20ms 一次，块间释放并核实清理；总数每 cell 先 300，后 1500。
    r.run_in_owned_blocks(case, 300, std::time::Duration::from_millis(20))?;
}
r.collect(std::time::Duration::from_secs(1))?;
r.finish_and_compare()?;
```

capture 控制在**窗口所属线程**执行的 FFI 核心如下。helper 使用不同 WM_APP 命令设置 capture/modal/取消息策略；ack 以窗口线程状态写入 channel/原子字段实现。不能从 sender 调用 SetCapture 来假装目标已捕获。

```rust
// 放在独立诊断窗口的 WndProc 中；RCA_CAPTURE=WM_APP+1，命令只由测试发送。
if msg == RCA_CAPTURE {
    // SAFETY: hwnd 是本 UI 线程创建的诊断窗口，操作只在该线程执行。
    unsafe {
        if wp.0 != 0 { let _ = SetCapture(hwnd); }
        else { let _ = ReleaseCapture(); }
    }
    acknowledge_capture_command(); // 仅确认命令执行；sender 再用 GetGUIThreadInfo 核实
    return LRESULT(0);
}
// HeldNoCapture 的窗口过程不在 WM_LBUTTONDOWN 中捕获；HeldCapture 则显式捕获。
// 所有路径对 WM_LBUTTONUP/清理命令释放本窗口拥有的 capture，再转 DefWindowProcW。
```

采集：左右链入口、每个 sender 记录、GetGUIThreadInfo 的 hwndCapture/flags、窗口 WM_MOUSEMOVE/WM_LBUTTON*、实际点位。夹具窗口日志最好用预分配队列，退出后写盘。相邻像素始终选夹具 client rect 内、避开边缘；相对 +1 按每块中心重新布置，不能不停 +1 走出夹具，也不把 +1/-1 当作无副作用操作。

判读：在最小夹具稳定复现同位置缺失、相邻绝对/相对不缺、原始 hook 无借用失败/摘钩/链截断证据时，支持 H1，但尚不能区分 Windows 与链前外部过滤。只按住就出现支持 button 依赖；仅 capture/modal 出现支持 H8；窗口端消息少、两个 LL 都全则只证明下游合并并否定其作为 LL 缺失原因。所有模式均缺且按安装 epoch 聚集，回 E4/E5。未复现只能对本样本/夹具给上界，不能说 H1 被普遍否定。

耗时：15 cells ×300×20ms=90s 纯输入，加清理/稳定期约 3–5 分钟；仅对出现差异的 cell 扩至 1500。风险：**会移动光标、按住左键和捕获鼠标**；限定空白夹具、每块释放；相邻/相对移动可产生拖拽效果，绝不能直接升级为产品探针。

### E3：NOCOALESCE、重复坐标/标签、队列消费的单变量对照

覆盖 H1/H2/H8。根据 E2 最敏感状态，固定其他条件，先 2×2：NOCOALESCE on/off × 窗口正常取消息/30ms 降频（hook 泵仍正常）。之后才比较 same-pixel、交替邻像素，以及每次唯一标签/固定诊断标签。固定标签组不能用于产品证明，只用返回数与事件总数/顺序评价，无法逐标签一一关联。

```rust
for no_coalesce in [true, false] {
    for queue in [DrainNormally, DrainEvery30ms] {
        r.configure(HeldCapture.with_queue(queue))?;
        r.run_in_owned_blocks(Case::same_pixel(no_coalesce), 1000,
                              std::time::Duration::from_millis(20))?;
    }
}
// 第二阶段：保持 NOCOALESCE=true，变动坐标或 tag，一次只变一个因素。
// 降频只在夹具 UI 线程的消息循环实现；不在 WH_MOUSE_LL 内 sleep。
```

采集：各 LL 的 message/tag/pt/time、窗口接收次数、发送/泵延迟、capture 和 clip。支持规则：唯一标签输入在 LL 之前被后一条覆盖/省略，且随 same-pixel 或标志改变稳定变化，支持前 LL 去重/优化；若只有应用消息数改变，否定其解释 LL 缺失。NOCOALESCE off 的效果只有在相同输入数、同状态、无 trace overflow/observer 超时的多块交叉实验中才算证据。

耗时：基础 4×1000×20ms=80s，加稳定/重复约 3–5 分钟，第二阶段再 2–4 分钟。风险：会按住鼠标、可能产生拖拽；降频会让测试窗口短时不响应，但 hook 在独立线程；不冻结真实应用、不修改系统 NOCOALESCE 配置。

### E4：每秒重装、重入与 hook 超时的生命周期实验

覆盖 H3/H4/H5。真实产品组只记录现有 1s 重装的 begin/installed/old-unhooked/end 与实际 epoch，不改变重装周期。复制的最小 harness 再做“持续不换”“每秒先装后卸”“两种不同静态 callback A/B 轮换”，用 A/B 明确来源；保持真实产品守卫原样。

```rust
// AB callback 使用相同记录/CallNext 模板，只写不同的 observer epoch。
for replacement in [NeverReplace, ReplaceEverySecondSameProc, ReplaceEverySecondAB] {
    r.set_diagnostic_replacement(replacement)?;
    r.run_drag_blocks_for(std::time::Duration::from_secs(60))?;
}
// 正例仅在独立短命 helper 进程中：处理一次专用标签时延时 1200ms。
// 当前 Win10/11 的文档上限使它能测试摘钩检测链；不是生产路径设置。
if isolated_timeout_positive_control {
    r.spawn_timeout_helper(/* sleep only once, 1200ms */)?;
    r.send_one_same_pixel_without_buttons()?;
    r.observe_no_hold(std::time::Duration::from_secs(3))?;
}
```

采集：本地处理、CallNext、总 ABI 耗时、线程调度/泵间隔、每代首末回调、重装各阶段、同标签重复数；只读记录 OS build。LowLevelHooksTimeout 的实际注册表值由总控在测试账号/测试会话中记录，如无法获取则 unknown，不能拿当前 Linux HOME 下的配置替代。本任务不读任何用户凭据目录。

判读：某代最后一次回调显著拖延，此后无任何 mouse、下次重装才恢复，支持 H3；同代马上成功且未重装，否定永久移除解释。故障聚集在重装区间只是相关；复制 harness 的不换组稳定、AB/同 proc 重装组可重复缺失，才增强 H4；若同时 borrow_fail 则转 H5。新旧 hook 重叠导致重复是可预期现象，不能单独算故障。正例验证检测能力，不证明真实存在 timeout。

耗时：基础 3×60s 加正例/清理约 4–6 分钟；低概率组按 E0 缺失率预先扩样。风险：基础同 E2；**1200ms 正例会短暂拖延测试桌面输入**，仅在独占可丢弃桌面、按钮全部释放后用短命 helper 执行，外部 watchdog 5s 内终止 helper。不能在有真实守卫/用户工作时跑，也不改注册表。若桌面无法独占，正例标 not_run，不影响其余观测实验。

### E5：另一 LL hook 的链序与观测器干扰

覆盖 H3/H4/H10/H13。三组：无测试观察 hook、低扰动原子观察 hook、旧 HookLog；每组用同一产品 Host 原始诊断点判断，不用“无外部日志”代替缺失。对原子观察器再测安装在 Host 之前/之后，并记录 Host 每秒重装造成的链序变化。

```rust
for observer in [None, AtomicObserverBeforeHost, AtomicObserverAfterHost, LegacyHookLog] {
    let mut p = ProductRig::with_observer(observer).await?;
    p.run_existing_drags_until_stop_or_limit(120).await?;
    p.finish_and_compare().await?;
}
// 检测链截断的正例：仅独立诊断链中的“较新”测试 hook 对某个测试专用 tag 返回 1。
// 该 hook 对其他任何输入仍 CallNext；不用真实 Stop hook 作被阻断对象。
// newer_raw 有、older_raw 无，验证分析器能识别链截断；马上卸载该 hook。
```

采集：hook 安装成功/失败、epoch、顺序变更、各入口/CallNext 耗时、队列占用/overflow、旧 HookLog cap 是否已达。支持规则：不挂测试 hook 仍发生则否定“另一测试 hook 必要”；仅 Legacy 组缺失且链耗时/锁竞争对应，支持观察扰动；较新节点见到而较旧节点不见、或其 CallNext 路径缺失，支持链截断/旧 hook 不活。较新/较旧两个观察器都看不到仍无法排除链前第三方节点；不通过杀安全软件或卸载系统 hook“排除”。

耗时：四组各最多 120 动作，约 20–60 分钟；先复用 E0 的 None/Atomic 两组，仅在证据指向该处时扩至 Legacy/顺序组。风险：产品拖拽会动桌面；旧日志器可能扰动时序；截断正例只截断自定义探针，不截真实输入，独占测试桌面且不运行真实守卫动作。

### E6：归一化、DPI、ClipCursor 与异步位置验证

覆盖 H7，并区分 H1/H2 的坐标依赖。先做无输入整数回放；再在现有显示配置中选择每个 monitor 的 client 内部、像素边界和负坐标点。不要主动修改显示布局、缩放或远程连接。

```rust
// 仅是待验证的 Windows 像素映射模型，不宣称 floor 是文档保证。
for size in [1_i32, 1920, 2560, 3840, 65535, 65536] {
    for pixel in 0..size {
        let n = policy::normalize(pixel, 0, size).unwrap();
        let predicted = ((i64::from(n) * i64::from(size)) / 65536) as i32;
        assert_eq!(predicted, pixel);
    }
}
for dpi in [PerMonitorV2, SystemAware, Unaware] {
    // PMv2 是原样组；其他两组仅诊断，显式保存并恢复线程 DPI context。
    r.set_sender_dpi(dpi)?;
    for point in r.fixture_interior_sample_points()? {
        r.stage_inside_fixture(point)?;
        r.record_physical_logical_and_bounds()?;
        r.run_same_pixel_block(300)?;
    }
}
// 可选 clip 对照仅夹具自己的 UI 线程设置，并保存/恢复原 clip；失败立即中止。
// 不对其他窗口/用户会话设置 ClipCursor；真实配置不支持多 DPI 则对应 cell not_run。
```

采集：物理/逻辑读数、DPI context、虚拟桌面 origin/size、normalize 值、MSLL 点、实际轨迹、clip、worker 最后 move 返回/settle 结束/探针采样 QPC；独立线程按 1–2ms 采样，无输入时不 busy-spin。真实轨迹以 raw hook 为主；2ms 采样不能排除更短暂往返。

判读：纯整数回放通过仅验证模型内一致性；真实 PMv2 组仍有 1px 等偏移、故障随边界/clip/未落定移动集中，支持 H7。真实缺失时点位/布局始终相同、相邻控制 LL 正常且没有任何瞬态证据，弱化但不能绝对否定亚采样周期竞态。若只在人为非 PMv2 组出现，**不能**归因给已进入 PMv2 的产品。

耗时：整数回放秒级；实际每 cell 300×20ms=6s，少量点/3 上下文约 3–6 分钟。风险：会移动光标；可选 clip 暂时限制测试桌面光标，独占条件不满足就不跑该子项。整数回放无桌面影响；不为测试调整真实显示设置。

### E7：同包顺序、F24 配对与迟到尾部

覆盖 H6/H9/H11。固定 E2 中最敏感状态/NOCOALESCE，做 4 组；MouseOnly 仅测试 mouse 层，不可提供生产双钩子证明。每组保留最后一次发送后 1s 的原始尾部，不借此续租。

```rust
match order {
    OriginalMouseThenKey => log_send(&[mouse, f24_up(tag)]),
    KeyThenMouse => log_send(&[f24_up(tag), mouse]),
    SeparateMouseThenKey => {
        log_send(&[mouse]);
        log_send(&[f24_up(tag)]); // 不加暂停；两次调用分别记录 QPC/count/error
    }
    MouseOnly => log_send(&[mouse]),
}
// 每组先 1000 个探针，分块持键 <=2s；原始日志不因 active_tag 改变而丢旧标签。
// 追加记录每个 first/retry 的 current_tag 切换时刻，仅作为产品组的只读观测。
```

采集：同标签 mouse/key 的两个 ABI 入口与退出、两次发送边界、实际应用 key/move 消息、generation/重试切换时间、1s 尾部。支持规则：原样组 mouse 缺而 F24 到，且 MouseOnly 稳定不缺，支持混包依赖；仅 key-before-mouse 改善说明顺序相关，但不能说明 Windows 违反 D1。mouse 迟到且早于新标签/晚于回报的关系支持异步/接收问题；只有 F24 迟到解释配对失败，但不解释原有 mouse=false。若所有原始鼠标入口都及时存在而证明丢失，则否定“打包令 Windows 不调鼠标 hook”，转 H6。

耗时：4×1000×20ms=80s，加尾部/随机交叉重复约 3–5 分钟。风险：按住鼠标及 F24 up 可触发夹具/全局软件的键盘逻辑；不把 F24 当作绝对无副作用。拆包可能增加双证明时间，若提出产品方案，必须重新满足原 100ms 预算，不能删掉键盘证明。

### E8：环境元数据对齐，检查输入桌面/会话/过滤边界

覆盖 H12，作为 E0/E2 旁路采集，先不增加输入。每块开始/结束记录输入桌面、进程/目标的完整性与 UIAccess/session、前景 HWND/线程、目标是否在当前桌面、OS build、当前可见输入设备/远程会话标志。只收测试进程和夹具身份，不枚举用户内容。

```rust
// read_test_environment 复用现有 unsafe_desktop/unsafe_token 检查语义，
// OpenInputDesktop + GetUserObjectInformationW(UOI_NAME)，GetTokenInformation
// 读取本测试/夹具 TokenIntegrityLevel、TokenUIAccess、TokenSessionId；全部只读。
let before = read_test_environment(fixture_pid)?;
require_same_medium_default_session(&before)?;
r.run_one_owned_probe_block()?;
let after = read_test_environment(fixture_pid)?;
write_environment_pair(before, after)?;
// 不提权、不切桌面、不打开 UAC、不退出/登录、不调整远程会话，不停安全软件。
```

采集：上述元数据及块内前景/输入桌面变更标记，和缺失标签关联。支持规则：缺失与明确的桌面/前景/完整性变化同现，支持环境机制但仍需专用夹具复测；稳定 Medium/Default/同 session 且 2/2 不足以否定插入后过滤，但否定普通静态 UIPI 拒绝这个解释。第三方过滤不能仅从现有 API 全面排除；若 E2 独立夹具仍缺、所有本地层和链证据正常，应交给维护者准备 ETW/WPR/微软反馈，而不是推断已证实内核 bug。

耗时：附加约几十秒至 1 分钟；风险：只读采集本身不动桌面，复用块有其原有输入风险。远程重连/显示变更等主动实验本轮不做，标 not_run；不要求真实账户或管理员权限。

## 5. 实验顺序、停止条件与统计判读

1. **E0 观测有效性 + E1 策略回放**：先排除 cap、安装失败、标签归因错误，再用一次真实缺失分层。优先回收同标签 F24、Host entry、borrow_fail/panic 和 1s 重装关联，信息量最大。E8 只读元数据同时采，不另开并行输入任务。
2. 若 entry→处理缺口，做 **E1 真实路径归因**，随后停止广撒网矩阵；若外部有而 Host 无，直接 **E4/E5**。若鼠标在所有观察器都无而键盘存在，先 **E2** 最小按钮/capture 矩阵。
3. **E7 混包四组**与 **E3 标志/消息消费四组**：单组成本低；根据 E0/E2 哪个因素最敏感先选一个，不跑全笛卡尔积。
4. **E4 生命周期基础组**：用 E0 时间线先筛；只有怀疑 timeout 才做独立 1200ms 正例，无法独占桌面则该正例 not_run。
5. **E6 坐标实测**与 **E5 Legacy/链序长跑**：仅对存在坐标/链迹象的方向扩样。E6 的纯整数检查可放第一步，不能据此替代实机。
6. 若最小独立夹具稳定复现而本地丢弃/重装/链解释均无证据，才准备针对相同 OS build 的更深系统追踪。该升级需要维护者另行安排；本次不下载工具、不启 ETW、不操作 win-test。

每个关键 cell 先定 300/1000 accepted probes 的筛查预算，再按**实际 first-missing 率**扩至 1500/3000 或固定墙钟上限。按随机分块保留 seed；低概率相关事件按动作/块聚类，而不是把同一拖拽里的探针都当独立样本。零缺失可报告 `约 3/n` 的 95% 上界，前提是近似独立；不满足时按块报告，不给虚假的精度。若对照组在相同预算也未复现，应写 inconclusive，而不是“修好了”。

任何真实产品 Stop 必须保留锁存并结束当前测试进程/用例，不自动 resume，不为完成样本数继续注入。只有独立诊断 harness 能在原始观察中继续看 1s 尾部；它不签发产品证明。任何 count 不全、桌面变更、外部输入、trace overflow、线程退出失败均标 invalid/not_run 或 failed，不能算 callback 缺失样本。正例、原样复现、候选变体必须分开计数。

## 6. 哪些成立后有机会根治

| 成立的证据 | 比一次重试更直接的方案轮廓 | 必须维持的验证条件 |
| --- | --- | --- |
| H5/H6：真实 ABI 到但本地丢弃 | 将 ABI 入口的最少标量写入固定容量无阻塞队列，在 listener 普通 poll 上更新 MouseProof；不持 CONTEXT 借用跨可能重入的 Win32/诊断调用。panic/overflow/context 不存在显式失效并记原因，不伪造成功。 | 使用**入口时间**与原标签/代做证明；延后处理不能重新计时续租，旧代/迟到标签仍拒绝；物理 Stop/活动路径及时性不能回退；队列满 fail closed。 |
| H4：重装交错可重复造成缺失 | 明确 epoch owner、hook retirement 与在途消息接收关系；可在新旧代之间共享只读入口队列，先取得新代真实带标签双回调，再退休旧代。若调整更换时机，必须在原租约内，不能仅延后重装掩盖摘钩。 | 不接受旧代证明新代、不新增未监控空窗，不改 100ms/4750ms/5s。重装设计需安全审阅，不能凭“少重装”即宣布解决。 |
| H3：本地 hook 工作/CallNext 负载导致超时 | 入口最小化、去掉锁/格式化/阻塞写；将日志及策略处理移出 hook；保留真实活性探针。 | 实测入口至返回分段时延和摘钩失败路径；第三方链故障无法由我们根治，仍需及时 fail closed。 |
| H9：仅混包/顺序触发 | 采用经真机证实的输入顺序或拆包，将鼠标和键盘证明独立发送但同尝试绑定；保持跨进程指针闸和坐标检查。 | 两种真实回调仍必需，测完最坏延迟和 Unicode/拖拽回归；不将 SendInput 成功计作证明。 |
| H7：归一化/物理坐标或可定位的异步竞态 | 修正物理映射/上下文/clip 验证，保留读→发→复读与已落地动作的互斥关系；显示布局变化时拒绝当前证明并重新观察。 | 禁止补偿用户移动，禁止靠容忍位移放松正确性；需要真实多 DPI/负坐标/边界证据。 |
| H8：特定 NOCOALESCE/捕获交互 | 若单变量、跨多次重装的实验确认，可评估省略 NOCOALESCE 或调整发送组织；应用消息策略与 LL 证明分别回归。 | 先证明带标签 LL 交付稳定和鼠标行为不变，不能仅因应用消息少/锁存少就发布。 |
| H10/H13：测试观察器制造问题/错误归因 | 用固定缓冲替换旧 HookLog、显式安装状态/overflow、统一 QPC 和标签；清除测试自身持锁回调。 | 这能根治测试伪影，但只有无外部 hook 的真实 Host 也不再缺失才能声称产品问题解决。第三方不合作 hook 仍是失败关闭边界。 |
| H1/H2：Windows 确实省略同位置事件 | **目前没有已证明无副作用且有逐回调保证的鼠标替代探针。** 只能继续评估可用的零效果输入类型，或重新设计带真实鼠标证明的活性机制并写 ADR。 | 不直接改用 ±1px、XButton、SetCursorPos、仅 F24、仅线程心跳或 SendInput count；这些分别有拖拽/导航/副作用或不能证明鼠标 hook。此类改动需维护者决定，现有一次重试仍是缓解。 |
| H11/H12：OS/链前长期延迟或外部过滤 | 若是本地过早换 tag/错误配对，修状态关联；若真实已超 100ms 或过滤不受我们控制，当前安全策略下应失败关闭并提供诊断，不承诺无条件消除锁存。 | 不扩大期限，不用旧标签救新尝试，不自动恢复；更广的替代安全架构必须经 ADR/维护者审批。 |

只有 E0/E1/E4/E7 等提供明确层内因果证据，才适合实现“根治”。现阶段可以立即改进的是诊断设计；本报告没有授权更改输入协议，也没有用推测否定 #157 的必要性。

## 7. 总控一次性计划任务运行方式与验证状态

本次是文档任务，不编译。本机已有 rcargo/wcargo 路径不代表已调用；以下是**后续草稿落地后**的验证入口，全部 `not_run`。纯报告交付不需要为了检查草稿而修改 Cargo 或运行 GUI 编译。

```text
# 构建任务只在合适构建机、逐个执行；不在此 ARM worktree 本机编译：
wcargo test -p gcu-driver-windows --features test-worker-faults --test contract --no-run -j 1
# Windows GUI/夹具构建由总控使用已有 Windows GUI 构建流程安排，wcargo 非 GUI 路径不代替它。
# 若只落实纯策略回放（放入 portable tests），可在 build8 串行验证：
rcargo test -p gcu-driver-windows --features test-worker-faults --test contract probe_rca -j 1
```

下面是**计划任务 action 所运行脚本的正文模板**；`$TestExe/$WorkerExe/$FixtureExe/$OutputDir` 由总控传入已构建绝对路径。标准用户已登录的交互式 Default 桌面、Normal 优先级、非提权，与 diagnostics 的已验证运行方式相同；不能改成 SYSTEM/session 0，也不在本次创建任务。注册任务时应设置 Normal 对应的任务优先级（XML Priority=4），不能继承默认 BelowNormal。开始前复用仓库 desktop/token 预检；预检不通过写 not_run，不继续。

```powershell
# 后续实现命名：在 live.rs 声明 unsafe_probe_rca 后提供统一入口。
# GCU_RCA_CASE 是拟新增 helper 的选择器，不是现有产品环境变量。
$env:GCU_DESKTOP = '1'
$env:RUST_MIN_STACK = '8388608'
$env:GCU_WORKER_EXE = $WorkerExe
$env:GCU_FIXTURE_EXE = $FixtureExe
$env:GCU_CONTRACT_WORKER_EXE = Join-Path (Split-Path $WorkerExe -Parent) 'gcu-contract-worker.exe'
$env:GCU_NATIVE_TRACE_DIR = Join-Path $OutputDir 'native'
$env:GCU_RCA_REPORT = Join-Path $OutputDir 'probe-rca.jsonl'
$env:GCU_RCA_CASE = 'layers' # 后续改为 cells/coalesce/lifecycle/chain/coords/batch/environment
$env:GCU_RCA_SEED = '20261022'
Remove-Item Env:GCU_POINTER_GATE_OFF -ErrorAction SilentlyContinue
$name = 'live::unsafe_probe_rca::live_probe_rca'
$listing = @(& $TestExe --list --ignored --exact $name)
if ($LASTEXITCODE -ne 0 -or "$($name): test" -notin $listing) {
    throw 'not_run: expected RCA test missing; draft is not implemented'
}
New-Item -ItemType Directory -Force -Path $env:GCU_NATIVE_TRACE_DIR | Out-Null
& $TestExe --ignored --exact $name --nocapture --test-threads=1 *> (Join-Path $OutputDir 'probe-rca.log')
$caseExit = $LASTEXITCODE
Set-Content -LiteralPath (Join-Path $OutputDir 'probe-rca.exit') -Value $caseExit
exit $caseExit
```

统一入口只路由选择的 E0–E8，不默认全部跑；E1 纯策略可由独立测试入口执行。总控读 JSONL、summary、exit/log，至少确认 `case/n_requested/n_sent/n_first_missing/n_retry/mouse_entry/key_entry/borrow_fail/panic/epoch/late/overflow/reproduced`，不单读 exit code。timeout 正例独立子进程；长任务外部有界终止，清理逻辑仍负责 owned-button up，不靠杀进程释放虚拟按键。

| 验证 | 本次状态 |
| --- | --- |
| 必读协议、指定实现/测试/真机记录的只读核对 | 完成；本文引用不代表新增真机证据 |
| 报告文件范围、Markdown 结构、相对本地链接、空白检查 | 通过：`git status --short` 仅本报告；`python3` 文档自查覆盖 13 假设/9 实验/本地链接/代码围栏；`git diff --no-index --check -- /dev/null <报告>` 无空白错误（no-index 新文件差异退出 1） |
| 所有 Rust/PowerShell 草稿的编译、E0–E8、win-test 任务 | **not_run**；仅设计，没有向测试代码落地，没有访问真实桌面 |
| 实现修改、Stop 常量/语义变化、STATUS、push/PR/分支操作 | 未做；不属本次允许范围 |

遗留问题：缺少失败标签对应的原始 Host ABI 入口、同标签 F24、安装 epoch 和完整尾部；因此无法从现有证据唯一确定 Windows、hook 链还是本地证明接收层。下一份有效证据应来自 E0 的一次分层复现，而不是增加一次重试后的成功次数。
