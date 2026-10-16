# GitHub Copilot CLI native text contract

Checked 2026-10-16 using [Running GitHub Copilot CLI programmatically](https://docs.github.com/en/copilot/how-tos/copilot-cli/automate-copilot-cli/run-cli-programmatically) and the [programmatic reference](https://docs.github.com/en/copilot/reference/copilot-cli-reference/cli-programmatic-reference).

Piping the prompt activates non-interactive mode. Passing `-p` as well would discard the piped input, so the YAML deliberately omits it. `--silent` suppresses metadata and statistics; `--no-ask-user` prevents clarification questions. These flags do not grant tool permissions. The user's native saved permission settings remain authoritative; the integration does not enable `--allow-all` or assume that every requested tool is approved.

GitHub's [Actions usage documentation](https://docs.github.com/en/copilot/how-tos/copilot-cli/use-copilot-cli-in-actions) explicitly states that non-interactive runs cannot present approval prompts and automatically deny actions that are not pre-approved. Configure the needed native tool permissions before submitting a task. A normal CLI exit records process completion; this text integration does not inspect or infer whether every requested edit or command actually succeeded. The dynamic-workflow exit-code table is not used for ordinary prompts.

The documented `--output-format json` emits JSONL, but the public ordinary-prompt reference does not promise the full per-event schema. The dynamic-workflow `workflow.result` schema and workflow exit-code table describe a different command and must not be substituted for prompt mode. The official SDK has typed events, but this integration does not assume that its separate transport contract guarantees identical CLI output. Copilot therefore uses the existing plain-text output contract, with native process exit determining Run success. No tool trace, native model discovery, or complete native session contract is advertised. `--model` remains an optional explicitly selected argument.

Compatibility is checked by the required native help flags, rather than a guessed minimum version or static model list. The optional model value is passed as one argv entry. The regression test exercises actual process execution with a fixed local recording helper, proving that multiline prompts and shell-looking model values remain literal. It does not run Copilot.

No inference, installation, login, session inspection, or credential reads were performed to produce these files.
