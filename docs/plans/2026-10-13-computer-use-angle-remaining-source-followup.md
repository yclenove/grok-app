# CU 维护浏览器：三项缺失库许可的来源补充

2026-10-13；延续[registry XML 检查点](2026-10-13-computer-use-angle-registry-checkpoint.md)。
这是来源采集结果，尚未接入统一候选；完整交付仍 active / partial。

## 已获得的原文

复用已测试的有界 Git collector，先核对原 ANGLE catalog 的 repository、
revision、Gitlink和原统一候选中缺失的许可路径，再采集固定源；不执行上游
代码、hook、条件或generator，不修改原 metadata、旧receipt或产品runtime。

| 来源 | 固定 revision | 树条目 | 选定文件 | 原文字节 |
| --- | --- | ---: | ---: | ---: |
| OpenCL-ICD-Loader | ddf6c70230a79cdb8fcccfd3c775b09e6820f42e | 84 | 1 | 11357 |
| astc-encoder | 2319d9c4d4af53a7fc7c52985e264ce6e8a02a9b | 553 | 3 | 18729 |
| cpu_features | 81d13c49649f0714dd41fb56bb246398b6584085 | 127 | 1 | 12876 |

总764树条目、5份原文、42962 bytes。三份原声明的根许可共34372 bytes，
每份核对 Git blob/SHA256；astc额外两份是测试图片的CC0/MIT许可，单独记录，
不将它们当作二进制依赖证据。cpu_features完整LICENSE末尾包含 Android
Open Source Project 的 BSD 声明，不能只导出开头的 Apache 正文。

此结果使原4项来源缺口中的3项具备准确原文字节；统一候选05/06仍保持原
4项未解决记录，后续须显式接线和重新复验，不能用采集成功回填旧报告。
Fuchsia SDK原许可、剩余复制/生成源、完整NOTICE/SBOM、媒体/源提供/安全
策略及原R2–R4全部门禁仍未完成。条件的本地false不作二进制排除依据。

## 证据与收尾

证据根：`tools/computer-use-probe/.run/maintained-notice-source-20261013-01/`。
原文与完整树在 `angle-remaining-notice-source-01/`；6579-byte根receipt SHA256：
`9f04f6f5c6ced5d32bf87c1990bee5c32b30876391d58aa94769e2a440f75bfa`。
一次性采集入口 `collect-angle-remaining-notices.py` 保留供复现，实际原进程
session22517已join、exit0。首败因本机Python3.8不支持`str.removesuffix`，
发生在输出目录创建前；改为固定`/src`后缀切片，保留首败与重试日志：
`angle-remaining-source-collect-01.log`、`angle-remaining-source-collect-01-retry.log`。

本次只新增源数据，无新编译/包/可退休执行副本，源cache和原失败证据保留。
完整554项测试与129文件格式/质量结果属于前一registry批次，不将其冒充本次
三库的独立NOTICE导出或发行验收。

当前用户App144真实窗口已通过computer-use查看：首页已加载、907字草稿保留。
另一个`grok-session-import-validation`工作区的测试程序弹出Windows防火墙
请求；未操作该系统权限弹窗、未停止该测试或代发草稿。本次观察只证明当前
首页渲染，不证明模型CU/文件选择/暂停恢复或最新维护runtime可用。
