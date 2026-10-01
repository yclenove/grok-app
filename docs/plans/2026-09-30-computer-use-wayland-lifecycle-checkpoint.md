# Wayland 静态画面与观察生命周期 — 2026-09-30

状态：**active / partial — not releasable**。上一轮像素呈现阶段有实际源码和测试证据，归类为 progress；本轮先验证上一轮 311 源冻结无漂移，再修改生命周期、原生夹具和测试，同样属于 progress。完整目标未缩小，不把观察超时当作进程终止，不把 WSLg 当作原生 GNOME。

## 已落地

- 去除“像素收到超过两秒就无画面”的错误耦合。保留最后有效像素及其真实本地交付时间；未收到新帧时，不伪造捕获时间、序号或新像素。原生 stream invalidation、GAP/corrupt/empty buffer 和撤权仍清空画面。
- 将模型动作授权与像素交付分离。`PortalOptions.observation_timeout` 默认 60 秒，必须在 `(0,300s]`；这是显式、可配置的模型响应预算，**不是图像新鲜度保证**。`ObservedFrame::issued_at/expires_at` 暴露原时间界限，呈现、缩放、后续帧均不能续期。
- 每次新 `observe()` 取代旧的 outstanding 观察，即使图像没有变化；只读 `frame()` 不产生新授权。真正 native dispatch 在持有 EI gate + frame store 锁时验证 epoch/owner/generation/期限，并在 FFI 前消费凭据；原生错误也不能重放。观察序号溢出 fail closed，不影响另一个 owner。
- 原来的两秒 **EI 队列执行期限**保留，未放宽。geometry、crop、旋转、resize、clear/恢复 ABA、Stop 和异步撤权仍不允许旧凭据重新生效。

必须区分：一个仍连接且 Streaming 的 producer，可能画面静止，也可能无响应。缓存、一次新观察或 PipeWire round trip 都不能证明是哪一种。Host 仍须绑定模型回复、执行自己的取消/观察失效、权限/目标/意图检查和动作后验证；没有以延长 TTL 声称解决整个端到端问题。

## 当前证据

目录：`tools/computer-use-probe/.run/wayland-lifecycle-20260930/`。
相对上一轮 311 源，只变更 Wayland crate 内 **7 个文件**，没有新增源文件或依赖；Cargo.lock 原 **693 包**版本/checksum 不变。

- 新 C 夹具通过自己持有的 stdin 接收命令，实际停止触发 PipeWire buffer，不是持续发送相同像素。进程仍 live、source stream 为 Streaming、实际 daemon node 为 running；两段各 2.2 秒等待前后，图像 bytes、序号、交付时间严格不变。
- 第一段等待后才签发新观察；第二段模拟模型处理延迟，随后真实 EI 收到指定像素中心事件。过期测试则明确配置两秒观察预算，在源仍持续出帧时验证旧票仍过期；没有删除过期负例来换通过。
- 新 native 用例还验证 supersession、真实 Header GAP、Chunk CORRUPTED、零长度 chunk 的清空与恢复 ABA、四条精确 `(500,500)` EI motions，以及实际杀掉自己 source 后释放按住的键、关闭两个 native consumer。非法路径没有额外 key/motion。
- 同一个最终 joint-feature Wayland executable 三轮（threads 1/4/4）各 **56 passed / 0 failed / 0 ignored**，含 **12 个 native tests**。不是实际 Grok 推理调用或原生桌面应用效果证明。
- X11 单测 **49**、owned Xvfb **19 PASS 组**、GTK/AT-SPI **41 PASS 组**重跑通过；联合全 targets Clippy `-D warnings`、Windows App library offline/locked check、workspace fmt、tracked diff whitespace check 均通过。
- 三轮测试与立即归档在同一 Linux 调用内完成：**84 个最终必需 native 目录**完整，加上早期测试共 **86 个**；缺失 0，owned fixture 进程残留 0。只操作 UUID 归属的私有 bus/socket/process，无用户桌面动作。
- 五个归档 executable 测试前后 hash 相同；静态 libei 1.5.0 未变化、ELF 无动态 libei。早期严格 Clippy 的 `byte_char_slices` 失败日志保留，改为 byte string 后重新冻结并通过，未禁用 lint。

Wayland test executable SHA256：
`a64e966256d7cf05f884f2f8a6e72d7f8a98c88b1d7160b37a5088fd7494c1fb`

上一阶段 receipt 原字节另存，SHA256：
`5c83393c0859c24f87751029df8496e15358cf0ce96bd5c133ff956dba3ed268`。
本轮 `receipt.json` 保存源、文档、日志、实际 executable 与前驱链；Node 和独立 Python 按文件长度/hash 校验，不能把自生成的绿色状态字段当功能完成依据。

## 继续推进，不宣布最终版

下一项是 Host 图片编码和观察 ID/model-response 绑定、权限/目标/意图及复合动作/效果验证；随后须在真正 GNOME 上验证静态/遮挡/无响应 producer、内容与 EI region 配对、resize 时序，以及 parent/restore UX、IME、clipboard、取消恢复。私有 IPC 夹具不能替代这些。

**App 仍拒绝 Wayland，native_wayland 未开启**。本轮没有改安装态/注册表，没有 commit/push/tag，没有远端 CI、完整前端/Core 重跑、真实 Grok E4、签名安装/更新验收或最终候选 soak；不得推断这些已经完成。

完整交付范围保持：Windows x64；macOS arm64/x64；Linux X11 **及原生 GNOME Wayland**；Desktop/managed browser/existing Chrome+Edge/App WebView；App/ACP/MCP；完整输入/中文 IME/clipboard/取消恢复；签名干净安装/更新/修复/回滚/卸载；原生窄窗/DPI/权限 UI；真实 Grok E4；**同一最终冻结候选 12h active soak**。全部逐项验证前，不调用 complete，也不把仍可开发的工作标为 blocked。
