# 0011 向后兼容的 auto 截图策略

状态：接受（P6-02，总控批准选项 1）
日期：2026-10-17

## 背景（事实 + 证据链接）
[P6-02](../../../docs/plans/computer-use-v2/06-roadmap.md#p6-02-auto-截图策略) 规定按树质量决定附图，且只有评测证明成功率不降后才能修改默认值。[03 §4.4](../../../docs/plans/computer-use-v2/03-architecture.md#44-截图) 当前默认附图；§11.1 的布尔参数不能表达主动选择 auto。总控批准向后兼容扩展，不修改宪法文件。0010 已被未合并的 builtin-browser-window-protocol PR 预占，本决定使用 0011。

## 决定
- `GetAppStateParams.include_screenshot` 从布尔扩展为 `boolean | "auto"`。模型可见 schema 为 `anyOf: [{type: "boolean"}, {type: "string", enum: ["auto"]}]`，`default: true`。参数序列化保留布尔/字符串输入形态；未知字符串、null、对象和数字被拒绝。
- 默认仍为 `true`；显式 `true` 请求同步且脱敏的图片，`false` 不采图、继续交付文本（包括原生不完整树），语义保持不变。
- auto 使用原始 canvas 角色占比、保留树可交互节点数及弹窗内节点数评估质量，背景控件不能补足弹窗的可操作信息。阈值通过 `QualityThresholds` 注入，未提供可交互节点阈值时保守附图；不写未经评测的生产阈值。
- auto 先采集并完成既有同步校验和脱敏，再决定是否附图，以发现树无法表达的像素变化。空 diff 按模型实际渲染内容判断；像素只比较 `RedactedImage`。未附图的采集结果不进入快照图片基线，不授予新的坐标观察。
- 原生树前后不完整、未知敏感区域、图树/几何不同步继续沿原有拒图路径返回 `handoff_required` / `stale_screenshot` / `stale_geometry`。auto 不能取消或绕过脱敏。Driver/Host 接口、Stop、5s 原生期限保持不变。

## 备选与为什么不选
- 只保留布尔：无法让模型显式试用任务卡规定的 auto。
- 立刻把默认改为 auto 或借用外部阈值：缺少同任务集成功率证据；保留 true，阈值待评测校准。

## 影响（对接口、测试、用户）
差异仅为上述工具参数类型/schema 与 core 策略；Rust 调用者使用 `ScreenshotMode::Explicit(bool)`。现有 JSON 调用无需修改。协议测试覆盖输入回显、默认值、严格 union 与完整 golden；`gcu-core --test auto_capture` 覆盖质量、动作附图、空 diff 像素变化、同步拒图和脱敏。auto 优化交付图片量，当前不声称减少原生采图耗时或提高任务成功率。

## 退出条件（回滚方式）
若评测失败，保留 true 默认及保守策略；可撤回 core 的省图决策，同时仍接受并回显 `"auto"`。彻底删除 union 必须先撤回支持 auto 的客户端/模型目录，再还原布尔类型与 golden 并运行协议回归。任何默认值或生产阈值调整须提交同任务集成功率不降的评测证据。
