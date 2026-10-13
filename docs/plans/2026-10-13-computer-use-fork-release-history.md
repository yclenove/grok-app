# fork 同版本号发行记录归档

2026-10-13 同步时发现 fork 和源仓库分别发布过不同内容的0.2.39/0.2.40。
主 CHANGELOG 保留源仓库已发布章节原文和唯一版本标题，避免更新弹窗/发行脚本
读到重复版本。以下完整保留 `origin/main@b58b97cd` 的两个原章节；它们是fork
发行历史，不是当前CU分支的发行完成声明，也不修改任何远端Release或tag。
本地Markdown安全修复保留在主CHANGELOG的Unreleased中。

## [0.2.40] - 2026-10-04

> **Highlight:** Linux ARM64 and Apple Silicon installers are on this GitHub Release.
>
> **中文 · 亮点：** 本版 GitHub Release 含 Linux ARM64 与 Apple Silicon 安装包。

### Fixed
- Linux ARM64 AppImage, .deb, and .rpm are published. v0.2.39 lacked them (`xdg-mime` missing on the ARM runner).
- The Apple Silicon .dmg is published. v0.2.39's macOS ARM job ran out of Node heap.

**中文 · 修复**
- Linux ARM64 的 AppImage、.deb 和 .rpm 已发布。v0.2.39 缺这三项（ARM 构建机没有 `xdg-mime`）。
- Apple Silicon 的 .dmg 已发布。v0.2.39 的 macOS ARM 任务 Node 内存不足。

## [0.2.39] - 2026-10-04

> **Highlight:** Linux ARM64 installers are on GitHub Releases.
>
> **中文 · 亮点：** GitHub Releases 提供 Linux ARM64 安装包。

### Added
- GitHub Releases include Linux ARM64 AppImage, .deb, and .rpm packages. Ubuntu 24.04+ aarch64.
- Settings can switch the composer between Built-in and Markdown.

### Fixed
- Custom channels connect again when the channel name and the model id differ. The app tells the CLI the channel name, which is how that CLI finds the key. (#1294)
- Sending one picture on Windows no longer shows that picture twice. The chat was counting the same file a second time. (#1284)
- Markdown editor no longer freezes on large link-heavy documents.

**中文 · 新增**
- GitHub Releases 现提供 Linux ARM64 的 AppImage、.deb 和 .rpm。面向 Ubuntu 24.04 及以上 aarch64。
- 设置里可以把输入框换成「内置」或「Markdown」。

**中文 · 修复**
- 通道名称和模型 ID 不同时，自定义通道可以重新连上。应用发给 CLI 的是通道名，CLI 靠它找到密钥。（#1294）
- 在 Windows 上发送一张图片后，不会再显示成两张。同一文件之前被算了两次。（#1284）
- Markdown 编辑器在大量链接的文档上不再卡死。
