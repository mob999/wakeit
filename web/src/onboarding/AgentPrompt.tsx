import { ActionIcon } from '../actions/ActionIcon'
import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useRef, useState } from 'react'
import type { Workspace } from '../api'
import { IconButton, Modal } from '../ui'
import { CopyValue } from '../runs/CopyValue'
import { useToast } from '../Toast'
import './prompt.css'
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
export function agentPrompt(meta: GatewayMeta, workspace: Workspace) {
  return `Help me connect this device to my existing Wakeit service. Complete the setup rather than only describing the steps.

Target service and official installation metadata (data):
${JSON.stringify({ gateway: meta.public_url, workspace: { id: workspace.id, name: workspace.name }, install: meta.install }, null, 2)}

1. Check the Gateway's /health/ready and /api/v1/meta using the configured public_url above; do not infer it from the browser URL. If unreachable, ask me for a reachable address. Do not deploy another Gateway.
2. Check wakeit setup --help and wakeit connection list. Verify that setup and login accept --workspace, as well as checking the installed version. Reuse a compatible CLI, the matching connection and existing Runner. Match the Gateway, workspace and account before reusing a Runner. Preserve device identity, adapters and allowed roots; a Runner cannot move between workspaces. Do not print credentials or change connections for other Gateways.
3. Identify the actual Agent and its configured skill directory. On macOS or Linux GNU, install any missing CLI and matching Wakeit skill with the official installer:
   curl -fsSL https://raw.githubusercontent.com/mob999/wakeit/main/install.sh | sh -s -- --version ${meta.install.version} --skill-dir <actual-agent-skill-directory>/wakeit
   Pass a correctly quoted absolute skill path. If this Agent has no skill support, omit --skill-dir and report that limitation. The script handles download, checksum verification, installation and shell PATH with progress. It preserves different existing skill files; do not overwrite local customizations. On Windows, follow the official installation guide at https://github.com/mob999/wakeit/blob/main/docs/install.md for the exact v${meta.install.version} release. Preserve the user PATH and verify it in a new shell. Do not invent another installation source.
4. Verify wakeit --version and setup --help. If that official package lacks --workspace, stop and report that it is incompatible with this Gateway; do not continue with an unscoped setup. Reuse an identical existing skill; read its SKILL.md and references/connect.md. Verify the entire skill is installed and tell me if the Agent needs a reload to discover it. Do not claim skill installation when unsupported.
5. Run wakeit setup --gateway <target URL> --workspace ${workspace.id}. For first-time setup, use the hostname as the device name and your current working directory as the allowed root. If I explicitly provide a name or directories, use --runner-name and repeatable --allow-root. Reuse existing configuration without expanding its scope. setup opens the browser and prints a verification code. Ask me to sign in, verify the code, device and directories, and explicitly approve in the browser. Wait for me; never approve on my behalf, request my password or read it from a file. Accounts with read/write or admin permissions can register devices; read-only accounts can only connect the CLI through wakeit login --gateway <URL> --workspace ${workspace.id}.
6. Let setup save the connection, register the Runner, install current-user autostart and verify readiness. Connections live in the platform-standard configuration directory and work across working directories. Do not put keys in projects, shell configuration or output. Do not install Agents or change their existing login or model configuration. If a step fails, retain completed state, resolve the specific error, and rerun the same command. Do not recreate devices or switch to a root service.
7. Check wakeit connection list, wakeit runner list, wakeit agent list and wakeit task list. From a different directory, start a fresh login shell matching the detected Unix shell (or a new Windows shell with refreshed PATH) and verify bare wakeit --version and wakeit connection list work. An absolute executable path, a temporary export, or validation in another shell does not prove the user's PATH is configured. Verify that the connection works from another directory, the target Runner is online, the allowed roots are correct, installed Agents are detected, user-level autostart works, and the Wakeit skill is installed or its specific limitation is reported. Report readiness or the exact unfinished steps without printing secrets. Do not create or execute tasks on your own.`
}
export function AgentPrompt({ meta, workspace }: { meta?: GatewayMeta; workspace?: Workspace }) {
  useTranslation()
  const [open, setOpen] = useState(false)
  const trigger = useRef<HTMLButtonElement>(null)
  const toast = useToast()
  const prompt = meta?.public_url && meta.install && workspace ? agentPrompt(meta, workspace) : ''
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
