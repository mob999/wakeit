# Test ownership and review

Use [.agents/skills/unit-test-review](../.agents/skills/unit-test-review/SKILL.md) when auditing or refactoring the suite. It is a development skill, independent of the packaged Wakeit operation skill. Copy the whole directory to your Agent's skills directory to install it.

## Layers

| Contract                                           | Primary owner                            | Integration protection to retain                                                    |
| -------------------------------------------------- | ---------------------------------------- | ----------------------------------------------------------------------------------- |
| Diagnostic routing                                 | `web/tests/unit/api-error-text.test.ts`  | Toast rendering and permission errors in browser/API checks                         |
| Translation keys and interpolation                 | `web/tests/unit/locale-contract.test.ts` | Live language switching and accessible labels                                       |
| Recording causes                                   | `web/tests/unit/recordingInfo.test.ts`   | Runner diagnostics, log persistence and toolbar presentation                        |
| Summaries, payload previews, folding and following | `web/tests/unit/reading.test.ts`         | Polling, late events, scroll anchors and rendered Markdown                          |
| Reference identity and reading state               | `web/tests/unit/report-reading.test.ts`  | Sanitized DOM extraction, real image/link clicks, layout and execution ownership    |
| Native adapter normalization                       | `crates/wakeit/tests/*_adapter.rs`       | CLI framing, actual exit status and pipe draining                                   |
| Session and DAG lifecycle                          | Rust and service integration suites      | Transactions, local locks, migration compatibility, restart and concurrent requests |
| Task Template compilation and instantiation        | `crates/wakeit/tests/task_templates.rs`  | Real migrations, cross-Gateway idempotency, authorization, frozen Runs and CLI      |
| Template inputs and generated-preview interaction | `web/tests/templates.spec.ts`            | Typed drafts, stale responses, graph/table search, bindings and responsive layout   |
| Card and toolbar appearance                        | Local browser suites                     | Geometry, transparency, keyboard access, themes and narrow layouts                  |

The same risk can justify tests at two layers when they exercise different boundaries. Do not remove service tests in favor of mocks for database constraints, authorization, NATS delivery or process-tree cleanup.

## Review rules

List input, outcome and boundary before deleting a test. Share repeated fixture construction or assertions rather than deleting distinct scenarios. Use descriptive table cases so failures name the input that failed. A complete final-report assertion checks exactly one record, its kind and its value; checking only the first result or applying `all` to an unchecked collection cannot protect this contract.

Expected cause lists come from the protocol contract, independently of the production allowlist. Diagnostic routing is checked once independently of language; catalogs and real i18next interpolation are checked separately. No test-local interpolation implementation is needed.

Pixel dimensions and transparent backgrounds that encode the design specification are intentional visual contracts. Large-output, session identity, literal argv/stdin, cancellation and retry tests protect different failure modes even when their setup looks similar. Keep those scenarios.

## Commands

```sh
npm --prefix web run test:unit
npm --prefix web run build
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```

Frontend unit tests type-check with `web/tsconfig.unit.json`, then use Node's existing test runner. They need no Gateway, browser, database or native Agent login. CI runs these on the existing OS matrix. Browser suites stay local-only; use the isolated test stack described in [Contributing](../CONTRIBUTING.md). Test collection is not evidence that a browser scenario passed.

For a previously weak assertion, a reversible mutation can check whether the test detects the defect. Restore the source in a `finally` block and rerun the clean suite. Mutation checks are diagnostic tools, not a test-count or coverage target.

The review approach draws on [TestRail's unit-testing article](https://www.testrail.com/blog/how-to-write-unit-tests/). Preserve exact error and schema contracts when consumers depend on them, and use as many assertions as one coherent outcome requires.
