# M3 Chrome Connector 实现计划

> **面向 AI 代理的工作者：** 使用 subagent-driven-development 或 executing-plans 逐任务实现。步骤使用未勾选复选框追踪；本文件记录未执行待办，当前文档交付不代表产品完成。实施期间的 commit/push 遵循总索引。

**目标：** 交付扩展显式 claim、可信 native bridge、整连接接管和可恢复、可撤销的 Chrome 操作闭环。
**架构：** Manifest V3 扩展只把已认领目标投影到 Rust Host，native bridge 验证 Chrome/Host 进程身份并保持私有 IPC。Gateway 发出受限动作，整条 ChromeConnection 是输入 fence 域。
**技术栈：** Rust/Tauri 2、Chrome MV3、TypeScript/React、固定 Playwright helper 可行性验证、native messaging、Vitest 与 M1 真实包 fixture。
**规格：** [M3](../../specs/2026-09-05-browser-rearchitecture/m3-chrome.md)、[共用合同](../../specs/2026-09-05-browser-rearchitecture/00-contracts.md)。
**进入门：** M1-G06、M1-R06、M1-D06；总工作量预估 27..43 工程日，外部商店审核等待单列。
**验证边界：** 本计划的 extension/bridge/脚本/QA 路径均为计划新增；当前未运行、未发布扩展或注册 native host。

## 代码证据与文件职责

| 文件 | 处理方式与职责 |
| --- | --- |
| `src/lib/sideWorkbench.ts`、`src/components/side-workbench/BrowserTab.tsx` | 现有 URL/tab UI 模型只作迁移入口，Chrome tabId/groupId 不能成为 Host 身份。 |
| `src/lib/api/system.ts`、`src-tauri/src/commands/terminal.rs` | 旧 WebView 命令不承担外部 Chrome 权限；新增能力走 M1 Gateway。 |
| `browser-connector/extension/` | 新建 MV3 源码、manifest、claim UI、固定脚本、CDP allowlist、i18n构建和测试。 |
| `src-tauri/src/browser/connector/` | 新建协议、配对、进程核验、claim注册、输入fence、动作和恢复服务。 |
| `src-tauri/src/bin/browser-native-bridge.rs` | 新建纯 framing/认证IPC companion，不做模型推理或自行授权。 |
| `src-tauri/Cargo.toml`、`src-tauri/src/lib.rs` | 新增 binary target/模块注册；沿用现有 serde、tokio、keyring、原生平台基础。 |
| `src/components/browser/connector/` | 新建连接/claim投影、App确认与设置管理；禁止向 App壳添加状态。 |
| `src/lib/settingsCatalog/entries/runtime.ts`、`src/components/settings/RuntimeSection.tsx` | 修改 runtime/connection 的稳定锚点和搜索登记。 |
| `src/i18n/messages/<locale>/features.ts`、`settings.ts` | 15目录修改；`<locale>`为`de,en,es,fil,fr,id,it,ja,ko,pt-BR,ru,ta,uk,zh,zh-TW`。 |
| `tests/browser/m3/`、`docs/qa/browser-rearchitecture/m3/` | 新建场景与实际证据；复用 M1-D04 的 `tests/browser/playwright.config.ts`。 |

Host 共用协议位于 `src-tauri/src/browser/{protocol,gateway,origin,binding,policy,lease,lifecycle,persistence}.rs`，由 M1 提供。
`NativeInputFenceProvider` 来自 M1 的 `src-tauri/src/browser/runtime/input_fence.rs`；Connector 只实现包装和归属，不改变安全合同。
wire协议版本1、camelCase；Chrome `runtimeGeneration=null`；revision/generation/fence为不回绕`u32`，origin由Host计算。
以下JSON为用例scenario，不是缺少完整RequestEnvelope的生产请求；根Vitest仅发现`src/**`，extension独立配置并验证用例数。
任务依赖：M3-01 -> M3-02 -> M3-03 -> M3-04 -> M3-05 -> M3-06；M2不是前置依赖。

### M3-01：MV3 骨架、配对与 native bridge

