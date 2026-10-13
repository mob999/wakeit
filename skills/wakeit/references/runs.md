# 运行

```sh
wakeit --json run list --task <task-id>
wakeit run get <run-id>
wakeit run logs <run-id> --follow
wakeit run cancel <run-id>
wakeit --request-key <stable-retry-key> run retry <run-id>
```

get 的 JSON 中读取实际状态和结果，日志跟随可在需要看到最终报告时停止。WebUI “执行”页展示实际过程和结果，“详情”页提供日志及运行信息。纯文本适配只有输出，不从文本猜工具调用。

取消操作是请求；确认最终 cancelled 才算完成。终止无法确认、同步监控丢失或异步提交结果不确定会进入 unknown。核对本机进程或外部 job_id 的实际状态，确认后：

```sh
wakeit run resolve <run-id> --state interrupted --reason '实际核对说明'
```

支持的人工最终状态见 `wakeit run resolve --help` 和 API 返回错误，不能仅因离线把运行标记成功或已取消。重试会创建关联的新 Run，可能再次产生外部副作用；用户要重试且原执行结束确认后再操作。

输出可能明确截断或过期，报告中保留该状态，不把缺少输出解释为没有执行。默认日志/过程保留 7 天，运行记录 90 天。

用户要继续对话时，读取实际 Run 的 session.command，按 [会话](sessions.md) 提供原生 Agent 续聊命令。需要本地验证和互斥时，可选用 `wakeit run resume <run-id>`。二者均与 `run retry` 不同：续聊在本机打开已有会话，retry 重新提交一次任务；不要相互替代。
