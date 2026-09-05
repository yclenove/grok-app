# M6：独立 Runtime 更新、优化与 CEF 决策设计

**状态：** 设计完成；独立更新器、更新发布通道和 CEF 实验未实现/未执行。
**上游：** [总体设计](../2026-09-04-browser-rearchitecture-design.md)、[共用合同](00-contracts.md)。
**实施：** [M6 开发计划](../../plans/2026-09-05-browser-rearchitecture/m6-updates.md)。
**准入：** M1-R06、M1-D06；M2 诊断数据可辅助优化，不阻塞独立更新；Linux 仅扩展到已通过 M4 的行。

## 目标与非目标

- 在 M1 已验证的完整嵌入 tuple 基础上独立交付签名 Runtime 安全更新。
- 下载、解压、激活、回滚、Profile migration 与垃圾回收均可在崩溃点恢复。
- 保持活跃 BrowserSession 的 Runtime tuple 固定，更新不原地修改正在执行的目录。
- 使用独立签名域、key、manifest 和 release sequence，阻止正确签名旧漏洞版本的重放。
- 基于可复现数据优化容量与启动开销，保留 explicit capacity 和不驱逐活跃任务规则。
- 完成 CEF 的独立实验与 Go/No-go 决策，不预设其胜出。
- 不删除 Preview，不承诺 CEF 替换系统 WebView，也不把实验结果自动变成产品迁移。
- 不把 App updater 的 Tauri `.sig` 当作 Runtime manifest 验证或平台代码签名。
- 不允许首次使用 Agent Browser 必须联网下载；M1 embedded tuple/bootstrap 继续作为可验证入口。
- 不绕过 revocation floor、不回退最高 sequence、不原地降级 Profile、不 wipe/merge 用户数据。
- 不将用户关闭自动下载等同于可忽略安全 policy；过期/撤销规则仍按 Host 准入执行。

## 当前代码证据

| 位置 | 当前行为 | 本包边界 |
| --- | --- | --- |
| `src-tauri/src/updater.rs` | Tauri App update 状态和 install 成功后 teardown/relaunch | Runtime activation 不调用 App 重启/agent recycle |
| `src-tauri/build.rs` | 只读取 App updater 公钥与 endpoint 编译开关 | 新 Runtime trust root 独立配置与 attestation |
| `.github/workflows/release.yml` | 当前 App 发布和 App updater asset 装配 | 新独立 Runtime workflow 与 Draft 验证门 |
| `scripts/assemble-updater-manifest.sh` | Tauri `latest.json` 和 rolling App release | 不复用为 Runtime manifest 或反重放账本 |
| `src-tauri/src/app_update.rs` | App 版本查询 | UI 分清 App 与 Runtime 状态 |
| `src-tauri/src/browser/runtime/manifest.rs` | M1 前置产物，并非当前已有代码 | 复用 embedded tuple 验证与文件表 |

现有 release.yml 的自动公开行为不能用作本包安全门；M1-D 的 Draft/clean-runner 契约是实施前置。

## 模块与独立接口

| 新模块 | 单一职责 |
| --- | --- |
| `src-tauri/src/browser/updates/manifest.rs` | 组件签名、target、兼容区间与 sequence |
| `src-tauri/src/browser/updates/store.rs` | 扩展 M1 同一 security checkpoint，增加组件密钥元数据 |
| `src-tauri/src/browser/updates/installer.rs` | 有界下载、no-follow 解压、逐文件验证 |
| `src-tauri/src/browser/updates/activation.rs` | 组件编排；复用 M1 runtime/activation 的 canary、pointer/LKG |
| `src-tauri/src/browser/updates/policy.rs` | 复用 M1 policy 接口，组件更新信任与离线准入 |
| `src-tauri/src/browser/runtime/distribution.rs` | 扩展 M1 attestation，区分官方 App 身份、初始 embedded tuple 和组件授权范围 |
| `src/components/browser/updates/RuntimeUpdatePanel.tsx` | 检查、下载、取消、等待新会话、失败修复 |
| `.github/workflows/browser-runtime-release.yml` | build、sign、Draft、clean verify、publish、index |
| `experiments/browser-cef/` | 隔离实验，不能被生产 App 引用 |

```ts
type RuntimeReleaseInstruction = {
  schemaVersion: 1;
  component: "managed-browser-runtime";
  releaseSequence: string;
  instruction: "release" | "rollback";
  runtimeId: string;
  tupleDigest: string;
  targetTriple: string;
  appVersionRange: string;
  profileEpoch: number;
  issuedAt: string;
  expiresAt: string;
  keyId: string;
};
```

