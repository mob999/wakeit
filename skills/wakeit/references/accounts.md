# WebUI 账号管理

首位管理员由 gateway bootstrap 初始化。已有管理员登录后，在 Settings → 成员直接创建账号、修改角色、启停或重置密码，也可生成邀请链接。新建账号默认 Operator、启用；密码由管理员在弹窗中填写并确认。

这些操作需要 Admin。最后一名有效管理员不能停用或降权。停用和重置密码会撤销目标账号所有登录会话与 API Key；重新启用不会恢复旧凭据，已有 Runner/Agent 执行不受影响。操作自身账号可能退出登录；先明确这些操作后果，不把启用账号当成恢复旧 Key。

自动化接入使用 POST /api/v1/users、PUT /api/v1/users/{id} 和 POST /api/v1/users/{id}/password。PUT 可独立提交 role 或 active，省略字段保持当前值。密码不写入任务、适配文件、日志或查询缓存。账号不提供永久删除。
