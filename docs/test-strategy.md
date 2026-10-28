# Test ownership and visual acceptance

Keep tests that protect business behavior, data boundaries and real integrations. Do not maintain browser suites, CSS-class assertions, markup snapshots or tests that only repeat a layout constant. Use the repository's [Unit Test Review skill](../.agents/skills/unit-test-review/SKILL.md) when reviewing coverage.

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

Visual acceptance is a local review activity, not a maintained browser test suite or CI job. For a broad styling migration:

1. Freeze the original source revision and any preview overlays before editing. Preserve its dependency lock and the synthetic data used to render it.
2. Run the original and candidate separately, using the same browser build, fonts, viewport, scale, language and theme. Fix dates and API responses; wait for fonts and images and disable animations while capturing.
3. Capture representative pages and states with a disposable script. Cover Tasks, Runs, devices, presets, Moments, settings, creation flows, menus, Tooltips and dialogs; include long content, narrow layouts, both languages and both themes.
4. Compare paired images. Investigate every changed region, especially typography, wrapping, borders, whitespace, sticky headers and overflow. A migrated image must not replace the baseline to make a difference disappear.
5. Exercise scrolling, resizing, DAG dragging, focus and dialog pagination in the same local review. Reuse the current interactions without adding assertions about every element's coordinates.
6. Keep the script, screenshots, differences and review notes under `.local/`. Do not commit them, upload them to GitHub or add them to CI. The PR records the reviewed scope and any remaining limits in text.

Existing appearance defects are separate changes. A styling migration preserves the accepted baseline; it does not silently redesign the page.

## Commands

```sh
npm --prefix web test
npm --prefix web run build
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

`npm test` runs the frontend unit suite; `test:unit` remains an equivalent explicit entry. It needs no browser, Gateway, database or native Agent login. Run the affected suites after a behavioral change, and broaden verification only when shared boundaries change. Do not add a test framework, coverage quota or duplicate test just to increase the count.