**依赖：** M1-G06、M1-R06、M1-D06。
**负责：** Rust/平台工程师，扩展工程师。
**估算：** 4..6 工程日。
**文件：** 新建 `browser-connector/extension/{package.json,pnpm-lock.yaml,manifest.json,vite.config.ts,vitest.config.ts}`、`browser-connector/extension/src/{serviceWorker.ts,nativeProtocol.ts,nativeProtocol.test.ts}`、`src-tauri/src/browser/connector/{mod.rs,protocol.rs,pairing.rs,ipc.rs,protocol_tests.rs}`、`src-tauri/src/bin/browser-native-bridge.rs`；修改 `src-tauri/Cargo.toml`、`src-tauri/src/lib.rs`；新建 `tests/browser/fixtures/m3/native-frames.json`。

- [ ] **步骤 1：定义最小 manifest 与畸形帧 fixture。** 以仓库已选依赖版本固定 extension package，运行 `pnpm --dir browser-connector/extension install` 生成独立 lockfile；使用合成扩展ID，覆盖截断、256KiB+1、重复身份字段、伪origin、重放nonce、旧Host boot。

```json
{"manifest_version":3,"name":"__MSG_extensionName__","default_locale":"en","version":"0.1.0","permissions":["activeTab","scripting","nativeMessaging","storage"],"optional_permissions":["tabs","tabGroups","debugger"],"background":{"service_worker":"service-worker.js","type":"module"}}
```

- [ ] **步骤 2：建立协议红灯。** 新增extension Vitest `include:["src/**/*.test.ts"]`；运行 `pnpm --dir browser-connector/extension exec vitest run --config vitest.config.ts src/nativeProtocol.test.ts` 和 `cargo test --manifest-path src-tauri/Cargo.toml browser::connector::protocol_tests -- --nocapture`，预期解析/握手尚未实现导致失败，不能跳过未知字段案例。

- [ ] **步骤 3：实现有界 framing 与私有 IPC。** Chrome侧读取32位little-endian长度、严格UTF-8/JSON/schema，stdout仅协议；Host IPC采用共用big-endian/1MiB帧，bridge转换而不猜测字节序。Unix peer credentials/Windows SID ACL及peer PID校验Host，内存nonce绑定connection/extension/Host boot。

```rust
const MAX_NATIVE_FRAME_BYTES: usize = 256 * 1024;
fn valid_frame_len(len: u32) -> bool {
    len > 0 && (len as usize) <= MAX_NATIVE_FRAME_BYTES
}
```

加入backpressure、timeout、EOF fence和stderr脱敏ring；不能仅靠允许的extension ID或命令行origin授予权限。

- [ ] **步骤 4：实现双端配对状态。** App用户发起60秒单次challenge，扩展显示匹配码，可信App确认extension/channel；持久化非秘密pairing元数据，connection key仅内存，page/postMessage无法进入配对权限入口。

```json
{"case":"expired-pairing","challengeAgeSeconds":61,"userApproved":true,"expectedReasonCode":"pairing_expired","expectedActiveConnections":0}
```

- [ ] **步骤 5：验证协议和产物。** 跑步骤2两条命令、`cargo check --manifest-path src-tauri/Cargo.toml --bin browser-native-bridge`、`pnpm --dir browser-connector/extension exec vite build --config vite.config.ts`；预期PASS/exit0，帧错误不panic、不输出秘密，build只产生manifest列明本地脚本。

- [ ] **步骤 6：交付可信连接骨架。** 新建 `docs/qa/browser-rearchitecture/m3/pairing-protocol.md`，记录版本、消息schema、超时和撤销行为；尚未完成进程归属前writer始终不可授予，连接成功不等于认领或浏览器控制已成功。

**验收：** 合法双方才能完成一次性配对，畸形帧、伪身份、重放和过期challenge均明确拒绝，退出不遗留活动凭据。

### M3-02：显式 claim、任务组与 App 交互

**依赖：** M3-01。
**负责：** 扩展/前端工程师，Host工程师。
**估算：** 4..6 工程日。
**文件：** 新建 `browser-connector/extension/src/{ClaimPopup.tsx,claims.ts,claims.test.ts,connector.css}`、`src-tauri/src/browser/connector/{claims.rs,claim_tests.rs}`、`src/components/browser/connector/{ChromeClaimDialog.tsx,ChromeConnectorPanel.tsx,ChromeConnectorPanel.test.tsx,connectorStore.ts}`；修改 `src/lib/settingsCatalog/entries/runtime.ts`、`src/components/settings/RuntimeSection.tsx`、`src/i18n/messages/<locale>/{features,settings}.ts`；新建 `tests/browser/m3/claims.spec.ts`。

