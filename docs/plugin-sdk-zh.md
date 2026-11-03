# 插件 SDK

[English](plugin-sdk.md)

通用扩展为插件包增加事件处理器、生命周期 Hook、动作和查询 Provider，由 Runner 作为独立进程管理。扩展不在 Gateway 进程内运行，也不要求包含 Agent 适配器。

安装和来源身份沿用[插件包](plugins-zh.md)。包可以提供 `agents`、可执行 `commands`、`extensions` 中的一种或多种资源。每个实例在自己的 Runner 上保存配置、凭证和私有数据；代码可以共享，工作区之间不共享实例数据。

## 声明扩展

在 `wakeit-plugin.json` 中声明入口、配置 schema、凭证槽、权限和回调 ID。完整示例见[英文指南](plugin-sdk.md#describe-an-extension)。

配置采用 JSON Schema，由设备验证，不下载远程 schema 引用。运行时注册的回调必须与 manifest 一致。声明能力不等于获得授权；管理员授予权限，并将事件和 Hook 绑定到指定实例。

`wakeit schema plugin-sdk` 提供公共数据结构、事件和 Hook 目录。Agent YAML 及输出事件保持 version 1；通用扩展 IPC 独立协商协议版本。

## 编写扩展

TypeScript SDK 位于 [`sdk/typescript`](../sdk/typescript)。插件作者将 SDK 打包进分发的程序资源；安装时 Wakeit 不执行 npm install 或构建脚本。Node 22.18+ 可运行可擦除类型语法的 TypeScript，也可以分发编译后的 JavaScript。

```ts
import { Plugin, patch } from '@wakeit/plugin-sdk'
const plugin = new Plugin()
plugin.on('completed', async (event, ctx) => {
  await ctx.storage.set('last-event', event)
})
plugin.hook('prepare', async (input, ctx) => {
  const settings = await ctx.config.get<{prefix: string}>()
  return patch({prompt: `${settings.value.prefix}\n${input.prompt}`})
})
plugin.action('inspect', async (_, ctx) => (await ctx.config.get()).value)
await plugin.serve()
```

`completed`、`prepare`、`inspect` 是 manifest 中声明的处理器 ID。通过 `plugin.provider` 注册查询 Provider，通过 `plugin.lifecycleHandler` 注册固定生命周期回调。

Rust 使用 [`wakeit-plugin-sdk`](../crates/wakeit-plugin-sdk)。`Plugin` 注册回调，`Context` 提供调用身份、取消信号和类型化宿主调用。原生二进制沿用平台 artifact 分发。

其它语言可以实现 stdin/stdout 上的逐行 JSON-RPC 2.0。宿主发送 initialize、生命周期和 invoke；扩展在处理期间可反向调用宿主。请求 ID 关联乱序响应。标准输出只承载协议，诊断使用结构化日志。

## 事件和 Hook

事件表示已提交事实。Gateway 负责 Task、Run、DAG，Runner 负责本机执行。`run.finished` 对应普通运行或 DAG 整轮结算，不是每个子节点 attempt。排队运行也可能直接结束；run.started 表示核心运行进入执行准备窗口，execution.started 才确认原生命令已启动。unknown 不表示完成。DAG 恢复后再次完成会产生新事件。

重试保留事件 ID；显式重放建立新的 delivery 和幂等键。外部副作用可能在进程断连前已经完成，处理器应使用服务提供的幂等能力，并准确报告不确定结果。

完成事件关联当次不可变快照。投递只携带小型元数据和快照引用，读取保存的结果，而非可能已被重试修改的当前 Run。投递期限与正文保留期明确区分。

Hook 在业务操作提交前返回 allow、deny 或 patch，各阶段有明确的可修改字段。执行准备可修改 prompt 和已声明参数，不能替换 Runner、Agent、工作目录、会话或运行身份；变换后重新执行核心验证。

Hook 按配置顺序执行。Gateway 调用设备 Hook 时不持数据库行锁。不可用时按绑定的拒绝、等待或显式跳过策略处理；业务 deny 不自动放行。Hook 上下文只允许读取宿主 API，外部副作用由提交后的事件或独立动作完成。

## 设备配置

入口为“设备 → 指定设备 → 插件”，用于实例配置、凭证、处理器绑定和活动查询。CLI 使用同一管理契约：

```sh
wakeit --json plugin instance --runner RUNNER_ID catalog
wakeit --json plugin instance --runner RUNNER_ID create --file instance.json
wakeit --json plugin instance --runner RUNNER_ID configure --file settings.json
```

创建参数包含 source、extension、name、config 和 grants；提供 instance_id 可在重试时保持身份。保存配置包含 instance_id、update.operation_id、expected_revision、config 和 secrets。结构化示例见[英文指南](plugin-sdk.md#device-configuration)。

秘密更新明确区分 keep、replace、clear；读取只返回是否配置。新值通过在线认证请求发送到 Runner，不保存在 Gateway 操作或事件队列中。扩展是受信本机代码，SDK 不提供操作系统沙箱。

调用固定代码版本和普通配置修订，修改不会覆盖已有投递的执行上下文。更新保留版本资源，升级验证配置和权限后再切换；失败候选不替换正常版本。更新包与激活实例是两个操作。

网页安装、更新和删除代码包需要设备所有者明确允许：在 DATA_DIR/plugins-policy.json 中设置 `{"remote_package_management":true}`。配置和实例管理不等于允许任意远程 Shell。

## 宿主 API 与交互

宿主调用使用具名业务方法，不开放任意携带身份的 HTTP。命名空间授权和当前工作区成员权限共同生效；用户动作保留发起者权限。插件不获取 Gateway 或 Runner Token。

SDK 提供配置、凭证槽、私有修订存储、业务查询／修改、事件、Hook、动作、Provider、外部 HTTP 和受管理的子进程。修改使用请求幂等键及适用的对象 revision。HTTP、子进程、授权流程分别要求 http:request、process:execute、auth:manage。

动作可以请求 schema 表单、选项、确认、秘密输入、授权地址、设备码及进度，由 WebUI 共享控件展示。关闭页面不代表取消；取消是明确的宿主操作。

OAuth 支持授权码＋PKCE 和设备授权，verifier 和 Token 保存在 Runner。Gateway 验证短期 state 并转发授权响应。供应商要求仍然适用；没有可达回调时需要供应商支持设备码或本机流程。Token 刷新和撤销使用明确的凭证操作。

## 通用能力验证

[`fixtures/plugins/generic`](../fixtures/plugins/generic) 是使用 TypeScript SDK 的合成插件，验证配置、凭证不回显、allow/deny/patch、宿主回调、耐久投递与重放、结构化动作和取消。不发送外部消息，不使用真实业务凭证。

完整覆盖和语义见 [RFC 4](rfc-4-plugin-sdk.md)。

## 待完成操作与公开过程流

存在远程 Hook 的工作区操作会返回 HTTP 202 和操作回执。CLI 和 WebUI 跟踪回执直到取得已提交结果；SDK 可通过 runtime.operation 查询、runtime.cancel 取消。已取消的提案不能在稍后提交，权限和 Hook 配置修订会在提交事务中再次检查。

TypeScript 使用 `ctx.streams.events(runId, {after, kind, signal})`，Rust 使用 api::EventCursor，按游标读取公开过程。这沿用有保留期的执行记录，不为每个 token 建立普通耐久回调；截断与到期标记仍然可见。完成事件继续采用耐久投递。

files.stat/read 提供修订校验的分块读取；files.watch/unwatch 仅监听明确授权的目录，消费落后时报告缺口。artifacts.register/export 建立授权引用，下载交互使用该产物 ID，不会把另一台设备的路径当作本地文件。
