import { ActionIcon } from '../actions/ActionIcon'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useRef, useState } from 'react'
import type { Workspace } from '../api/types'
import { IconButton, Modal } from '../components/ui'
import { CopyValue } from '../runs/CopyValue'
import { useToast } from '../components/Toast'
import './prompt.css'
export type GatewayMeta = {
  version: string
  public_url: string
}
export function agentPrompt(meta: GatewayMeta, workspace: Workspace) {
  const shellQuote = (value: string) => "'" + value.replace(/'/g, "'\\''") + "'"
  return `Connect this device to my existing Wakeit workspace.

Gateway: ${meta.public_url}
Workspace: ${workspace.name} (${workspace.id})

1. Check the Gateway is reachable. Reuse the matching connection, compatible CLI and existing Runner.

2. Install the Wakeit CLI if needed (macOS / Linux GNU):

\`\`\`sh
curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh
\`\`\`

3. Identify the Agent you are using and its configured skills directory. If it supports skills, install the Wakeit skill separately, replacing the path below with its actual directory:

\`\`\`sh
curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh -s -- --skill-only --skill-dir "/absolute/path/to/agent/skills/wakeit"
\`\`\`

Read the installed SKILL.md and references/connect.md. Keep existing customizations and mention any required Agent reload. On Windows, use the official installation guide: https://github.com/mob999/wakeit/blob/main/docs/install.md

4. Run:

\`\`\`sh
wakeit setup --gateway ${shellQuote(meta.public_url)} --workspace ${shellQuote(workspace.id)}
\`\`\`

For a new Runner, use this device's hostname and the current directory as its allowed root. Preserve an existing Runner's settings. Ask me to complete browser authorization and wait for approval. Read-only accounts should use wakeit login with the same Gateway and workspace instead.

5. Verify wakeit works in a new terminal, the connection targets this workspace, and the Runner is online with its Agents detected. Report readiness or the specific unfinished step.`
}
export function AgentPrompt({ meta, workspace }: { meta?: GatewayMeta; workspace?: Workspace }) {
  useTranslation()
  const [open, setOpen] = useState(false)
  const trigger = useRef<HTMLButtonElement>(null)
  const toast = useToast()
  const prompt = meta?.public_url && workspace ? agentPrompt(meta, workspace) : ''
  return (
    <>
      <IconButton
        ref={trigger}
        label={tr('agentPrompt.open')}
        disabled={!prompt}
        onClick={() => setOpen(true)}
      >
        <ActionIcon name="agentPrompt" />
      </IconButton>
      <Modal
        open={open}
        onOpenChange={setOpen}
        title="Agent Prompt"
        description={tr('agentPrompt.help')}
        className="agent-prompt-modal"
        onCloseAutoFocus={(event) => {
          event.preventDefault()
          trigger.current?.focus()
        }}
      >
        <div className="modal-body agent-prompt-body">
          <div className="agent-prompt-preview">
            <CopyValue value={prompt} label={tr('复制 Agent Prompt')} toast={toast} />
            <pre role="region" aria-label={tr('agentPrompt.preview')} tabIndex={0}>
              {prompt}
            </pre>
          </div>
        </div>
      </Modal>
    </>
  )
}