外层签名覆盖 domain separator、规范化 JSON 以及 archive/file manifest 的 digest。
`releaseSequence` 与 policy sequence 是规范十进制字符串：`0` 或无前导零的正整数；任意精度无符号比较。
generation/revision/fence 继续是共用 u32 number，禁止把 sequence 转 JS Number。
releaseVersion 是展示 semver，不用于防重放；相同 sequence 仅允许同 digest 的幂等重复。
序列语法上限 128 个十进制数字，范围内按任意精度处理；embedded/component 共用 M1 Runtime release 最高值。
`updates/store.rs` 不能建立第二份较低账本；比较和激活委托 M1 `runtime/sequence.rs` 与 `runtime/activation.rs`。
rollback 指令使用更高 sequence，精确列出目标 tuple 与允许 Profile epoch；旧 signed release 不能充当回滚授权。
Runtime 更新 trust root 与 M1 embedded manifest key、Managed policy key、Tauri updater key 分离。
独立签名角色不代表独立可降级的 tuple 权限；所有来源共同遵守同一 Runtime revocation floor。
key rotation 使用当前授权 root 签署的递增 root metadata，验证用途、有效期和旧/新 key 交接；拒绝任意 manifest 自带公钥。
本机 UI 无权改写可信 key、revocation floor 或安全账本。

## 官方资格与组件授权

M1 attestation 绑定初始 embedded tuple digest；M6 必须显式升级 distribution schema，不能直接要求所有新 tuple 等于该旧 digest。
officialApp 身份包括发行类别、source commit、App version、target 与 publisher；embeddedTupleDigest 只证明随包初始字节。
新增 componentAuthorization 由官方构建签入，包含信任域/key role、允许 target、Runtime protocol 与 App compatibility 范围。
无 componentAuthorization 的旧 App 只接受精确 embedded tuple；unsigned/community App 不能靠下载 manifest 或设置获得该授权。
有授权的 App 可接受不同 digest 的新 tuple，但候选仍须精确匹配 component trust domain/key、target、protocol、App/Profile epoch、签名、sequence 和当前 policy。
信任角色来自编译/签名的 App attestation 与受验证 key rotation，不能从候选 manifest 自带 key 升格。
同一 App 接受合法新 tuple、拒绝错误 key/target/protocol/兼容区间/撤销版本的 fixture 是 M6-01 硬门。

## 下载、验证与提取

1. Host 取得 signed index，验证 key、用途、时间、sequence、target 与 App 兼容性。
2. 只接受 https 且在签名允许的下载主机集合内的不可变 asset；重定向重新验证目的地。
3. 先计算磁盘预算，下载到独立 staging；网络中断只恢复同 digest 的验证范围，未知 partial 重新下载。
4. 验证 archive 字节数、SHA-256、外层签名和 embedded tuple manifest 后才提取。
5. no-follow staging 提取拒绝 traversal、绝对/盘符/ADS 路径、hardlink/device、重复路径及大小写/规范化冲突。
6. macOS Framework 必需 symlink 仅允许 manifest 声明的相对目标，逐跳验证不出 tuple、无循环；其他 link 拒绝。
7. 逐文件 hash、mode、文件表完整性、平台签名、Team ID/Authenticode 与架构检查全部通过后才允许运行 canary helper。

采用已有 Rust archive/parser 库，不通过 shell `tar` 处理不可信条目。
Host 硬上限初值：archive 2 GiB、展开 8 GiB、100,000 entries；manifest 限制只能更严，扩容需新版 Host。
预留磁盘为待展开大小两倍加 512 MiB；active tuple 和受保护 Profile 不纳入可回收预算。
错误/取消只清理本 staging transaction，current pointer、active directory 与 Profile 不改变。
staging 文件永不执行；未知文件、额外可执行文件和验签 warning 按 blocking failure 处理。

## 激活、活跃会话与 Profile epoch

状态机为 `idle -> checking -> downloading -> verifying -> canary -> staged -> activated`，失败进入明确 reasonCode。
只有新的 disposable Profile 完成 initialize、空白页截图、正常关闭和 descendants=0，tuple 才能进入 staged。
canary 失败 quarantine 当前候选；最多一次自动选择当前有效 policy 允许、兼容且未撤销的已记录 LKG。
Host 在原子事务中记录已接受指令、最高 sequence、候选 tuple 与 pointer journal，再切换 current pointer。
崩溃发生在任意事务点时，启动 reconciliation 只能选择完整 verified tuple；不依据目录名或 mtime 猜新版本。
新 BrowserRuntime 从 current pointer 解析 immutable tuple，记录 runtimeGeneration 与 tupleDigest。
活跃 Session 保持原 tuple 和 worker 协议；refcount=0 后关闭 admission 并 drain，随后证明 descendants=0，最后释放 Profile lock/回收 tuple。
refcount=0 只说明没有合法使用者，不证明进程已经退出；drain 失败保持 lock 与 quarantine，不先 GC 再清理。
普通更新不能自动重启活跃任务；新 session 可用新 tuple，现有 session 显示等待结束。
安全 stop_all 是单独 policy 事件，按共用 fence/unknown outcome 语义终止命中 Runtime。
Profile epoch 相同且兼容才能原地使用当前 generation；更高 epoch 只能创建新 generation。
migration 在无活跃 writer/OS lock 释放后复制原 Profile 到隔离新 generation，保留原始版本和校验清单。
迁移成功须证明合成/批准的登录态恢复、schema 健康与正常关闭；用户明确选择后绑定新 generation。
迁移失败保留原 Profile，不合并两份历史，也不删除旧数据以解锁更新。
App downgrade 或 Runtime rollback 只选择相容 Profile generation；没有则提供新 Profile/兼容更新/人工 Preview。
不同 Runtime 同时处理同一 Profile generation 必须返回 `profile_busy`；不能因更新而偷锁。

