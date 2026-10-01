# macOS 进程出生身份与输入边界检查点

本机日期：2026-09-27，Asia/Shanghai。
接续 [单窗口截图检查点](2026-09-27-computer-use-macos-capture-checkpoint.md)。
完整 Computer Use 最终版目标仍为 active；未 commit、push、PR，未替换或关闭用户 App。
R17 UI/UX 重设计工作保留，本批不把后台安全改动冒充新的界面验收。

## 已复现的缺陷

旧 `mac:<pid>:<wid>` 只识别数值 PID/窗口号。测试替身在保留窗口号、标题、边界和像素大小
的情况下替换进程出生时间，旧授权仍被接受；截图期间替换进程仍能发布图像，按下之后
会向新进程发送 release，进程身份未知也仍可被发现。

先加生产调用链回归，实际得到 **9 passed / 4 failed**，保留 `identity-red.log`。
第一次修复后 16/16；继续检查事件配置后的边界和目标资格一致性，新反例为
**16 passed / 2 failed**，保留 `dispatch-boundary-red.log`。没有删掉失败或放宽断言。

## 本批源码改动

- 新 `macos_adapter/identity.rs` 通过公开 libproc `proc_pidinfo(PROC_PIDTBSDINFO)`
  获取完整出生秒/微秒，校验返回结构长度、实际 PID、存活状态和时间字段；未知或部分结果
  拒绝继续，不以 PID、进程名、标题或零值兜底，也不将 Host 自身作为目标。
- 目标 ID 改为 `mac:<pid>:<wid>:<birth-seconds>:<birth-microseconds>`；旧三段 ID
  明确失效，必须重新发现并授权，不静默升级。完整时间保存在字符串；数值 lifecycle/geometry
  revision 限于 JavaScript 安全整数，但摘要不代替完整原生身份比较。
- 发现时重新查询窗口；liveness、截图前后、坐标映射、遮挡检查及事件配置完成后均重查
  进程出生身份。发现与存活检查使用同一窗口资格条件，不因新快路径放行原来排除的窗口。
- 截图期间进程变化不发布图像；新模型捕获失败也不保留旧帧。进程替换后，旧目标、旧
  geometry revision 和旧 snapshot 都不能用于替代进程；正常的新发现/新帧路径仍可用。
- mouse-down 后身份改变时不向替代进程发 mouse-up，保留未知输入占用；Stop 不伪造 idle。
  目标列表改用与截图相同的 RAII 所有权清理。

## 回归证据

日志根：`tools/computer-use-probe/.run/macos-identity-20260927/`。

- 纯截图/身份合同 **12/12**：`core-frame-green.log`。
- 生产 Rust 适配器 + 系统 FFI 替身 **18/18**：`dispatch-boundary-green.log`。
  覆盖秒/微秒变化、旧 ID、部分/失败/错 PID/僵尸/非法时间、枚举中替换、截图中替换、
  首个事件之前替换、按下后替换、正常重新发现及之前的截图/取消/所有权回归。
- 严格 `cargo clippy -p grok-computer-use-core --all-targets --offline --locked --target-dir target-cu -- -D warnings`
  exit 0：`clippy.log`。
- 最终串行全量 **Core 531/531（109.98 秒）+ FFI 18/18**，终态 exit 0：
  `core-and-ffi-final.log`。使用之前的独立 Windows current seed，不改用户 App/资源。
- scoped `rustfmt --check` 和六个源码文件空白检查通过；`format-final.log` 记录 exit_code=0。
  六个源码文件、全部测试日志及四份 Apple 公开头文件的 SHA-256 记录于 `source-receipt.json`。

**这些是 Windows 上执行生产 Rust 调用链的 FFI doubles 测试，不是 macOS 真机证据。**
测试替身独立按公开布局写入字节，不复用生产结构体；这仍不能证明真实 SDK、链接、系统
权限、真实进程重用或真实输入效果。ImageIO 替身仍只是 transport sentinel，不是原生 PNG。
CI 原有非 macOS contract 步骤会包含新用例；未推送，也未执行远端 CI。

## 未闭合边界与完整范围

1. 同一活进程内的 window ID 重用、AX 元素实例和 exec 转换尚无完整生命周期 witness；
   出生时间不能冒充窗口实例身份。必须继续实现并原生验证。
2. 身份查询与 `CGEventPostToPid` 之间不是原子操作；返回仍只表示 queued/applied-unverified，
   没有证明应用已处理或原生物理完成。即时 Stop、原生队列确认及不确定 release 的恢复仍待完成。
3. macOS Apple Silicon / Intel 真实编译、权限流程、AX 树/语义/完整输入、ScreenCaptureKit、
   签名资源、安装/更新/回滚及独立原生 fixture 仍是交付要求。
4. Windows/浏览器退出长尾、WebView 运行中取消、X11 完整文本与恢复、GNOME 原生 Wayland、
   正式浏览器扩展、完整重设计 UI（包括原生窄窗/缩放/权限矩阵）、真实模型 E4 和冻结
   12 小时 active soak 仍开放。不得把本批 18 项绿色结果替代整个目标验收。

## 公开 API 核对

直接读取 Apple 开源主仓库，未读取账号或凭据：

- `https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/sys/proc_info.h`
  — `proc_bsdinfo` 完整布局、`PROC_PIDTBSDINFO = 3`。
- `https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/sys/param.h`
  — `MAXCOMLEN = 16`。
- `https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/libsyscall/wrappers/libproc/libproc.h`
  — `proc_pidinfo` 公共签名。
- `https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/sys/proc.h`
  — `SZOMB = 5`。

最初尝试的 `Libc/main/libdarwin/libproc.h` 返回 404，不作为成功依据；没有采用私有的
process unique-identifier flavor。上述头文件核对仍不等于本机有 macOS SDK。
