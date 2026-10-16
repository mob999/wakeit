# Agent Logo

Agent YAML 的可选 `logo` 字段使用 Gateway 根路径或 HTTPS URL。Runner 按配置上报，WebUI 直接展示；缺省或图片加载失败时显示 Agent 显示名首字母。

```yaml
version: 1
id: custom-agent
name: Custom Agent
logo: https://example.com/custom-agent.svg
mode: sync
submit:
  command: custom-agent
```

内置资源保存在 `web/public/agent-logos/`，由 Vite 复制到构建目录，随 Gateway 二进制嵌入，离线部署不依赖外部图片服务。

| Agent | 官方来源 | 打包文件 |
|---|---|---|
| Codex | [OpenAI Codex](https://openai.com/codex/) macOS 官方应用（bundle ID `com.openai.codex`）（版本 `26.1002.52244`，构建 `13536`）的 `app.asar` 原始矢量资源 `webview/assets/codex_new-f14177b03534.svg` | `codex.svg` |
| Pi | [官方 Press Kit](https://pi.dev/press-kit) 的 [Badge](https://pi.dev/favicon.svg) | `pi.svg` |
| OpenCode | [官方 Brand 页面](https://opencode.ai/brand)及[官方 Logo 资源](https://github.com/anomalyco/opencode/tree/c1615b1db9c2e7a955f2be1024e84bb74c90cc6b/packages/console/app/src/asset/brand)，合并官方浅色和深色配色，几何形状不变 | `opencode.svg` |
| Claude Code | [Claude 官方站点图标](https://claude.ai/favicon.svg) | `claude-code.svg` |
| Antigravity CLI | [Google Antigravity 官方站点矢量图标](https://antigravity.google/favicon.svg) | `agy.svg` |
| Gemini CLI | [官方 Brand Kit](https://geminicli.com/brand-kit/) 的 [SVG 图标](https://geminicli.com/brandassets/gemini-cli-icon_full-color.svg) | `gemini-cli.svg` |
| Qwen Code | [官方固定版本的 SVG](https://github.com/QwenLM/qwen-code/blob/70758572ff16bfe7b703a25c3e3f8ac7901625b7/packages/desktop/bootstrap/qwen-code-logo.svg) | `qwen-code.svg` |
| Goose | [AAIF / Block 官方 v1.54.0 SVG](https://github.com/aaif-goose/goose/blob/v1.54.0/documentation/static/img/goose.svg) | `goose.svg` |

Pi、OpenCode、Goose 的 SVG 随页面明暗主题使用对应配色。第三方 HTTPS 图片不经 Gateway 代理，图片请求不发送页面 Referrer；不支持 Runner 本地文件路径。

内置图标均为原始 SVG，保持 path 几何不变，按实际图形边界统一 viewBox 留白。Logo 容器为 `1em`，随旁边 Agent 名称的字号变化：列表为 11px，设备 Agent 表格为 12px；首字母替代使用相同容器。没有打包经核实矢量标识的 Agent 使用首字母替代。Codex 单色标识按明暗主题使用前景色；不再使用应用 PNG 图标。