- [ ] **步骤 1：固定“组是快照”的失败用例。** 当前tab/当前group显式选择；测试组内新增、移出、关闭、document替换、ID复用和跨Session转移，未确认新成员不能进入claim。

```json
{"case":"group-add-after-claim","approvedTabIds":[10,11],"currentGroupTabIds":[10,11,12],"expectedClaimedTabIds":[10,11],"expectedUnclaimedTabIds":[12]}
```

- [ ] **步骤 2：确认越界与交互红灯。** 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::connector::claim_tests -- --nocapture`、`pnpm --dir browser-connector/extension exec vitest run --config vitest.config.ts src/claims.test.ts`、`pnpm test -- src/components/browser/connector/ChromeConnectorPanel.test.tsx`；预期尚无claim状态机和聚焦路径而失败。

- [ ] **步骤 3：实现claim权限与稳定绑定。** 用户动作才申请可选tabs/tabGroups并查询当前组；Host在批准和attach时双检snapshot，BrowserTab UUID对应易失Chrome tab/document，关闭/移出/rebind递增generation。

```ts
export type ClaimSnapshot = { claimId: string; connectionId: string; chromeTabIds: readonly number[]; generation: number };
export function isClaimed(snapshot: ClaimSnapshot, chromeTabId: number): boolean {
  return snapshot.chromeTabIds.includes(chromeTabId);
}
```

转移先fence/revoke旧claim再创建新对象；不转移其他Session的Artifact/grant；保持共享ChromeConnection单writer范围。

- [ ] **步骤 4：实现完整可用UI。** App显示origin/目标项目/数量/整连接接管影响，用户确认后创建；扩展停止/释放只能收紧，交还在App。完成空/忙/取消/无权限/失联状态，320px popup、键盘勾选、焦点恢复、portal与native cover，所有文字来自15语言catalog。

```ts
{ id: "runtime.browser.chromeConnector", section: "runtime", tab: "connection", anchorId: "settings-anchor-browser-chrome-connector", labelKey: "browser.chrome.manage", keywords: ["chrome", "connector", "browser"] }
```

- [ ] **步骤 5：验证范围与设置入口。** 跑步骤2三条命令、`pnpm test -- src/lib/settingsCatalog.test.ts src/i18n/messages.test.ts`、`pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/m3/claims.spec.ts`；预期PASS，关闭pane不关Chrome，释放claim不关tab，未认领页的标题/URL/正文读取次数为零。

- [ ] **步骤 6：交付任务组闭环。** 新建 `docs/qa/browser-rearchitecture/m3/claim-lifecycle.md`，记录增减移动/冲突转移、长文本与键盘截图；来源不明的tab/document禁止复用旧binding，所有菜单操作有真实状态结果。

**验收：** 用户只认领明确快照成员，未认领页面读取/控制零发生，任务隐藏/释放与Chrome页面关闭行为可区分。

### M3-03：实际进程身份与整连接原生输入 fence

**依赖：** M3-01、M3-02。
**负责：** macOS/Windows平台工程师，安全工程师。
**估算：** 5..8 工程日。
**文件：** 新建 `src-tauri/src/browser/connector/{process_identity.rs,process_identity_macos.rs,process_identity_windows.rs,input_fence.rs,input_fence_tests.rs}`、`browser-connector/extension/src/{fence.ts,fence.test.ts}`；修改 `src-tauri/src/browser/connector/{pairing,ipc}.rs`；引用M1 `src-tauri/src/browser/runtime/input_fence.rs`；新建 `tests/browser/m3/connection-fence.spec.ts`、`tests/browser/fixtures/m3/process-identity.json`。

- [ ] **步骤 1：构造身份与输入负例。** 合成PID相同但startTime不同、伪签名/父链、不同用户、未登记窗口、两个ChromeProfile同进程、Provider健康丢失，以及一个连接下多个claim、同进程多connection同时fence。

```json
{"case":"connection-wide-input","connectionId":"chrome-fixture","claimIds":["claim-a","claim-b"],"inputWindow":"unclaimed-window-same-process","expectedRevokedClaimIds":["claim-a","claim-b"],"expectedTakeoverLatch":"user"}
```

- [ ] **步骤 2：确认fence红灯。** 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::connector::input_fence_tests -- --nocapture` 和 `pnpm --dir browser-connector/extension exec vitest run --config vitest.config.ts src/fence.test.ts`；预期旧lease/Provider故障/跨claim停止断言失败，证明测试不是只看DOM事件。

