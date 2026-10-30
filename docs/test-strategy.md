# Test ownership and visual acceptance

Keep tests that protect business behavior, data boundaries and real integrations. Do not maintain browser suites, CSS-class assertions, markup snapshots or tests that only repeat a layout constant. Use the repository's [Unit Test Review skill](../.agents/skills/unit-test-review/SKILL.md) when reviewing coverage.

## Local work and CI

Use the dev preview and focused checks to resolve concrete questions during implementation. Full builds, complete suites, platform coverage, real-service integration and packaging belong to CI. Do not reproduce that pipeline locally for each edit or wait for it before continuing unrelated implementation. A development handoff may report CI as pending; merging and releasing require the current commit's required checks to pass.

For ordinary UI or copy changes, inspect the affected page and interactions through Vite hot reload. A production build is useful when investigating a bundling/type issue, not a mandatory finish step for every UI edit. Run or add a test when it protects meaningful behavior; widen checks only when a changed boundary or failure justifies it.

## CI scope

`verify` selects jobs from changed files, not PR titles or branch names. Pull requests use the base-to-head merge-base diff; pushes use the before-to-after diff. Deletions and both sides of renames are included. Manual runs and changes to CI routing/workflows run every check.

| Changed files | Checks |
| --- | --- |
| `web/**` | Frontend build and unit tests; package manifests also check release metadata |
| Markdown, `docs/**`, `.agents/**` | Documentation links/examples and bilingual changelog structure |
| `crates/wakeit-*/tests/**` | Owning package checks only; Gateway still builds embedded WebUI assets |
| CLI source/configuration, `plugins/**`, native Agent Runtime | CLI checks on Linux/macOS/Windows and service integration |
| Gateway source/configuration, database fixtures | Gateway checks on Linux/macOS/Windows and service integration |
| Core source/configuration, shared Cargo configuration/lockfile | Both products, shared core checks and service integration; package metadata also checks packaging |
| `schemas/**` | Frontend, Rust, service integration, documentation examples and packaging |
| Runtime/deployment examples and `skills/wakeit/assets/**` | Rust/service integration and documentation; Skill assets also check packaging |
| Skill instructions | Documentation and standalone Skill packaging |
| `install.sh`, `deploy-gateway.sh` and their checks | Installer checks on Linux/macOS and packaging |
| Packaging/changelog scripts | Packaging and documentation checks |
| Native-check script / service-integration scripts | Native Rust checks / service integration respectively |

Unknown code/configuration paths run all checks until their owner is added in `scripts/ci_scope.py`. CLI and Gateway have independent check jobs. The CLI job requires no WebUI assets or Gateway build; integration builds both programs to exercise their connection. Frontend-only and documentation-only changes do not start Rust or service checks.

Rust formatting runs once per affected package on Linux; compilation and behavior tests retain all three platforms. Unpublished database repair scripts are retired; published migrations and their preservation checks remain.

The frontend is built once per workflow. Gateway and integration jobs download those assets for the embedded WebUI; frontend unit tests run only when their scope is selected. The required `verify-result` job always runs and rejects failed, cancelled or unexpectedly skipped selected jobs, including failed scope detection. Unselected checks may be skipped. Release/tag builds remain independent and continue to build all published targets.

## Maintained coverage

| Contract | Owner |
| --- | --- |
| Diagnostic routing and interpolation | Frontend unit tests with independent diagnostic cases and real i18next |
| Typed preset inputs and prompt references | Frontend parser tests and Rust template compilation tests |
| Reference identity, deduplication and reading-state isolation | Frontend unit tests |
| Run status, retries, session identity and recording causes | Frontend state tests, Rust and service integration tests |
| Transactions, permissions, migrations, message delivery and process cleanup | Rust and service integration tests using the real boundaries |
| Release packaging, checksums and standalone skill contents | Packaging and native replacement checks |

Tests are redundant only when their input, outcome and boundary are the same. Similar fixtures or function names are not sufficient grounds for deletion. Preserve historical inputs for Unicode, escaping, missing data, late events, retries and restart behavior. Do not replace real database, filesystem or process behavior with a second implementation in a mock.

## Visual changes

Visual acceptance is a local review activity, not a maintained browser test suite or CI job. Ordinary UI work uses the shared dev preview. Use the following comparison process only for explicitly requested visual preservation, a broad styling migration or a specific visual regression; select the pages and states affected by that work:

1. Freeze the original source revision and any preview overlays before editing. Preserve its dependency lock and the synthetic data used to render it.
2. Run the original and candidate separately, using the same browser build, fonts, viewport, scale, language and theme. Fix dates and API responses; wait for fonts and images and disable animations while capturing.
3. Capture representative pages and states with a disposable script. Cover Tasks, Runs, devices, presets, Moments, settings, creation flows, menus, Tooltips and dialogs; include long content, narrow layouts, both languages and both themes.
4. Compare paired images. Investigate every changed region, especially typography, wrapping, borders, whitespace, sticky headers and overflow. A migrated image must not replace the baseline to make a difference disappear.
5. Exercise scrolling, resizing, DAG dragging, focus and dialog pagination in the same local review. Reuse the current interactions without adding assertions about every element's coordinates.
6. Keep the script, screenshots, differences and review notes under `.local/`. Do not commit them, upload them to GitHub or add them to CI. The PR records the reviewed scope and any remaining limits in text.

Existing appearance defects are separate changes. A styling migration preserves the accepted baseline; it does not silently redesign the page.

## Commands

These are available commands, not a mandatory local checklist. Choose the relevant check; let CI run the complete workflow.

```sh
npm --prefix web test
npm --prefix web run build
cargo fmt --all --check
cargo clippy --locked -p wakeit-cli --all-targets -- -D warnings
cargo test --locked -p wakeit-cli --all-targets
```

`npm test` runs the frontend unit suite; `test:unit` remains an equivalent explicit entry. It needs no browser, Gateway, database or native Agent login. Run the affected suites after a behavioral change, and broaden verification only when shared boundaries change. Do not add a test framework, coverage quota or duplicate test just to increase the count.
