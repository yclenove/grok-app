# `gcu-fixture` 跨平台测试夹具应用

跨平台自动化测试夹具应用（基于 `eframe 0.36` 与默认 `AccessKit`），作为 Grok Computer Use v2 契约用例 C03–C15 及模型评测任务（`07-quality-gates.md`）的被测对象。

---

## 1. 架构与设计

1. **逻辑与 UI 分离**：
   - `src/state.rs`：纯状态机与原子持久化逻辑，无 GUI 依赖，可在任何无头/CI 环境独立单测。
   - `src/ui.rs`：基于 `eframe`/`egui` 的交互界面，为控件提供稳定的自动化标识（Name/ID）与双语标签。
   - `src/main.rs`：启动引导程序，配置 1280×860 默认窗口尺寸。
2. **外部断言通道**：
   - 当设置环境变量 `$GCU_FIXTURE_STATE` 时，每次状态变更均原子写入该路径（先写入同目录下临时文件，再执行原子重命名 `rename`）。
   - 包含 `text`、`password_len`（不泄露明文）、`checkbox`、`checked`、`radio`、`combo_selected`、`slider`、`counter`、`list_visible_range`、`tab`、`modal_open`/`modal_closed_count`、`drag_drop_ok`、`grid_hit`、`submitted_count`、`send_clicked`、`form`（`form.name`, `form.agree`, `form.submitted_count`）及最近 50 条按来源标记的 `event_log`（`keyboard`/`mouse`/`accessibility`）。
3. **安全脱敏保证**：
   - 密码框在内存中使用独立字段，序列化时明文被跳过，持久化文件中仅保留 `password_len` 与 `"is_password": true`。
4. **自绘画布区（无无障碍子节点）**：
   - 8×6 自绘色块网格（固定尺寸 320×240），在 AccessKit / 无障碍树中仅暴露为单一容器节点（Role: `Image`，Name: `grid_canvas`），不产生子节点，专供视觉坐标点击测试。

---

## 2. 控件清单与无障碍标识

| 控件 | 角色 (Normalized) | 标识 / Name | 界面标签 | 外部断言字段 |
|---|---|---|---|---|
| 单行文本框 | `text` / `edit` | `name_field` | Name / 姓名 👤 | `text`, `form.name` |
| 多行文本框 | `text` / `edit` | `multiline_field` | Notes / 备注 📝 | `multiline` |
| 密码输入框 | `password` / `secure_edit` | `password_field` | Password / 密码 🔒 | `password_len`, `is_password: true` |
| 复选框 | `checkbox` | `agree_checkbox` | Agree / 同意 ✅ | `checkbox`, `checked`, `form.agree` |
| 单选按钮组 | `radio` | `radio_group` (0..2) | Option 1 / 选项甲 🅰️ 等 | `radio` |
| 下拉选择框 | `combobox` | `combo_box` (0..3) | Item 1 / 第一项 🥇 等 | `combo_selected`, `selected` |
| 滑块 | `slider` | `slider_field` | Volume / 音量 🔊 | `slider`, `value` |
| 计数器按钮 | `button` | `counter_button` | Increment / 计数 🔢 | `counter` |
| 发送按钮（敏感词测试）| `button` | `send_button` | Send / 发送 🚀 | `send_clicked` |
| 提交按钮 | `button` | `submit_button` | Submit / 提交 📤 | `submitted_count`, `form.submitted_count` |
| 模态对话框按钮 | `button` | `open_modal_button` | Open Modal / 打开弹窗 💬 | `modal_open` |
| 模态确认按钮 | `button` | `modal_confirm_button`| OK / 确定 ✅ | `modal_closed`, `modal_closed_count` |
| 虚拟滚动列表 | `list` / `scroll_pane` | `virtual_list_scroll` | Row 0000..1999 | `list_visible_range`, `visible_range` |
| 拖拽方块 / 目标区 | `button` / `drop_target`| `drag_source` / `drop_target` | Drag Me 🟦 / Target 🎯 | `drag_drop_ok`, `drop_ok` |
| 自绘网格画布 | `image` (无子节点) | `grid_canvas` | Canvas Grid (8x6) | `grid_hit: (col, row)` |

---

## 3. 三平台无障碍验收清单

> **说明**：无头环境或远程编译机（无物理显示）无法拉起原生 GUI 与 AT 服务。在 CI 或开发机上实测原生无障碍树时，按以下清单验收。

### 3.1 macOS 验收清单

- **工具**：
  - Xcode Accessibility Inspector（`/Applications/Xcode.app/Contents/Applications/Accessibility Inspector.app`）
  - macOS 屏幕阅读器 VoiceOver（快捷键 `Cmd + F5`）
- **启动命令**：
  ```bash
  GCU_FIXTURE_STATE=/tmp/fixture_state.json cargo run -p gcu-fixture
  ```
