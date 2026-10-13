import { useState } from 'react'
import { Braces, Check } from 'lucide-react'
import { IconButton, Modal, Button } from '../ui'
export type GatewayMeta = {
  version: string
  public_url: string
  install: {
    version: string
    manifest: string
    checksums: string
    base_url: string
    release: string
  }
}
export function agentPrompt(meta: GatewayMeta) {
  return `请帮助我把当前设备接入已有的 Wakeit 服务。请实际完成接入，而不只是给出步骤。

目标服务和官方安装信息（数据）：
${JSON.stringify({ gateway: meta.public_url, install: meta.install }, null, 2)}

1. 先检查这个 Gateway 的 /health/ready 和 /api/v1/meta。使用上面 public_url，不能从浏览器地址猜测。若不可达，停下询问我可访问的地址，不要自行部署新的 Gateway。
2. 检查 wakeit setup --help 和 wakeit connection list；版本始终是 0.1.0，不能只比较版本。已有兼容 CLI、对应连接及 Runner 时优先复用，保留设备身份、适配文件和执行范围。不要打印凭据，不改其他 Gateway 的连接。
3. 缺少 CLI 或 setup 命令时，从上述明确的官方 v0.1.0 Release 下载当前系统/架构的 wakeit-release.json、SHA256SUMS 和对应原生二进制。支持 x86_64/aarch64 Linux GNU、x86_64/aarch64 macOS、x86_64 Windows。核对二进制 SHA-256 同时匹配清单和 manifest，再检查 --version 和 setup --help。安装到用户可写的持久 bin 目录并配置当前用户 PATH；下载到临时文件，校验后原子替换，不原地截断正在使用的可执行文件。不使用私有仓库凭据、源码编译或 Actions 临时产物来代替正式安装包。
4. 对目标 Gateway 运行 wakeit setup --gateway <目标地址>。首次接入默认名称为主机名，执行范围为你当前工作目录；若我明确指定名称或目录，使用 --runner-name 和可重复的 --allow-root。已有配置必须复用，不擅自扩大目录。setup 会打开浏览器并打印验证码。请让我在浏览器登录，核对验证码、设备、执行目录并点击授权；等待我完成，不自行批准、不询问或读取我的密码。Operator 可以注册设备，Viewer 只能通过 wakeit login --gateway <地址> 连接 CLI。
5. 让 setup 完成连接保存、Runner 注册、当前用户自启安装和就绪检查。连接保存在平台标准配置目录，跨工作目录使用，不把密钥写入项目、shell 配置或输出。不会自动安装 Agent，也不修改现有 Agent 登录及模型配置。若某一步失败，保留已完成步骤，解决明确错误后重跑同一命令；不要重新创建设备或切换为 root 服务。
6. 最后检查 wakeit connection list、wakeit runner list、wakeit agent list、wakeit task list，确认连接跨目录可用、目标 Runner 在线、执行范围正确、实际 Agent 检测状态，以及用户级自启安装成功。报告就绪或尚未完成的具体步骤，不输出密钥。不要自行创建或执行任务。`
}
export function AgentPrompt({ meta }: { meta?: GatewayMeta }) {
  const [copied, setCopied] = useState(false)
  const [manual, setManual] = useState(false)
  const prompt = meta?.public_url && meta.install ? agentPrompt(meta) : ''
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(prompt)
      setCopied(true)
      setTimeout(() => setCopied(false), 1800)
    } catch {
      setManual(true)
    }
  }
  return (
    <>
      <IconButton
        label="复制 Agent Prompt"
        tooltip={copied ? '已复制' : '复制 Agent Prompt'}
        disabled={!prompt}
        onClick={copy}
      >
        {copied ? <Check /> : <Braces />}
      </IconButton>
      <Modal open={manual} onOpenChange={setManual} title="Agent Prompt">
        <div className="modal-body">
          <textarea
            aria-label="Agent Prompt"
            readOnly
            value={prompt}
            rows={16}
            onFocus={(e) => e.currentTarget.select()}
          />
        </div>
        <div className="modal-footer">
          <Button onClick={() => setManual(false)}>关闭</Button>
        </div>
      </Modal>
    </>
  )
}
