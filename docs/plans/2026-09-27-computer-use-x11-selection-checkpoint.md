# Computer Use：X11 原生选区替换与分步失败检查点

时间：2026-09-27，Asia/Shanghai。分支 `feat/computer-use-implementation`，HEAD `30757366a739ec9aaf0ccc95bbb3efe19a067aa9`；保留已有大量 dirty/untracked 工作。完整目标继续 **active**，这不是最终版验收。

## 本轮实现

- 真实 GTK 红测确认旧 `TypeText` 在有选区时直接失败。现在通过 AT-SPI `GetSelection / DeleteText / InsertText / SetCaretOffset` 操作原引用的精确字符范围；**没有使用整字段 `SetTextContents` 模拟输入替换**。保留 `SetValue` 独立的整字段语义。
- wire 位置是 Unicode code point，`InsertText` 长度是 UTF-8 字节。支持正向/反向选区、全选、未选中插入、中文、emoji、组合字符；输入/最终文本受 32768 字节上限约束。空 `TypeText` 保留选区和光标且不发原生写入。多个或不一致选区明确拒绝，不猜目标范围。
- 文本、光标、选区做重复稳定读回；每一步复核原 unique bus owner、进程/窗口、角色/名称、可用性、完整祖先链、run 授权、几何、前台、物理按键状态与取消。步骤间的文本/选区漂移中止后续输入；只在光标仍处于可预期的插入起点时推进光标，不随意接管。
- `DeleteText + InsertText` **不是原子事务，也不宣称一次 undo 分组**。已删除后遇到取消、禁用、外部改文等，返回 `Applied / applied=true / postcondition_ok=false`，描述 partial；不回滚覆盖用户改文，不自动重试或重新找同名控件。
- 原生调用发出后回包丢失继续返回 Unknown 路径，并保留 `NativeActionSlot` 占用。真实延迟回包后仍不偷偷解除隔离，后续动作不会重放。这证明未知完成隔离，不证明完整恢复已经实现。
- 修正首次能力查询的缓存缺口：成功模型 AT-SPI 观察也发布总线可用性，活动输入持锁时第一次 `capabilities` 不再错误返回 semantic=false。原生夹具在真正 DeleteText 在途边界验证查询不阻塞。

生产文件：`src-tauri/computer-use-x11/src/accessibility_text.rs`、`accessibility.rs`、`lib.rs`。夹具扩展 `selection_fixture.rs`、`semantic_fixture.rs`、`fixtures/gtk_accessible.py`；App 继续重导出同一 LinuxAdapter，没有单独的 fixture-only 输入实现。

## 原生 oracle 与范围

私有 Xvfb + D-Bus/AT-SPI + 独立 GTK 进程。GTK 自身读回 text/caret/selection 和删除/插入事件数，不由探针伪造预期效果。原生 DeleteText 回调边界分别注入外部改文、禁用、Stop 和超过生产方法期限的回包延迟。Stop 先观察真实在途回调，再撤销授权，不靠固定 sleep 假设任务已开始。

生产 Broker 另外验证：已删除但未插入的结果保留 Applied/executed，不冒充 Verified；同 action ID 重复调用不会再执行。负向未知完成用独立适配器及其自行枚举的目标；丢弃私有夹具只是测试清理，不冒充生产恢复。未修改用户 App/runtime/全局剪贴板，也未操作任何权限提示。

## 红绿过程与保留失败

证据：`tools/computer-use-probe/.run/x11-selection-20260927/`，不覆盖旧 Wait 批次。