## 防重放、回滚与离线 policy

security checkpoint 持久化最高 release/policy/root sequence、指令 digest、revocation floor 和已撤销 key。
更低 sequence 拒绝；相同 sequence 不同 digest 拒绝；损坏账本进入修复并关闭 Managed，不从 embedded 低值重置。
本地一次 LKG 恢复仅引用已记录且当前 policy 允许的 tuple，不接受旧发行指令、不降低最高 sequence。
远程授权 rollback 必须是更高 sequence 的新签名指令，且目标不低于 revocation floor。
有效未过期 cache 可以离线使用；bootstrap 仅在无更高缓存且允许精确 embedded tuple 时生效。
过期 bootstrap、可信时钟异常、无有效 policy 均 `block_new`，保留 Preview 与数据。
`block_new` 禁止新 binding、lease、续租和 command；既有派发只在原 lease 内有界 drain，不能自动重试续跑。
只有新鲜有效签名 `stop_all` 因远程策略强制终止活跃 Runtime；网络失败不能伪造 stop_all。
policy 命中撤销 key/tuple 时不能以离线、用户 pin 或 LKG 例外恢复受撤销 capability。
所有 action 仍走 `RequestEnvelope` 的 policyRevision/lease/fence 校验，worker 每 primitive 复核。
已取得新 sequence 后手工 App downgrade 不可重建较低信任状态；跨版本共享兼容的安全账本并验证 schema。

## 平台、发布与回滚流程

macOS arm64/x64、Windows x64 使用同一组件协议，平台签名与文件清单按 target 分开验证。
Linux 仅对 M4 已通过的包/桌面行开放；deb/rpm App 更新仍是手动，Runtime 可在用户目录独立更新。
Windows portable App 保持 manual-update-only；组件更新须通过 portable 可搬动路径和重启 E2E 才开放。
更新目录由 app-data resolver 提供；不能要求写系统安装目录或以管理员权限覆盖包管理文件。
发布到不可变版本路径，先上传 Draft 全资产，再由 clean runner 重新下载验证；任何缺项保持 Draft。
publish 成功后才推进 signed index；客户端发现 404/错 digest/半发布保持当前可用版本并记录失败。
App release body 与 CHANGELOG 继续遵循 release wiki；独立 Runtime security notes 不手工改 App 发布正文。
撤回更新先发布更高 sequence 的 block/rollback 指令；删除远端 asset 本身不是安全回滚。
回滚 UI 只呈现有效签名指令允许的目标，没有任意选择磁盘旧 tuple 的绕过入口。

## 容量优化与 CEF 决策

M6-05 比较同 fixture/硬件的冷启动、warm tab、RSS/CPU、磁盘、崩溃恢复和打包维护成本。
保持默认最多两个 ProfileRuntime；提高 ceiling 必须用至少 30 样本证明，不静默驱逐活跃 Profile。
只复用同一兼容 ProfileRuntime；空闲回收按 refcount=0 -> drain/停止 -> descendants=0 -> unlock/GC，Artifact 写入未结束也阻止回收。
任何 pooling/prewarm 不得继承上一 AppSession 的 grant、观察数据、秘密或 TakeoverLatch。
CEF 实验位于隔离目录，使用同 fixture 与权限模型，不接生产路由/设置。
Go 必须同时满足启动/交互、sandbox/进程隔离、输入接管、可访问性、平台签名、升级和维护成本证据。
性能至少一项相对 Managed 改善 20%，其他初始指标不回退超过 10%，且安全硬门零退化，才可建议后续独立规格。
不可访问性/平台/升级证据不完整即 No-go；没有可用证据也记录 No-go，而不是默认继续集成。
Go 只授权提出后续实现计划，M6 不替换 WebView 或宣布 CEF 已成为正式 Runtime。

## 量化验收与未来证据

每个支持包连续三轮 100% 通过 App N-1/N × Runtime N-1/N × Profile epoch 组合。
至少 50 个下载/提取/pointer 断点、30 次 active update、30 次迁移失败/rollback，Profile 原始 checksum 不变。
坏签名、过期 key、序列重放/同序列异 digest、授权 rollback、revocation floor、root rotation 用例全部通过。
安全越权、执行未验证 helper、自动重放、Profile 串用、活跃目录修改和 30 秒后 orphan 为零。
离线有效/过期 bootstrap、缓存过期、时钟回拨、stop_all 与下载/副作用竞态均有可复现证据。
CEF 结果必须包含可复现测试、构建成本和明确 Go/No-go；不得只提交截图。
完整记录位于 `docs/qa/browser-rearchitecture/m6/`；M6-06 为出口，目前全部未执行。
