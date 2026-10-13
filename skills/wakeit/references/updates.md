# 更新

```sh
wakeit --json update --check
wakeit update
```

GitHub 凭据与 Gateway Token 分开。依次读取 WAKEIT_GITHUB_TOKEN、GH_TOKEN、GITHUB_TOKEN 或已有 `gh auth token --hostname github.com`。公开 Release 无需 GitHub 登录；普通更新忽略预览版，首次接入使用明确的 v0.1.0 预览包。

当前 0.1.0 未正式发布，no_release 是正常状态。草稿、预发布和同版本提交不触发更新；只按正式 SemVer 比较。不要通过生成新构建编号绕开版本约定。

先检查，用户要求更新时才执行替换。更新校验 Release manifest、当前编译目标、SHA-256 和候选二进制版本；只替换当前实际执行路径，不更新 Docker 镜像或另一个 PATH 中的副本。目录不可写则说明该安装应由所有者或包管理器更新，不自行提权。

运行中的 Gateway/Runner 不会自动重启。服务升级另行备份、执行独立迁移、重启并验证 ready；保留数据库、Runner journal、NATS 身份和 Agent 凭据。容器/系统包安装用对应更新方式。
