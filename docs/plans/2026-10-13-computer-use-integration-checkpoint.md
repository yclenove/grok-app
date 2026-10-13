# CU 维护浏览器：ANGLE / OpenScreen 的 Chromium 集成

2026-10-13；`feat/computer-use-implementation`；active / partial。
承接[Crashpad 配置与 lss 来源](2026-10-13-computer-use-crashpad-checkpoint.md)。
用户已确认 App 保持 macOS 11+，CU 受管浏览器要求 macOS 14+。

## 本批结果

准确主 DEPS 的 `vars.build_with_chromium = True` 导出到
`src/build/config/gclient_args.gni`；原 recipe 使用的 gclient helper 的 AST
确认通过 `get_vars()`、`variables[arg]`、`ToGNString(value)` 写入。
主 `build_overrides/build.gni` 和 ANGLE 的准确 Chromium 分支导入该 generated file。
helper builtin vars 没有覆盖此变量，四原 build 的 GN 写入参数/custom vars 也无相关覆盖。
没有读取远端 generated file 或测量整个 effective GN 图；以上是源码对应证据。
ANGLE / OpenScreen 自己 standalone 的 false default 单独记录，不代替 Chromium 根配置。

| 依赖 | Chromium 内的实际引用 | 已核验许可证原路径 |
| --- | --- | --- |
| ANGLE Vulkan loader | `//third_party/vulkan-loader/src:libvulkan` | `third_party/vulkan-loader/src/LICENSE.txt` |
| OpenScreen URL | `//url`，`url/gurl.h` 的 BSD/root LICENSE header | `LICENSE` |
| OpenScreen QUICHE | `//net/third_party/quiche` | `net/third_party/quiche/src/LICENSE` |
| OpenScreen Perfetto | `//third_party/perfetto` | `third_party/perfetto/LICENSE` |

四条 mapping 保留原 standalone metadata 的嵌套路径错误；没有创建 alias、删错误或改 Shipped 字段。
ANGLE ASTC 默认表达式和 OpenCL false 默认/强制关闭分支只作源码记录，未据此豁免通知。

## 来源与复验

`collect-chromium-integration-sources.py` 的 integration / embedding / targets 三个 profiles
共采 **20份 / 142385 bytes**，每件准确 Git blob 独立匹配已封存主/外部树。
Chromium、ANGLE、OpenScreen 的 repository/revision 均匹配四个原 archive checkout manifests。
只读固定 public Gitiles endpoint，拒绝 redirect/Content-Encoding，20秒、raw2MiB/source1MiB/总8MiB。
来源只保留为 inert data，不执行 GN、gclient 或 upstream code。

三个 profile receipts：

- integration：`e13f661a278e0517b82eca45c0008134f5e56a62e3614981c0d1e608f2065b23`。
- embedding：`a17967d2acf493b62e2a4308d338eb75f64e828cb8f16b0f9302dedb7c4b0e82`。
- targets：`507cbcb9dabc519613cb065b4b021f8c2cd92f72f015f22dda9612aa0c00406d`。

Perfetto 的准确 pin 是 `59400b15c6ff8186ba0bba69f78c2cd454eea8b4`，
repository 是 `https://chromium.googlesource.com/external/github.com/google/perfetto.git`，
主 DEPS/Gitlink 和四 actual manifests 一致。
`collect-chromium-perfetto-licenses.py` 复用有界 inert Git collector，
采完整 tree index **12880 entries / 1632824 bytes** 与四 selected paths **27666 bytes**，
不是完整源 checkout。根/child receipt seals：
`fad740c55664cd5ce138c011985df120f65db8a9fe253b57f4579484093e47f6` /
`dfa59c35a0b46604d500e34903f1a799db69aa8398d17fc589fefe6d48e65267`。

`inspect-chromium-integration-coverage.py` 重验全部来源、raw/decoded bytes、原树与 manifests，
引用16条准确 source rules、main DEPS export/helper AST 和四 GN/custom-var records。
最终 `integration-coverage-04.json` / `05` 独立重算同 SHA256：
`910d738a12ee5a8fbf0a9e73e68329b397c8ef528d579bdf86ae4b21de62f8da`。
先前01–03保留：补强 generated-file import 链，并将跨块 regex 改为准确单块规则后才冻结04/05。

结果 **7541 selected paths / 1251 raw references / 829 resolved / 422 raw gaps**；
**17 yes metadata errors / 196 directory errors / 5 nonstandard Shipped values**。
比前批多23 selected paths 和1条已解析引用；原422 gaps/17 yes错误均保留。
Perfetto `protos/third_party` 的 android/chromium/pprof/primes/simpleperf/statsd
六目录缺 metadata，目录错误190→196；不排除或当成六个随包组件。
`licenseCoverage:not_proven` / `promoted:false`，不是完整 NOTICE/SBOM 或产品验收。

## 验证与清理

新增 collector14 / Perfetto8 / integration inspector16项，加前批319，显式 runner
实际 **357 passed / 0 failed / 0 skipped**。
覆盖四manifest/foreign pin、raw/source/tree/license篡改、路径逃逸、失败保留、
重复/跨scope规则、GN/custom-var覆盖、inert DEPS/helper与真实路径不伪造alias。
runner首次最小计数写15但实际14，首败日志保留；补Perfetto树/license篡改及跨scope反例后为16。
collector早期 Python3.8无 `str.removesuffix` 的失败另有原日志，现用suffix check/slicing。
收尾65份Python AST、73份严格格式、80份LF/EOF/hash与质量门禁通过；
App+Workbench13573 / ≥1000源码80不增长，原R2 22源码hash和四历史许可seals、前批两Crashpad报告不变。
记录为 `integration-final-input-hashes-01.json` / `quality-integration-final-01.json`。
本批没有编译包、runtime执行副本或重复大包，删除0/逻辑0/实际回收0，
`integration-cleanup-01.json` 保留实测空间观察及保留项，不把空间变化当回收量。
原preview PID59356实读已不存在；当前可用App144、用户home、依赖和全部source/失败证据保留。
产品pins、四锁、seed、App144未晋升；没有提交、推送、PR或发布。

## 下一步

1. 核验 ANGLE cpu_features / LLVM / EGL-OpenGL registries、OpenScreen libc++/LLVM 的准确消费路径、
   其余生成输入和 metadata字段。已有四条映射不代表其它同名来源可以直接复用。
2. 完成四平台 NOTICE/SBOM、源提供/LGPL-media与稳定安全更新策略，再选择最终 browser。
3. 继续原[R1–R4](2026-10-13-computer-use-maintained-runtime-next.md)全部门禁：
   production pins/四锁/prepare/check/repair/rollback/bundle、四架构原生/系统边界、
   安装/签名/更新/卸载、四surface/UI UX、真人ACP/MCP/Grok/model/picker、最终candidate12h/D00–D31。
