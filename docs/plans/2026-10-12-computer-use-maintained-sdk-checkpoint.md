# Ubuntu22 维护版 SDK、实际装载与 ABI 门禁

## 结论与范围

active / partial。原 PW1.0.5 兼容性试验不再作为下一步产品接线的候选。
固定来源的 PW1.6.9 + Debian1.6.9-2 安全补丁已在真实 Jammy/glibc2.35
用户态重新编译；动态PW/静态EI、原/迁移目录实际模块装载及八项原生负例通过。
最终回归 11 + 15 + 22 = 48 项，0 failed / 0 skipped；仅构建/SDK合同。

产品 Rust/前端代码本批未改，未构建完整App、未连接portal、未采集或输入桌面。
Ubuntu22 独立 CI job 已写入，但没有新增提交/push，**远端 CI 未运行**。
release.yml 未接入 SDK，仍保留 Ubuntu22 底线；不能把此门禁当作可发布。
三平台/四surface、原ACP/MCP/真人/picker、installed、UI UX、模型矩阵、
同最终候选12h和D00–D31完整范围仍开放。

用户唯一预览仍app144/PID19768，creation UTC `2026-10-11T21:53:09.3899735Z`。
本批不重启预览、不改home/login、不发送909字任务草稿、不自动授权或选文件。
前一轮已实看首页和三内嵌页面；页面正常不等于CU模型任务验收通过。

## 来源与安全/许可边界

来源：Debian官方pool的 `pipewire_1.6.9-2.dsc`（已存证）、orig归档和Debian delta。
固定HTTPS来源与SHA256，不把HTTPS/hash核验描述成GPG签名验证。

| 归档 | SHA256 |
| --- | --- |
| `pipewire_1.6.9.orig.tar.gz` | `a0012db8aaca4448fd205035e07d264235e4a4fe50713085fc4a6899e738a298` |
| `pipewire_1.6.9-2.debian.tar.xz` | `9ee24622dfd907c8f1283328632156d43d23013c7ce62b04ea8ed6991a1ae65c` |