- **检查项目**：
  1. **控件角色与名称**：
     - `name_field`：Role 为 `AXTextField`，Identifier 为 `name_field`，Label/Title 包含 `Name / 姓名 👤`。
     - `password_field`：Role 为 `AXSecureTextField`（或带有 password/protected 属性的 `AXTextField`）。
     - `agree_checkbox`：Role 为 `AXCheckBox`，`AXValue` 在勾选时为 1，未勾选时为 0。
     - `counter_button`、`send_button`：Role 为 `AXButton`。
     - 2000 行列表：Role 为 `AXTable` / `AXOutline` / `AXScrollArea`，树中仅出现视口可见行（约 10–15 个），滚动后树节点动态更新。
     - 画布区：仅暴露一个 Role 为 `AXImage` 的节点，子节点数量严格为 0。
     - 模态弹窗：点击打开后，弹窗作为 `AXSheet` 或 `AXWindow` 处于层级前端，且主界面进入模态或非焦点状态。
  2. **截图凭证**：
     - 使用 `screencapture -x -R<x,y,w,h> /tmp/mac_fixture.png` 截取夹具界面。
     - 使用 Accessibility Inspector 层次树截图作为 PR 交付物。

### 3.2 Windows 验收清单

- **工具**：
  - Accessibility Insights for Windows（推荐）或 Windows SDK Inspect.exe（`C:\Program Files (x86)\Windows Kits\10\bin\<version>\x64\Inspect.exe`）
  - Windows Narrator（快捷键 `Win + Ctrl + Enter`）
- **启动命令**：
  ```powershell
  $env:GCU_FIXTURE_STATE = "$env:TEMP\fixture_state.json"
  cargo run -p gcu-fixture
  ```
- **检查项目**：
  1. **UIA 角色与属性**：
     - `name_field`：ControlType 为 `UIA_EditControlTypeId` (50004)，AutomationId 为 `name_field`。
     - `password_field`：ControlType 为 `UIA_EditControlTypeId`，`IsPasswordPropertyId: true`，文本值被屏蔽。
     - `agree_checkbox`：ControlType 为 `UIA_CheckBoxControlTypeId` (50002)，`Toggle.ToggleState` 正确反映开/关。
     - `slider_field`：ControlType 为 `UIA_SliderControlTypeId` (50015)，支持 RangeValue 模式。
     - 计数器与发送按钮：ControlType 为 `UIA_ButtonControlTypeId` (50000)。
     - 画布区：ControlType 为 `UIA_ImageControlTypeId` (50006)，Children 为空。
     - 模态弹窗：ControlType 为 `UIA_WindowControlTypeId` (50032)，`Window.IsModal: true`。
  2. **截图凭证**：
     - 使用 Snipping Tool 抓取 Inspect.exe 树与 App 对比图。

### 3.3 Linux 验收清单

> **状态**：已由 CI 自动验证（Job: `fixture-a11y-linux`，见 `.github/workflows/computer-use.yml`；测试脚本：`computer-use/scripts/a11y/dump_atspi.py` 与 `check_fixture_tree.py`）。

- **工具**：
  - AT-SPI 2.0 / D-Bus（`at-spi-bus-launcher`、`gir1.2-atspi-2.0`、`python3-gi`）
  - Xvfb 虚拟显示与 Openbox 窗口管理器
- **CI 执行命令**（Xvfb / AT-SPI 会话）：
  ```bash
  dbus-run-session -- xvfb-run -a bash -c '
    openbox &
    /usr/libexec/at-spi-bus-launcher &
    export GTK_MODULES="gail:atk-bridge" QT_ACCESSIBILITY=1 ACCESSIBILITY_ENABLED=1
    cargo build -p gcu-fixture
    ./target/debug/gcu-fixture &
    python3 scripts/a11y/dump_atspi.py --output /tmp/fixture_tree.json
    python3 scripts/a11y/check_fixture_tree.py --input /tmp/fixture_tree.json
  '
  ```
- **检查项目**：
  1. **AT-SPI 角色与断言**：
     - `name_field`、`multiline_field`：Role 为 `ATSPI_ROLE_TEXT` 或 `ATSPI_ROLE_ENTRY`。
     - `password_field`：Role 为 `ATSPI_ROLE_PASSWORD_TEXT`（或具有受保护属性）。
     - `agree_checkbox`：Role 为 `ATSPI_ROLE_CHECK_BOX`。
     - `radio_group`：3 个单选按钮，Role 为 `ATSPI_ROLE_RADIO_BUTTON`。
     - `combo_box`：Role 为 `ATSPI_ROLE_COMBO_BOX`。
     - `slider_field`：Role 为 `ATSPI_ROLE_SLIDER`。
     - 按钮类（计数器、发送、提交、打开弹窗）：Role 为 `ATSPI_ROLE_PUSH_BUTTON`。
     - 标签页（全部控件、画布网格、虚拟列表、事件日志）：树中存在且逐字匹配。
     - 虚拟列表：Role 为 `ATSPI_ROLE_LIST` / `ATSPI_ROLE_SCROLL_PANE`，仅视口可见行（约 10–30 行）进树，禁止 2000 行全量加载。
     - 画布区（`grid_canvas`）：Role 为 `ATSPI_ROLE_IMAGE` / `ATSPI_ROLE_CANVAS`，子节点数量严格为 0。
     - 模态弹窗：打开后出现 Role 为 `ATSPI_ROLE_DIALOG`，拥有 `MODAL` 状态标记。
     - 中文与 Emoji：18 组双语与 Emoji 标签逐字符完全匹配。
  2. **CI 凭证与 Artifacts**：
     - CI 生成 `/tmp/fixture_tree.json` 完整 AT-SPI 结构树。
     - 失败时自动上传 `/tmp/fixture_tree.json` 与 `/tmp/fixture.log` 为 Workflow Artifacts。
