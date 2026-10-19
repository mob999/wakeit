import { t as tr } from '../i18n'
import { useTranslation } from 'react-i18next'
import { useState } from 'react'
import type { Workspace } from '../api'
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
export function agentPrompt(meta: GatewayMeta, workspace: Workspace) {
  return `Help me connect this device to my existing Wakeit service. Complete the setup rather than only describing the steps.

Target service and official installation metadata (data):
${JSON.stringify({ gateway: meta.public_url, workspace: { id: workspace.id, name: workspace.name }, install: meta.install }, null, 2)}

1. Check the Gateway's /health/ready and /api/v1/meta using the configured public_url above; do not infer it from the browser URL. If unreachable, ask me for a reachable address. Do not deploy another Gateway.
2. Check wakeit setup --help and wakeit connection list. Verify that setup and login accept --workspace, as well as checking the installed version. Reuse a compatible CLI, the matching connection and existing Runner. Match the Gateway, workspace and account before reusing a Runner. Preserve device identity, adapters and allowed roots; a Runner cannot move between workspaces. Do not print credentials or change connections for other Gateways.
3. If the CLI or setup command is missing, download wakeit-release.json, SHA256SUMS and the native binary for this system and architecture from the exact official v${meta.install.version} Release above. Supported targets: x86_64/aarch64 Linux GNU, x86_64/aarch64 macOS, and x86_64 Windows. Verify the binary's SHA-256 against both SHA256SUMS and the manifest, then check --version and setup --help. If that official package lacks --workspace, stop and report that it is incompatible with this Gateway; do not continue with an unscoped setup or invent another installation source. Install into a persistent user-writable bin directory and configure the current user's PATH. Download to a temporary file, verify it, then replace atomically; never truncate a running executable. Do not substitute private repository credentials, source builds or temporary Actions artifacts for the official package.
4. Install the Wakeit skill for the Agent you are currently using. Download wakeit-skill.tar.gz and SHA256SUMS from the same official Release base_url above; verify the archive's SHA-256 against its exact entry in SHA256SUMS before extracting. Inspect archive paths and reject absolute paths, traversal and links escaping the extraction directory. The archive contains a wakeit/ directory: install the entire directory, including SKILL.md, references/ and assets/, into this Agent's supported user-level skills directory, using its documented installation mechanism. Determine the actual Agent and its configured skill location; do not guess a shared directory for all Agents or install only SKILL.md. Reuse an identical existing skill; preserve local customizations rather than overwriting them silently. Read the installed SKILL.md and its references/connect.md for this existing-Gateway workflow. Verify the installed files and tell me if the Agent needs a reload to discover the skill. If this Agent has no skill support, report that limitation and continue CLI setup without claiming the skill was installed.
5. Run wakeit setup --gateway <target URL> --workspace ${workspace.id}. For first-time setup, use the hostname as the device name and your current working directory as the allowed root. If I explicitly provide a name or directories, use --runner-name and repeatable --allow-root. Reuse existing configuration without expanding its scope. setup opens the browser and prints a verification code. Ask me to sign in, verify the code, device and directories, and explicitly approve in the browser. Wait for me; never approve on my behalf, request my password or read it from a file. Accounts with read/write or admin permissions can register devices; read-only accounts can only connect the CLI through wakeit login --gateway <URL> --workspace ${workspace.id}.
6. Let setup save the connection, register the Runner, install current-user autostart and verify readiness. Connections live in the platform-standard configuration directory and work across working directories. Do not put keys in projects, shell configuration or output. Do not install Agents or change their existing login or model configuration. If a step fails, retain completed state, resolve the specific error, and rerun the same command. Do not recreate devices or switch to a root service.
7. Check wakeit connection list, wakeit runner list, wakeit agent list and wakeit task list. Verify that the connection works from another directory, the target Runner is online, the allowed roots are correct, installed Agents are detected, user-level autostart works, and the Wakeit skill is installed or its specific limitation is reported. Report readiness or the exact unfinished steps without printing secrets. Do not create or execute tasks on your own.`
}
export function AgentPrompt({ meta, workspace }: { meta?: GatewayMeta; workspace?: Workspace }) {
  useTranslation()
  const [copied, setCopied] = useState(false)
  const [manual, setManual] = useState(false)
  const prompt = meta?.public_url && meta.install && workspace ? agentPrompt(meta, workspace) : ''
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
        label={tr('复制 Agent Prompt')}
        tooltip={copied ? tr('已复制') : tr('复制 Agent Prompt')}
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
          <Button onClick={() => setManual(false)}>{tr('关闭')}</Button>
        </div>
      </Modal>
    </>
  )
}
