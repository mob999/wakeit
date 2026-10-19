# 账号与工作区成员管理

首位账号由 `gateway bootstrap` 初始化，并成为实例管理员和 Default 工作区管理员。升级已有服务时，原管理员只获得 Default 工作区管理员角色；实例管理权限需运维通过 `gateway grant-instance-admin` 明确授予。

工作区管理员在设置 → 成员邀请已有或新账号加入，调整本区角色、启停成员关系或移除成员。界面角色名称为管理员、读写、只读，接口值仍为 `admin`、`operator`、`viewer`。成员接口位于 `/api/v1/workspaces/{workspace-id}/members`，修改提交 `{role?,active?}`，省略字段保持原值。移除或停用成员只撤销本区访问、API Key 和未兑换授权，不影响其他区。最后一名有效管理员不能退出、移除、停用或降权。

全局账号创建、启停与本地密码重置位于实例账号管理页面，需要实例管理员的 Web 会话；工作区 Admin 或 API Key 不能替代该权限。接口为 `POST /api/v1/instance/users`、`PUT /api/v1/instance/users/{id}`、`POST /api/v1/instance/users/{id}/password`。停用账号或重置密码撤销其全部登录会话与 API Key；重新启用不会恢复旧凭据。GitHub-only 账号不提供本地密码重置。账号不提供永久删除。

操作自己的账号或成员关系时明确说明可能失去访问权限或退出登录。密码只保留在当前弹窗或安全输入中，不写入任务、适配文件、日志或查询缓存。已有 Runner/Agent 执行不因注册人离开工作区而删除。