- [ ] **步骤 3：实现可证明的ProcessIdentity。** 用OS句柄读取PID/startTime/用户/实际binary身份和签名，验证native bridge父链；关联实际Chrome进程全部窗口，扩展windowId只作提示。原生事件进入M1 Provider，归属含糊时blocked，不授writer。

```json
{"pid":42001,"startTime":1700000000,"userIdentity":"fixture-user","executableIdentity":"fixture-signed-chrome","parentChainVerified":true,"windowOwnershipVerified":true}
```

真实测试读OS字段，不能把本JSON当作认证来源；macOS监听权限撤销/Windows hook或RawInput失败都即时fence。

- [ ] **步骤 4：实现原子撤销与ack。** Host提高epoch、独立设置TakeoverLatch、关闭admission/清队列/暂停观察；同一OS进程所有connection同时fence。扩展每primitive前和await/retry后检查epoch，等待重试必须可取消。动作竞态记unknown_outcome，重连/turn结束/lease到期不清latch，App交还生成新snapshot/lease。

```json
{"case":"late-primitive","sentFenceEpoch":7,"currentFenceEpoch":8,"awaitCompleted":true,"expectedReasonCode":"stale_fence_epoch","expectedRemainingPrimitiveCount":0}
```

- [ ] **步骤 5：运行单测和真实原生输入。** 跑步骤2命令及 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/m3/connection-fence.spec.ts`；在三个签名安装包上由操作员执行pointer/wheel/key/IME，记录Host权威事件到ack的p95 <=250ms。自动脚本只控制fixture，不用SendInput/CGEventPost冒充已验证来源。

- [ ] **步骤 6：交付Chrome专属接管证据。** 新建 `docs/qa/browser-rearchitecture/m3/native-input.md`，逐平台记录Provider权限、可覆盖输入、AX限制、进程/窗口歧义和故障恢复；任何归属缺口保持writer关闭，不能用M1 Managed证据代替。

**验收：** 一个原生输入撤销整连接所有claim控制，Provider故障不能续租，旧队列不恢复，三个平台均有实际fence延时证据。

### M3-04：受限定位器、动作、上传与下载 broker

**依赖：** M3-02、M3-03。
**负责：** 浏览器自动化/安全工程师，扩展工程师。
**估算：** 7..12 工程日。
**文件：** 新建 `browser-connector/extension/src/{adapter.ts,locator.ts,cdpAllowlist.ts,uploadBridge.ts,downloadBroker.ts,blobBroker.ts,adapter.test.ts,uploadBridge.test.ts,downloadBroker.test.ts,blobBroker.test.ts}`、`browser-runtime/worker/scripts/build-connector-injected.mjs`、`src-tauri/src/browser/connector/{dispatch.rs,transfers.rs,dispatch_tests.rs}`；修改M1 Gateway adapter注册、Artifact/UploadGrant接口；新建 `tests/browser/m3/{actions.spec.ts,upload-ownership.spec.ts,download-broker.spec.ts,blob-download.spec.ts}`、`tests/browser/fixtures/m3/{actions.html,uploads.html,downloads.html}`。

- [ ] **步骤 1：先验证引擎和文件可行性硬门。** 为固定Playwright revision导出locator/actionability helper，记录license/source/hash/内部接口；用真实Chrome证明frame/open shadow/auto-wait/overlay遮挡。探测有界分块经bridge/extension进入固定helper隔离环境组装Blob/File后一次attach，实证51/200MiB与延迟读取/提交hash，不借用Managed路径spool或50MiB受限buffer API。并探测已认领target的Fetch response stream能否携带可核实frame/networkRequestId并安全转入staging，禁止用URL关联；blob另验证已认领document/frame的creator origin和固定helper流式读取，不要求不存在的HTTP requestId。

```json
{"case":"same-url-downloads","requests":[{"claimId":"claim-a","networkRequestId":"net-1"},{"claimId":"claim-b","networkRequestId":"net-2"}],"sameUrl":true,"approvedNetworkRequestId":"net-1","expectedStoredRequestIds":["net-1"]}
```

若helper无法合法/稳定导出、上传无法独立持有字节或broker归属不成立，保留failed证据和具体能力缺口；人工路径仅受限替代，不能把本任务判为通过。

- [ ] **步骤 2：建立安全红灯。** 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::connector::dispatch_tests -- --nocapture`、`pnpm --dir browser-connector/extension exec vitest run --config vitest.config.ts src/adapter.test.ts src/uploadBridge.test.ts src/downloadBroker.test.ts src/blobBroker.test.ts`；预期未实现动作守卫/字节接管/下载identity导致失败。包含未认领target、第三方frame、原始CDP、秘密读取、上传分块中撤权、未知写操作、未知creator和跨claim blob handle。