- `red-native`：exit1，真实原生选区请求失败于 `typeText requires an unselected caret`；实现后 `expanded-native` 验证选区成功。
- `green-native`：exit1，GTK 初始 ready 通道超时，尚未执行功能门禁；没有把该失败当成功。后续独立运行及冻结三轮均通过，单次启动超时原因未据此声称根治。
- `fault-native`：exit1，测试错误地将旧适配器的 opaque target 交给新适配器，被 lifetime 校验拒绝；修正夹具为各自枚举绑定，不放松生产身份校验。同时将不能正确 GI marshal 的 `GtkEditable::insert-text` Python 事件观察改成 `GtkEntryBuffer::inserted-text`，最终无该警告。
- `fault-fixed-native / broker-diagnostic-native`：暴露夹具向 Broker 传入 `value` 而不是协议允许的 `text`。遵守既有 schema 修正夹具，未扩大 Broker 参数面。
- `expanded-clippy`：exit101，生产入口放在 test module 之后。移动入口后 strict Clippy 通过，没有增加 allow 或降低警告门禁。
- 补充原生场景后的 suite 上限为 90 秒，仅是整个测试进程的 watchdog；生产 AT-SPI 方法仍使用一秒期限，未扩大生产动作期限。

## 冻结回归与证据审计

| 项目 | 实际结果 | 范围 |
| --- | --- | --- |
| X11 单测 | **31/31** | 原有 17 + 精确文本编辑状态机 14；0 failed/ignored/filtered |
| Linux Core + driver | **560 + 16 = 576/576** | 使用当前隔离 runtime seed；driver 为真实 owned 子进程协议验收 |
| Rust 合计 | **607/607** | `frozen-unit / frozen-core`，exit0 |
| 原生语义 | **36/36 × 3** | 旧 25 门禁保留，加 11 项选区/故障/能力/Broker 门禁 |
| 原生坐标 | **19/19 × 3** | 原窗口/像素/输入/生命周期门禁完整重跑 |
| 原生重复范围 | **每轮 55 项，连续三轮通过** | 不是 165 个不同功能，不是 12h soak |
| 构建/strict Clippy | **exit0** | X11 all-targets/native-probe 与 Core all-targets/test-support |
| 格式/语法/差异空白 | **exit0** | Windows workspace cargo fmt、GTK Python AST、git diff --check |
| 冻结源 | **296 文件零漂移** | Core/X11/根适配器、依赖、选定 CI 与夹具，不是整个脏仓库 |

`audit.mjs` 对照上一批原生门禁和本批明确新增列表，逐项核验冻结三轮日志/退出码/顺序、单测数量和新增测试名；不把实际通过列表本身当作唯一需求。另核对 `TypeText` 无整字段调用、红测确实失败，以及源/证据 SHA-256。产物：`expected-native-gates.json`、`final-audit.json`、`source-before-final.json / source-after-final.json`、`frozen-native.sha256`、`handoff-readback.json`、`receipt.json / receipt.sha256`。

API 语义依据保存为 `EditableText.xml / Text.xml / gtkentryaccessible.c`，来自 GNOME 官方 at-spi2-core 与 gtk-3-24 仓库；接口源码只是语义依据，功能完成由真实原生行为与回归证明。已有 CI 原生探针入口会运行新门禁，本批没有修改 CI，也不宣称远端 CI 已执行。

## 未完成的最终范围

- 这是 WSL Debian owned GTK/Xvfb/AT-SPI 的执行，不是已安装 App、多工具包实机/多屏/DPI/输入法矩阵。分步 AX 编辑无法保证与物理键入完全相同的事件/undo 行为；非原子读写的所有竞态未被证明排除。
- 中文 IME composition、完整 clipboard ownership/restore、非响应服务器和完整未知输入恢复仍需开发验收；本批没有用安全拒绝或永久 busy 代替这些功能。不得把中文 Unicode 插入称为 IME 已完成。
- 保留完整目标：Windows、macOS arm64+x64、Linux X11+GNOME native Wayland；Desktop、托管浏览器、既有标签页、App WebView；App/ACP/MCP；签名安装/升级/回滚；重设计原生 UI 窄窗/系统缩放/权限矩阵；真实 Grok E4；最终候选源码冻结后的 **12 小时 active soak**。此前 Windows 发布/浏览器长尾、原生 UI 未执行部分继续保留。
- 下一步沿现有 G9 继续处理 Linux clipboard/IME/恢复，并完成其他平台与交付门禁。本轮分类 **progress**；目标实际查询仍为 **active**，没有 complete/blocked/paused 状态写入，没有 commit/push/PR。