按[Debian安全跟踪](https://security-tracker.debian.org/tracker/source-package/pipewire)
已读取的修复版本信息，CVE-2026-14324/CVE-2026-5674分别已在1.6.8/1.6.6修复；
CVE-2026-14330需要1.6.9-2 delta，不能仅使用orig1.6.9。
builder在校验两个归档后，仅抽取已知补丁，先dry-run再以fuzz=0实际应用。
未复现漏洞；这不是“所有依赖绝无安全风险”的承诺。

上游LICENSE声明整体MIT，例外为SPA ALSA/LGPL及jackserver/GPL2。
当前runtime只复制已知客户端清单，不含这两个模块，也不安装系统服务。
保留PW COPYING、LICENSE、原补丁、来源说明，以及实际静态EI1.5.0的COPYING；
四份许可/补丁内容固定hash，缺失或改动拒绝。

## 实际构建、装载和负例

独立root仍 `/var/tmp/grok-cu-jammy-sdk-20261012-01`；WSL Debian只提供内核。
编译在私有PID/mount namespace中的Ubuntu22.04用户态执行，GCC11.4/Meson0.61.2/
glibc2.35。patchelf仅装进该root；没有修改Windows/WSL全局库或代理。

证据目录均在 `tools/computer-use-probe/.run/`：

- `linux-jammy-sdk-20261012-02/`：来源归档、dsc、首个169-01真实构建日志。
  该运行副本早于最终LICENSE复制逻辑，不作为最终候选，保留原证据不补写成绿。
- `linux-jammy-sdk-20261012-03/`：新169-02实际build.log/inputs、最终contracts-03.log/
  inputs、native-faults-02.json及清理记录。原构建session50008实际exit0。
- `linux-jammy-sdk-20261012-04/`：最终checker冻结inputs、smoke.log和inventory。
  原实际命令chunk146afa exit0；不是仅观察日志末行。

最终SDK：`/work/pipewire-sdk-169-02`；EI复用 `/work/libei-sdk`，没有覆盖。
最终runtime：`/work/pw169-runtime-check-02/runtime`。
SDK和迁移runtime均实际装载私有主DSO、SPA support和七个客户端模块；
dladdr/realpath/dl_iterate_phdr验证路径，PW/EI contexts创建后退休，不连接remote。

runtime含10个ELF和6个config/notice文件。只复制默认client.conf所需模块、
SPA support/audioconvert，去掉上游模块的绝对build RUNPATH（只改副本）。
逐件SHA、ELF架构/SONAME/依赖/GLIBC审计；禁止symlink、特殊文件、额外模块、
dynamic EI、残留RPATH/RUNPATH。所见最高GLIBC需求2.34，小于baseline2.35。
这不是完整Rust App及其它打包依赖的ABI结论。

八个实际负例：缺PW/坏PW分别127，缺support为3，缺protocol/client-node为4，
错prefix/错version/加载旧1.0.5 SDK均2。只接受预期exit code，signal/crash/timeout
不能冒充正常拒绝；原正向runtime hash未变。CI默认运行前七项，旧SDK对照为本地额外项。

最终smoke ELF：`963ad6f960536b3f33fa0ad4507672e0596e65f5e70f4d955014efa32416ee83`。
前后两次smoke ELF hash相同，负例与最终正向绑定同一实际程序；DT_NEEDED仅PW/libc，
显式链接EI.a，其hash仍 `c6b4019a0899ed309b737567432c70741766409a849391f6c5ecfd8fab9d1849`。

最终输入hash：

| 文件 | SHA256 |
| --- | --- |
| builder | `695042ae80e3d289ade9b89d4373042cc4b318cf9263f878646543bed810df7d` |
| runtime checker | `ea7ea379f2a6893975ce55a9e3e35c4ffb75d4239ff0f11701e56f5740639445` |
| auditor | `af380f55f7701025960da212539db88e8c4530d2619057baf8d255f05ee0fdb4` |
| native C | `da7a2302770c7dfbe08b145fbd6dc1c54e6a80b342a1a0bdd470654cce0891b5` |

上游仍有-Wclobbered warning，不写成零warning。原两个launcher的引用/缺目录拒绝
不是SDK测试结果；前者说明及原日志保留，修正后用新输出重跑，不覆盖失败日志。
外层WSL git不能解析Windows worktree的gitdir；diff/质量检查使用Windows原工作区，
不把WSL最后一个命令exit0误认成git检查通过。

## 可复用入口与 CI

```bash
bash scripts/build-computer-use-pipewire.sh /absolute/NEW-PW-SDK
bash scripts/build-computer-use-libei.sh /absolute/NEW-EI-SDK
bash scripts/check-computer-use-pipewire-runtime.sh \
  /absolute/PW-SDK /absolute/EI-SDK /absolute/NEW-OUTPUT
python3 tools/computer-use-probe/jammy-sdk-runtime-faults.py \
  /absolute/OUTPUT /absolute/NEW-FAULT-OUTPUT
```

构建/校验入口拒绝覆盖已存在输出和canonical边界逃逸。pkg-config flags以shlex解析
为argv，不eval，路径空格不按shell单词强拆；空格SDK尚未实编验证，不据此宣称实测通过。
旧Jammy smoke/build/compile辅助入口已跟随维护版本改参数，旧.log/.run副本不改写。
CI `linux-native-sdk-jammy` 固定ubuntu-22.04/2.35，保存日志/清单，不发布runtime资源。
本地YAML解析/语法/diff/final quality通过；App+Workbench13573、≥1000文件80不增长。

## 生命周期与例行清理

原build/native/contract/faults句柄均已终态；清理前准确 `/proc/*/root` 无该root live owner。
只删除两个已结束faults运行时的152个regular files，保留各results.json和全部日志/hash。
逻辑33,455,536 bytes；WSL ext4空闲即时观察增加34,119,680 bytes（约32.5MiB），
并发磁盘活动不可独占归因，Windows C盘VHDX实际回收量未知。
记录：03的removals.tsv/cleanup.json，重新生成使用当前正向runtime和faults.py新输出。

169-01/build含SONAME symlink，清理在删除前拒绝，整项暂缓，未绕过或递归删。
保留当前169-02、static EI、原/迁移正向程序、来源/失败证据、工具链、app144/home/login
及正在等待真人验收的两台本机服务。没有停止无关进程。

## 下一步（完整交付范围不变）

1. 产品私有loader接线：在ELF预装载前固定资源目录；host-WebKit re-exec不能把
   系统PW0.3.48置前。缺资源/身份异常明确报错，不静默fallback，不污染全局环境。
2. Jammy完整Rust App编译、相同实际依赖清单/notice与AppImage/deb/rpm资源/依赖/安装。
   只有SDK库通过不能证明完整App、干净安装或升级修复回滚。
3. 原portal/真实capture/EI输入退出以及X11/Wayland各自验收；不自动批准权限，不用
   浏览器/XWayland结果代替Wayland。
4. Windows普通app144原真人CU/picker链、Chrome/Edge installed、DPI/输入；
   macOS原生ARM/Intel；四surface、UI UX、真实模型矩阵、同最终候选12h和D00–D31。
