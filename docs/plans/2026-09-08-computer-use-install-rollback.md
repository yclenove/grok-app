# Computer Use 安装 / 更新 / 回退（草案）

功能默认关闭。未通过三 OS 原生验收前不要随正式安装打开。

## 本机（Windows x64）

1. 使用当前开发标识运行：`pnpm dev`（`com.grokapp.desktop.dev`），设置 `GROK_APP_HOME` 指向独立目录，不要覆盖正式安装。
2. Settings → Extensions → Computer Use，打开开关后才会注入会话 MCP。
3. YOLO / acceptEdits 不会授予桌面控制。
4. 回退：关闭该开关并重启会话。删除 `%APPDATA%/grok-app/computer-use/` 下的 lease 文件。不要改 `~/.grok`。
5. 任务轨迹只在进程内 Broker 中保留；重启 Host 不会重放键鼠，也不会恢复旧 snapshot。

## Windows 安装包

系统 PATH 无 `makensis`，也没有 `C:\Program Files (x86)\NSIS\makensis.exe`。

Tauri 缓存里有 NSIS 3.11：`%LOCALAPPDATA%\tauri\NSIS\makensis.exe`（`/VERSION` → `v3.11`）。这仍不是 GitHub Release。

复现命令（本机）：

```
set PATH=%LOCALAPPDATA%\tauri\NSIS;%LOCALAPPDATA%\tauri\NSIS\Bin;%PATH%
set CARGO_TARGET_DIR=src-tauri\target-nsis
pnpm exec tauri build --target x86_64-pc-windows-msvc --bundles nsis --no-sign --ci
```

本机已打出未签名包（**不是 GitHub Release**）：

`src-tauri/target-nsis/x86_64-pc-windows-msvc/release/bundle/nsis/Grok_0.2.33_x64-setup.exe`（约 16.6 MB）。

`cu_probe` 已挪到 workspace 包 `src-tauri/cu-probe`，NSIS 模板不再 File extra cargo bins。2026-09-08 本机新包 `Grok_0.2.33_x64-setup.exe`（16.4 MB）：`7z l` 只有 `grok-app.exe`，**没有** `cu_probe.exe`。不要对 D: 正式安装执行 `/S`。

### 本机测试目录安装（非正式安装）

正式安装在 `D:\Users\Administrator\AppData\Local\Grok`（`grok-app.exe` sha256 `0234ED2B…`，含用户 `start-with-proxy.cmd`）。不要对它执行 `/S`。

已执行：

1. 备份 HKCU `Uninstall\Grok` 与 D: 目录到 `%TEMP%\grok-cu-nsis-test\`。
2. 临时删 HKCU 卸载项，避免安装器链式卸载 D:。
3. `7z x` 解包：`grok-app.exe` 50.3 MB + 夹带 `cu_probe.exe` 1.2 MB。
4. `Grok_0.2.33_x64-setup.exe /S /D=%TEMP%\grok-cu-nsis-test\install`：写入 testdir，并**拉起 testdir `grok-app.exe`**（当前宿主因此重启）。
5. 二次 `/S` 更新：**not_run**（会再杀宿主）。
6. testdir `uninstall.exe /S`：**not_run**（当前进程就是 testdir `grok-app.exe`）。
7. HKCU 已从 `uninstall-grok.reg` 写回 `InstallLocation=D:\Users\Administrator\AppData\Local\Grok`。D: hash 未变，`start-with-proxy.cmd` 仍在。

证据：`tools/computer-use-probe/.run/nsis/evidence.txt`（gitignored）。

testdir 残留（TEMP，可在退出 testdir 宿主后手工删）：`grok-app.exe` / `cu_probe.exe` / `uninstall.exe`。

回退产品功能：关闭设置开关并重启会话；删除 `%APPDATA%/grok-app/computer-use/` 下 lease。不要改 `~/.grok`。

## 其他 target

macOS arm64/x64、Linux x64（GNOME Wayland 与 X11）安装包与原生回归：**not_run**。交叉编译不能当作原生通过。

## Sidecar

- Playwright worker：`tools/computer-use-browser/`（独立 package，不进根运行时）。
- 合成探针：`tools/computer-use-probe/`。
- Cua Driver：不是根 runtime。私有 gitignored clone 仅 P0 探针；发行不要捆绑 `cua-driver.exe`。
