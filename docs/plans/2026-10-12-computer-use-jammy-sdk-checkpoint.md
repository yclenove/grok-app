# Ubuntu22 私有 SDK：实际编译与迁移 smoke

## 状态

active / partial。新增私有 PipeWire builder、五项 guard 回归与 Jammy 验收辅助程序；
未修改产品代码、提交、push、PR或发布。用户仍使用app144/PID19768；本批复核首页、
收起验收侧栏，草稿未发送，不自动授权/选择文件。

这不是完整Linux App、安装态、portal、真实采集/输入或跨平台交付通过。
PipeWire1.0.5仅为兼容性候选，未应用Ubuntu debian.tar的维护补丁，
不得把来源hash正确当作最终发行版本/安全审计已通过。

## 环境、来源与实际证据

证据根：`tools/computer-use-probe/.run/linux-jammy-sdk-20261012-01/`。
借用现有WSL Debian13内核，在私有PID/mount namespaces和独立Ubuntu Base22.04.5
chroot编译；实际GCC11.4/Meson0.61.2/glibc2.35，不借Debian glibc2.41冒充Jammy。
root=`/var/tmp/grok-cu-jammy-sdk-20261012-01`。依赖仅装到该root，policy-rc.d拒绝
服务启动，namespace挂载随原命令退出。未启动Docker/VM/SSH，未改代理、系统库或权限。

来源均HTTPS、固定SHA256、安装前核验；builder拒绝已有prefix：

| 来源 | SHA256 |
| --- | --- |
| Ubuntu官方Base22.04.5 amd64 | `242cd8898b33ea806ef5f13b1076ed7c76f9f989d18384452f7166692438ff1a` |
| Ubuntu官方PW1.0.5 orig archive | `d5e5f3d0b8460e5711c1571c500156fd61768ba55a062eaeb94356abdb955a56` |
| Debian官方EI1.5.0 orig archive | `da1fba92daccd0667bc46c3ee952d4ae8cfc6bdb4c0bb4d34df26528fb240618` |

Ubuntu `.dsc`（source checksum）和全部原日志已保留。HTTPS/hash核验不冒充GPG签名审核。

## 首败与结果

- 原session83085 exit1：裁掉audioconvert后，上游模块引用缺失audioconvert_dep。
  保留默认客户端必需依赖，在新prefix重建，未修改上游源码绕过。
- 原session36972 exit1：上游使用shared_library，default-library=both不生成预期PW.a。
  builder明确动态PW、删除错误的PW静态要求；原任务要求的静态EI仍保留。
- 原session63865 exit0：新`/work/pipewire-sdk-03`与`/work/libei-sdk`实际构建完成，
  检查输出PW1.0.5/SPA0.2/GTK3.24.33/EI1.5.0。日志为build-03.log，前两失败未改绿。
- 原native smoke chunk f00478 exit0：原目录和迁移后的lib/share树都创建并退休PW/EI
  contexts。dladdr实际返回各自PW路径；显式指定SPA/module/config目录，未连接默认或
  系统PW socket/portal，未访问用户桌面。没有逐个证明所有模块的实际映射。
- C linker显式引用libei.a，不仅使用pkg-config --static（后者不强制选择静态库）。
  ELF DT_NEEDED仅PW和libc，没有libei动态依赖。
- readelf检查两个目录所有实际PW `.so*` regular files，所见最高GLIBC_2.34；
  native runtime为2.35。不据此证明完整Rust App及其它bundled依赖的ABI。

最终builder及root实际执行副本hash均为
`60ad219666678794cab92582d188298d4f51808556264d63eabcc53439b27981`。
PW DSO=`33857d33c28b54f7cc891b502984b38ff94056343176b9622f4a640afd6d0ec3`；
EI.a=`c6b4019a0899ed309b737567432c70741766409a849391f6c5ecfd8fab9d1849`；
native smoke=`debe3ca89159459c29f8d754d55dc60e4028c541563c938fe2fce6187ac87ec3`。

新guard5 passed/0 failed（5dcb77、最终d75157）；既有runner/SDK contracts
22 passed/0 failed/0 skipped（40aac5）。新shell语法、diff、final quality通过；
App+Workbench13573、≥1000文件80不增长。未运行cargo或完整App suite，不借旧宽测试数字。

## 生命周期与清理

所有原构建sessions已exit，原smoke命令exit0。清理前准确/proc/*/root没有该chroot
的live owner。只删除SDK02/build内373个退休regular .o；逐件canonical path、
非symlink、hash与删除记录见removals.tsv/cleanup.json。

逻辑9,332,576 bytes；WSL ext4空闲即时观察增加10,194,944 bytes（约9.7MiB）。
并发磁盘活动不能独占归因；**不等于Windows C盘VHDX已经缩小**，主机回收量未知。
保留源码/原失败配置日志、当前SDK03/迁移树、Jammy工具链和已核验源包，以及app144/home/登录。
含symlink目录和当前依赖不递归删；再生成使用归档SDK02配置进入新prefix。

## 下一批实际顺序

1. 审计最终维护版本/安全补丁来源；不直接将当前兼容性候选接正式release。
2. 固定private DSO/SPA/config/NOTICE清单，补实际模块映射和缺失/损坏拒绝验证。
3. 接App私有loader，处理host-WebKit re-exec的LD_LIBRARY_PATH优先级；不污染
   系统、不依赖用户全局环境、不静默退到系统0.3.48。
4. Jammy完整Rust App构建及Ubuntu22 CI SDK/loader gate，release保持ubuntu-22.04。
5. 分别补AppImage/deb/rpm依赖/资源/许可证/干净安装、原portal授权和真实采集/EI输入退出。
   X11不冒充Wayland，缺portal能力如实报告。
6. 普通app144真人链、Chrome/Edge installed、Windows输入/DPI、Mac原生双架构、
   安装升级修复回滚、UI/UX、真实模型矩阵、同最终候选12h与D00–D31范围全部保留。