- [ ] **步骤 3：实现受控adapter。** 只接受Host生成的语义动作、固定helper hash和typed args；根据当前binding计算origin，认证后映射明确CDP方法。debugger按需申请并仅attach已claim tab，detach立即fence，无法等价支持的能力返回明确未达结果。

```ts
export const forbiddenCdpMethods = new Set(["Network.getAllCookies", "Storage.getCookies", "Browser.setDownloadBehavior", "Target.createTarget"]);
export type ConnectorAction = "navigate" | "query" | "click" | "fill" | "scroll" | "screenshot" | "attachUpload" | "download";
```

`ConnectorAction` 仅为扩展内部primitive：共用wire的snapshot/type/upload分别映射query/fill/attachUpload；download broker随获批准触发动作建立，不接收自行发明的wire action。
真实实现采用允许列表，禁用列表只是回归用例；Runtime任意表达式/任意function/body不得通过。高风险和未知动作由Host确认，页面role/text与模型risk字段不充当低风险证明。
复用helper的actionability检查必须只读且逐次返回，内部等待由可取消调度替代；加入“等待遮罩移除时接管”的真实用例，ack后输入计数为零。不得通过关闭用户Chrome target来通过取消测试，无法停止的primitive不开放。

- [ ] **步骤 4：完成文件和观察闭环。** UploadGrant保留M1的200MiB/披露/hash/expiry/revoke合同；Chrome采用步骤1已实证的内存Blob/File接管，分块前后校验epoch/binding，独立字节确认后才释放Host staging。Managed的UploadSpoolLease等所属Runtime descendants=0才能删，Chrome不得复用该门、杀用户进程或永久保留副本；已披露字节无法收回。HTTP下载只对批准target/request启用一次性Fetch捕获，校验frame/origin chain/response大小后按downloadId写Artifact，完成/失败/fence撤销拦截。blob由固定helper在已验证creator的claim/document/frame内签发短期handle，分块流读取并核对预算/hash，跨claim、revoked或generation变化拒绝；无可行blob实现是未完成，不能据此PASS，不打开全局downloads权限。

```json
{"case":"download-fence-during-stream","bytesReceived":65536,"event":"takeover","expectedState":"interrupted","expectedReadyArtifacts":0,"expectedOtherClaimInterceptions":0}
```

补齐截图和有界Console/Network metadata，支持M2存在时消费相同投影；不向MCP发送Chrome本地路径/原始headers。

- [ ] **步骤 5：验证真实能力与负例。** 跑步骤2命令、`pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/m3/actions.spec.ts tests/browser/m3/upload-ownership.spec.ts tests/browser/m3/download-broker.spec.ts tests/browser/m3/blob-download.spec.ts`；预期规格能力表每个必须对等项PASS，覆盖51/200MiB延迟读取/提交、200MiB+1拒绝、分块撤权和无路径泄漏，同URL并发/redirect/frame/blob/自主下载/接管、文件撤销、页面替换后确认失效、未知风险dont_ask拒绝和无raw CDP。

- [ ] **步骤 6：交付能力差异与解决证据。** 新建 `docs/qa/browser-rearchitecture/m3/{adapter-feasibility.md,capability-matrix.json,transfer-attribution.md}`，逐行实现规格Managed/Chrome能力表的requirement、dependency、evidence和状态。helper、上传字节接管、HTTP或blob研究失败必须留未完成项及受限范围决定，M3-06不得以unsupported/人工替代算通过；完整M2人类面板单列M2-06与M3-06组合依赖。

