# Repository instructions for coding agents

Implement the user's requested outcome directly. Read the relevant code and docs first, keep the change small, and avoid unrelated refactors, speculative defenses and new abstractions. Contributor setup and commands are in [CONTRIBUTING.md](CONTRIBUTING.md); product usage is in [README.md](README.md) and `docs/`.

## Protect data

- Keep runtime data, local configuration, tokens, credentials, private prompts and user reports out of Git, logs and chat output. Use synthetic data for screenshots and UI checks.
- Preserve Gateway databases, NATS data, Runner identities, Agent credentials and existing connections. Do not reset or re-enroll them for a check.
- Do not restart or replace user services for frontend-only work. Before replacing real runtime data, confirm that no relevant Run is active and make a backup. Routine source edits and disposable test stacks need no backup.
- Keep fixtures, screenshots, comparison scripts and validation output under ignored `.local/` paths. Never upload them to GitHub.

## Git and delivery

- Create or link an Issue for delivered changes. Branch from the latest `main` with a type/scope prefix such as `feat(gateway)/device-auth`, `fix(runner)/reconnect` or `docs(agents)/rules`.
- Open a PR linked to the Issue. Never push directly to `main`, force-push, rewrite pushed commits or rewrite published tags.
- Keep review fixes as additional commits. After merge, delete the remote branch; `main` is the only long-lived branch.
- Keep private validation records, development diaries and implementation notes out of product docs, changelogs and PR descriptions.

## Implementation and verification

- CI owns full suites, platform matrices, service integration and release packaging. Run local checks only for a concrete question; do not repeat passing checks or reproduce unrelated CI jobs.
- CI selects jobs from changed paths through `scripts/ci_scope.py`. Keep frontend-only, documentation-only and product-specific changes scoped so unrelated jobs stay skipped.
- For WebUI work, use one Vite server serving the active worktree and inspect the affected page through hot reload. Do not copy source or rebuild/restart the backend for frontend-only edits.
- Do not maintain browser tests, CSS-position assertions, markup snapshots or tests that only repeat layout constants. Test business behavior, data boundaries, permissions, migrations, execution identity and real integrations; use focused unit tests for pure logic.
- Use local screenshot comparisons only for broad visual migrations or a specific visual regression, always with synthetic data.
- Update both English and Chinese UI resources. Do not translate task instructions, reports, logs, paths, model IDs or third-party responses. Preserve the ghdiff visual baseline in `docs/design.md`.

## Data and source boundaries

- Keep configuration and message protocol version `1` unless a protocol migration is explicitly required. Never edit a published database migration; add a new migration and checks.
- When a public API changes, update the handler, types, OpenAPI/schema description and user-facing docs together.
- Gateway owns HTTP in `api/`, identity in `auth/`, persistence/background state in `store/`, NATS in `transport/`, and shared state in `state.rs`.
- CLI owns parsing in `commands/`, device execution in `runner/`, and native Agent integration in `adapters/`; keep `main.rs` as the entry point. Shared task, Agent, DAG and protocol types belong in `wakeit-core`.
- WebUI owns router setup in `app/`, HTTP/DTOs in `api/`, shared UI in `components/`, and feature behavior in its feature directory.
- Keep these directories shallow and responsibility-based. Do not add forwarding modules, blanket re-exports or wrappers solely to reorganize files.

## Product docs and releases

- User-facing copy describes data, controls and necessary consequences. Do not expose internal IDs, migration details, CSS/layout terms, automatic-ID explanations or developer history.
- Changelogs describe final user-visible changes since the previous release. Keep English and Chinese entries aligned; omit refactors, CI, tests, migrations and development progress. A Fix entry must correct behavior from an earlier release.
- Gateway/WebUI and CLI/Runner are independent products; Runner and the Wakeit Skill ship with the CLI. Use `gateway-vX.Y.Z` and `cli-vX.Y.Z` tags, update only the affected product, and publish through `.github/workflows/release.yml`.
- Wait for required CI before merging or releasing. Never rewrite a published release or tag; keep historical release assets available.
