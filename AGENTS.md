# wakeit development conventions

Product/package versions follow releases (current version: 0.3.4); keep config/message version at 1. Preserve published migration baselines and add new migrations for future schema changes. Release changes require aligned package versions, installation metadata and curated bilingual changelog entries. Local runtime data and credentials must remain ignored. Before replacing local data, back it up and confirm no active Runs; never affect Agent credentials. Track each change with a GitHub Issue, work on a scope-prefixed feature branch using <type>(<scope>)/<description>, for example feat(gateway)/device-auth or fix(runner)/reconnect (do not use a codex/ prefix), and submit a pull request referencing the Issue. After a pull request is merged, delete its remote branch; keep main as the only long-lived remote branch. Never force push, rewrite pushed commits or tags, or push changes directly to main. Preserve the ghdiff visual baseline in docs/design.md.

Keep product copy focused on user data, controls, and necessary operational errors or consequences. Do not add implementation notes, development history, or automatic-ID explanations to titles, forms, or footers.

Changelogs describe only final, user-visible changes since the preceding version. The first release may summarize its core capabilities. Never include implementation progress, work logs, discussion history, test transcripts, or development diaries. Keep one entry per version and keep English and Chinese entries aligned. Curate release notes from these entries rather than generating them from commit titles.

Keep README.md in English and README-zh.md in Chinese with reciprocal language links and aligned content. Do not capture, commit or publish screenshots containing personal information, credentials, local account details or real runtime data. Use synthetic examples for any future documentation images.

## Development workflow

- Prioritize the requested behavior and visible result. Implement a direct solution; add abstractions, compatibility paths and fallbacks only for current consumers or concrete failure modes.
- For UI work, use one Vite dev server serving the active worktree directly. Keep the user on that preview and inspect the affected interactions through hot reload. Avoid copied source trees, repeated builds and backend restarts for frontend-only changes.
- Let CI own full suites, cross-platform builds, service integration and release packaging. Run local checks only to answer a concrete question about the change; do not repeat passing checks or install dependencies without a relevant change or failure.
- Ordinary UI and copy edits need focused observation, not a screenshot matrix or new tests. Use disposable local screenshot comparisons only for explicit visual-preservation work, broad styling migrations or a specific visual regression; never upload those artifacts to GitHub.
- Keep meaningful behavior tests and protections for data, credentials, permissions, migrations and execution identity. Tie backups to actual data replacement; do not add process steps or defensive code for hypothetical risks.
- Push development work and let CI run asynchronously while continuing useful work. Wait for required CI on the current commit when merging or releasing; do not reproduce the CI suite locally while waiting.
- Keep updates concise and about outcomes, concrete blockers or decisions. Avoid step-by-step CI narration and repeated requests for already-authorized work.