**验收：** helper导出合法且可维护，声明动作满足真实actionability，下载有逐claim请求归属及负例证明；任何研究失败都保持本任务未完成。

### M3-05：撤销恢复、最小权限与可发布产物

**依赖：** M3-04。
**负责：** 发布/平台工程师，安全与前端工程师。
**估算：** 4..6 工程日。
**文件：** 新建 `src-tauri/src/browser/connector/{recovery.rs,registration.rs,recovery_tests.rs}`、`browser-connector/extension/src/{recovery.ts,recovery.test.ts}`、`browser-connector/extension/scripts/check-package.mjs`、`scripts/browser/build-connector-package.mjs`、`scripts/browser/check-connector-registration.mjs`；修改 `scripts/build-local.sh`、`.github/workflows/release.yml`、`src-tauri/Cargo.toml` 和extension构建配置；新建 `tests/browser/m3/recovery.spec.ts`。

- [ ] **步骤 1：写恢复/安装归属fixture。** 覆盖Host/Chrome/bridge/ServiceWorker重启、卸载/权限收回、版本不兼容、portable移动、两安装竞争注册、篡改注册路径和撤销持久化失败。

```json
{"case":"service-worker-restart","previousClaims":2,"pairingMetadataPresent":true,"expectedConnectionState":"disconnected","expectedRestoredClaims":0,"expectedRestoredLeases":0}
```

- [ ] **步骤 2：确认恢复红灯。** 运行 `cargo test --manifest-path src-tauri/Cargo.toml browser::connector::recovery_tests -- --nocapture` 和 `pnpm --dir browser-connector/extension exec vitest run --config vitest.config.ts src/recovery.test.ts`；预期旧claim/进程注册或原子撤销逻辑尚未实现而失败。

- [ ] **步骤 3：实现收紧权限和保守恢复。** 撤销先fence/admission-close、清claim/grant/token、detach debugger和移除overlay，再持久化；失败保留fail-closed标志。重启只恢复脱敏任务摘要，必须新握手和新claim，旧队列/高风险动作不重放。

```json
{"case":"revoke-disk-full","expectedReasonCode":"persistence_degraded","expectedAdmission":"closed","expectedReconnectAllowed":false,"expectedChromeProcessTerminated":false}
```

- [ ] **步骤 4：完成最小权限和发行构建。** 生成官方ID专属native manifest，bridge目标必须为本安装验证的绝对路径，App升级原子重注册；卸载仅删除本安装拥有的记录。extension从15语言源生成本地catalog，校验无远程脚本/秘密，产出lockfile/SBOM/hash/兼容表及官方商店材料。

```bash
pnpm --dir browser-connector/extension exec vite build --config vite.config.ts
node browser-connector/extension/scripts/check-package.mjs --dir browser-connector/extension/dist
node scripts/browser/build-connector-package.mjs --extension-dir browser-connector/extension/dist --output .browser-test/connector-package
```

脚本固定输入schema并在缺少签名/许可/版本时exit1；平台签名和商店发布按现有release流程执行，开发ID/加载未打包扩展不能当正式发行证据。

- [ ] **步骤 5：验证真实恢复与注册。** 跑步骤2命令、`pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/m3/recovery.spec.ts`、`node scripts/browser/check-connector-registration.mjs --installed`；预期PASS/exit0。三个平台真实安装、升级、portable移动后重登记、撤销和卸载，用户Chrome进程/tab/数据保持完整。

- [ ] **步骤 6：交付发布与回退记录。** 新建 `docs/qa/browser-rearchitecture/m3/{release.md,recovery.md,registration.md}`，记录签名与商店状态、认证Chrome版本范围、更新前fence、版本不兼容和回退后重新claim；外部审核未完成保留发行门未通过，不能宣称已发布。

**验收：** 签名bridge与正式扩展可以安装/升级/撤销/卸载，故障恢复不复活权限，用户Chrome进程和数据保持完整。

### M3-06：Chrome 全连接验收与发布门

