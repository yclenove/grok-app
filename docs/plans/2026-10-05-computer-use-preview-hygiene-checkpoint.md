# 测试窗口与过期包收尾

2026-10-05；用户反馈发任务后Grok没有调用电脑工具，同时要求清理不用的包、
测试结束退出App。单writer，本批没有启动新App或提交/推送/发布。

## 失败证据

真实fixture任务时间08:50:32Z，存于原App数据目录的会话
`89701eaf-95d6-473c-8547-61e55c439ddc`；原会话08:17连接成功。
模型的Search Tools结果只有已有Chrome DevTools等工具，没有CU；回复指出
没有`grok-computer-use`，fixture仍未保存/计数0。新app98隔离home无会话。
因此本次实际使用会话缺CU工具，未取得新预览包的真人验收；具体旧会话
为何未注入还需实际应用/catalog诊断，不能把模型未调用判为提示词问题。

app97隔离home另有AUTH_FAILED：没有登录凭据，尚未进入任务。它是另一记录，
不作为本次旧会话根因。app98启动WARN与WebView注销1412也不作为任务根因。
代码中本地会话可在CU关闭时获得request-only MCP，不能要求先全局开启/授权来绕过缺catalog。

## 已清理与空间

证据根 `tools/computer-use-probe/.run/codex-u04-existing-app-20261004/`。
删除前检查原owner已退出，精确绝对路径、全部ancestor/descendant无reparse point，
旧EXE/ZIP身份符合记录；删除后复验当前app102及原源/最新缓存hash不变。

| 清理项 | 逻辑bytes | H空闲实测增加bytes |
| --- | ---: | ---: |
| dist-portable内app97/app98两个目录及两个ZIP | 1484753121 | 1486155776 |
| App/Core的13组过期增量代 | 9111736156 | 6969712640 |
| 合计 | 10596489277（9.87GiB） | 8455868416（7.88GiB） |

精确targets、再生成方式、前后空间保存在`preview-packages-103-cleanup.json`及
`managed-recovery-cache-103-cleanup.json`；删除不在回收站，构建/打包可再生成。
空闲变化与逻辑大小分别报告，不把hardlink按逻辑大小冒称实际回收。
原失败profile、working/未知代、home/登录/会话、source/dirty/fix.patch、依赖和seed保留。

## App退出与今后规则

app97 PID28196、app98 PID39348均已退出，原host日志shutdown=true。
另外只退出精确NSIS测试App PID77028：C:/Users/Administrator/AppData/Local/Temp/
grok-cu-nsis-test/install/grok-app.exe，2026-10-03创建，EXE SHA-256
`fa227d2b6954b45b4583e2a1ba78612910ddad80347a5c49c0a0a2cd6eb8a85a`。
递归进程树只有WebView，没有活动agent/任务；CloseMainWindow仅隐藏到托盘，
随后只终止这个空闲旧测试owner并确认退出，没有删除其安装/回滚备份或修改主设置。
其它工作区的`grok-session-import-validation`验证程序与Grok Bot均未动。

AGENTS补充测试App lifecycle。app102入口拒绝重复任务预览，复用app98独立home；
该测试home的closeToTray及keepTrayForSchedules已置false，普通App设置未改。
VerifyOnly检查当前EXE、独立模式、退出设置和已有预览，不启动App。
本批只改文档/测试预览流程，没有新增产品编译；当前可用包仍app102/source101。
app102包含bootstrap与managed恢复修复，但尚未通过真实用户聊天任务，完整目标仍active。
