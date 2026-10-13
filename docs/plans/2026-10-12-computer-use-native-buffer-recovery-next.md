# D22 Windows 下载私有 buffer：联合收尾与崩溃恢复

完整交付范围保持 active / partial，不把本批 Windows 下载工作替代三平台、四 surface、installed 升级回滚、UI UX、真实模型矩阵、冻结12h与D00–D31。用户当前唯一预览已更新为 app144/PID19768并实看首页/关于/历史消息及连接“就绪”，保留原home/login；该启动检查不冒充真人CU全链。

## 当前源码与实际缺口

最新[source144真实journal异常组合](2026-10-12-computer-use-native-journal-faults-checkpoint.md)：九项实际Host维护与文件后置条件通过，39原held owners最终退出。第4项的新run并存、破损/替换、readonly/receipt替换和重复维护局部已验；PID replacement、查询 unavailable、独立ACL、其它D22及真人全链仍开放，不把局部结果写成完整D22或交付完成。

source144 [正常退出取证检查点](2026-10-12-computer-use-normal-exit-checkpoint.md)：idle/held/before-start同原生候选三批业务及30秒原owners gate通过，未修改产品退出策略；历史138/139延迟根因仍开放。fresh定向153/三strict通过，app144普通fresh包/450件ZIP回读与启动实看已完成，原133及14 observed owners正常退出；第4项其余真实crash故障组合及真人全链继续，不靠无限复跑销掉历史失败。

最新状态见[持久journal与真实App崩溃检查点](2026-10-12-computer-use-download-journal-checkpoint.md)。source139派发前持久归属/原内核owner见证及显式Host维护已接生产；source140修复维护专属错误分类，source143 fresh153/三strict通过。

source142真实1MiB/held Wry/App force-kill、原browser live拒绝/文件hash不变、原owner退出后维护及重复维护通过。138/139正常退出30秒gate均失败，后来缺失不回填绿，根因仍开放。当前用户仍app133，普通fresh包/真人全链/其余原生故障组合和完整交付仍待办。以下136阶段文字是历史顺序，不再表示journal尚未接线。

`webview/download/windows/destination.rs` 在 Broker 长路径不能直接交给 WebView2 时创建随机 Temp 子目录。它保留 Broker 最终路径；139起必须先发布持久归属才派发，Lease Drop本身不清理。

联合收尾已经在source134真实red后由source135修复，见[检查点](2026-10-12-computer-use-native-buffer-retirement-checkpoint.md)。136定向回归通过；实际五项long-path业务完成但原owner退出门禁失败，独立error-unwind虽通过却未使用private buffer。先强化真实held/private-file与超过8秒caller-unknown证据，查清退出延迟，再接持久记录。不能把局部测试或后来退出当作完整原生门禁通过。

source136已完成两真实完整1MiB/private file（324/322字符）与held Wry的callback-first、原8.013秒caller-unknown验收；其后第2项派发前持久归属及第3项显式维护已实现。下一可执行工作是正常退出的准确kernel/COM角色诊断及第4项剩余故障组合验收，不通过无限复跑推迟独立可开发项。

## 顺序与关闭证据

### 1. 原调用方和回调的联合收尾

- 保持原取消、八秒 grace、unknown 不重放、暂停/停止/关闭 gate。不能因清理变慢改为业务成功。
- Caller 结束且原 Wry 终态已收束，或准确原 native exit 后，才释放这一项 expectation。未派发的拒绝仍可立即清理。
- 故障清理只由实际删除原 registry entry 的一次退休操作执行，不能让重复 terminal/caller 抢占第二次清理。
- 长路径真实文件回归验证 native-first、caller-first、callbacks-first、原准确退出、另一项不受影响。保留首次真实 red 证据；单元模拟回调不冒充真实 COM 或 force-kill 验收。

### 2. 持久化原归属，必须在原生派发前完成

- 原 browser identity 在准确 `ICoreWebView2` 安装阶段捕获，保留原内核 handle / creation witness；不在回收时根据名称、URL、复用 instance 或新 PID 猜旧 owner。
- App 独立 root 下维护有界记录，包含 schema、随机 transaction、原 App 与原 browser identity、原 native instance / expectation、准确 Temp parent 与 buffer directory 身份。
- 每项只允许一个 `download.bin` 与不可变原归属 marker；journal 的名字和内容必须相互匹配。记录中不写 URL、Cookie、token、正文或用户文件内容。
- 拒绝相对路径、遍历、symlink/junction/reparse、hardlink、替换目录、跨 App root、未知字段/超限/破损记录。借鉴现有受保护祖先目录 pins、按 handle 取卷/文件身份、create-new 与 flush；不新增临时放宽校验或失败静默跳过。
- 完整记录失败即下载未派发；部分记录保留为维护诊断，不能事后补一个自称属于原操作的记录。

### 3. 显式维护与可恢复失败

- 只遍历当前 App 的完整 registry records，不扫描系统 Temp 名称前缀。历史无 journal buffer 无权自动清理。
- 原 App / browser 任一 live 或查询 unavailable 即保留。不能用 App PID 消失替代仍活的 WebView browser / 原占用退出。
- 对准确已退休 owner，锁住该事务和祖先；验证 registry/marker/卷/文件身份、允许子项与无写入占用后，只删准确文件和空目录。失败保留记录、短错误和显式重试入口。
- 接到现有 Host 维护/清理入口，不向模型开放清理、路径或授权工具；不要在普通 status、启动或 feature off 中偷偷做宽清理。
- 维护不接触新 run、另一App home、borrowed tab、immutable runtime seed 或用户原文件。重复维护要无副作用。

### 4. 原产品 Native 验收与发布边界

- 固定新 source / App artifact，通过原 Wry+COM 路径验证长路径成功、Pause、Stop、导航/关闭与 original grace。
- 独立原进程 fixture：kill App 但保留原 browser 时拒绝；原 browser 真退出后准确维护；PID/目录替换、半写 journal、拒绝访问、重复维护、新 run 并存都要有文件后置条件。
- 用原 held handles / tool sessions 证明每个 fixture owner 退出。验证实际删除与磁盘净增量分别记录，保留证据及当前可用预览。
- 最终普通包并实际看窗口后再交付用户验收；本批 synthetic host/callback 或 libtest 通过不能解除正式 app-webview descriptor gate。

## 不变的待办

app133 授权状态修复后的真实模型/真人批准/picker 重验尚未完成。原132请求已过期、零效果，不复活，不自动重发。D22 其余 Quit/ACP/worker/安装 watcher/sleep-resume 与其他平台条目仍按完整剩余计划逐项关闭。