**依赖：** M3-03、M3-04、M3-05。
**负责：** QA/发布工程师，安全与平台工程师独立复核。
**估算：** 3..5 工程日。
**文件：** 新建 `tests/browser/m3/{package-golden.spec.ts,privacy.spec.ts,m2-inspection-integration.spec.ts}`、`scripts/browser/check-m3-evidence.mjs`、`docs/qa/browser-rearchitecture/m3/{README.md,matrix.json,acceptance.md,scope-decisions.md}`；修改M1 `tests/browser/playwright.config.ts`接入场景与`docs/llm-wiki/settings-ia.md`、`media-delivery.md`的连接器约束；更新M3 capability开关。

- [ ] **步骤 1：建立真实包/Chrome矩阵。** 每个macOS arm64/x64、Windows x64目标记录签名App/bridge、正式扩展ID/版本、Chrome major、Provider权限、fixture revision、命令、结果和脱敏证据hash；能力差异是必填字段。

```json
{"milestone":"M3","status":"planned","platforms":["macos-arm64","macos-x64","windows-x64"],"required":["pair","claim","processIdentity","nativeFence","actions","downloadAttribution","revoke","recover","release","accessibility"],"managedParity":"unverified"}
```

- [ ] **步骤 2：先验证门控会拒绝缺证据。** 运行 `node scripts/browser/check-m3-evidence.mjs --matrix docs/qa/browser-rearchitecture/m3/matrix.json`，planned/failed/空用例/开发ID/缺商店状态/未记录差异时应exit1。checker逐行读取规格能力表，任一必须对等项（含blob）无证据即失败，不将unsupported等同pass；完整M2检查仅在M2未完成时可记blocked_by_m2，不能据此开放组合能力。

- [ ] **步骤 3：运行完整自动验证。** 执行下列命令；预期非零测试PASS、typecheck/lint exit0，存量质量门失败须如实记录，不能覆盖为绿色。

```bash
pnpm --dir browser-connector/extension exec vitest run --config vitest.config.ts
pnpm test -- src/components/browser/connector src/lib/settingsCatalog.test.ts src/i18n/messages.test.ts
cargo test --manifest-path src-tauri/Cargo.toml browser::connector:: -- --nocapture
pnpm typecheck
pnpm lint
python3 scripts/check-code-quality-gates.py --mode final
```

- [ ] **步骤 4：执行真实Chrome安装包golden path。** 各认证平台运行 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/m3`；由操作员完成真实扩展安装/配对和原生输入，证明新组成员不可见、两claim同连接接管、DevTools detach、无归属不授writer、释放不关Chrome。不得用未打包扩展加开发Chromium替代正式Chrome发行验证。

- [ ] **步骤 5：完成权限、文件、视觉与恢复演练。** 记录敏感哨兵零泄漏、全局下载API零调用、HTTP与blob broker误归属零、上传延迟读取/撤销、helper许可/hash、PID复用和框架兼容。人工检查320px popup/360px pane、15语言/浅深色/200%字体、键盘读屏和native cover；多轮重启始终要求新claim。M2-06已完成时另跑 `pnpm exec playwright test --config tests/browser/playwright.config.ts tests/browser/m3/m2-inspection-integration.spec.ts`；M2后完成时由M2-06执行同一组合场景，证据缺失不开放完整M2检查。

- [ ] **步骤 6：给出明确M3出口。** 重跑 `node scripts/browser/check-m3-evidence.mjs --matrix docs/qa/browser-rearchitecture/m3/matrix.json`，能力对照表所有必须对等项/安全/发行门满足才exit0并开放完整M3。若定位器、上传字节接管、HTTP或blob broker失败，将证据、未完成能力与受限试用范围决定写入scope-decisions，不把人工替代或unsupported算Managed对等；M3完整验收仍保持未通过，M2组合能力按独立双门判定。

**验收：** 三平台正式发行物通过明确claim、原生接管、动作/传输、撤销/恢复、隐私和无障碍；未达能力在最终判定中可见。

## 覆盖自检

M3-01覆盖协议/配对，M3-02覆盖明确claim与任务组，M3-03覆盖进程归属/连接级fence，M3-04覆盖引擎/动作/文件，M3-05覆盖撤销/恢复/发行，M3-06覆盖全部真实包、隐私、能力差异和视觉门。
总计划恰好6个任务，每项6个未执行步骤；M3-06是完成出口，与M2无串行依赖，未解决能力不能在验收矩阵中消失。
