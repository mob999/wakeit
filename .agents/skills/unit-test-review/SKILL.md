---
name: unit-test-review
description: Review or refactor automated tests for redundant coverage, weak assertions, isolation problems and maintenance cost. Use when auditing a test suite or designing meaningful unit tests; do not use for UI redesign or as a reason to add tests to every small change.
---

# Unit Test Review

Improve confidence per unit of maintenance and runtime. Fewer test lines or a higher test count are not the objective.

## Audit before editing

Read repository instructions, test commands, CI and the relevant production contracts. Run a focused baseline only when needed to understand a test change or suspected existing failure; do not make a full local baseline a prerequisite for editing.

Map each candidate to its trigger/input, observable outcome, risk and test layer. Two tests are redundant only when these match and neither protects a distinct boundary. Similar syntax, the same function, or a shared fixture is insufficient evidence.

Keep pure transformations and state rules in fast unit tests. Retain integration coverage when correctness depends on transactions, serialization, migrations, process trees, locks, permissions or real dependency behavior. This repository does not maintain browser tests. Review ordinary visual changes through the shared dev preview. Use synthetic data and disposable local screenshot comparisons only for explicit visual-preservation work, broad styling migrations or a specific visual regression; keep their scripts and images outside version control and CI. Do not relabel mocked browser tests as unit tests.

## Refactor choices

- Share repeated setup when it has one meaning. Keep scenario-specific inputs and expected outcomes visible at the call site; avoid an options-heavy universal fixture.
- Use named table cases for equivalent behavior with different boundaries. Split unrelated behaviors that currently fail under one omnibus title. Multiple assertions for one outcome are often useful.
- Remove duplicate checks only after identifying the surviving test or stronger assertion. Preserve historical regression inputs, including Unicode, escaping, late events and restart cases.
- Prefer public behavior over private fields, CSS class spelling or snapshots of incidental markup. Preserve exact status codes, error types and field names where consumers depend on them. Fixed visual dimensions belong to local visual review, not maintained assertions.
- Derive expectations independently from the implementation. Do not build a test's input list from the production allowlist when omissions are the bug to detect, or compute expected output with the function under test.
- Check cardinality before inspecting a collection. An empty collection passes every/all and a first-match assertion can miss duplicates. Verify absence too when extra output would be incorrect.
- Stub genuine boundaries, not the behavior being verified. Use the existing library for a library-integration contract instead of maintaining a second interpolation, parsing or scheduling implementation in the test.
- Await an observable condition for async work. Use controlled clocks or explicit gates when appropriate; do not replace a meaningful race with an arbitrary sleep.
- Preserve test isolation. Avoid shared mutable fixtures, real accounts, native Agent credentials or production services. Follow the repository's local-only/CI split.

Do not extract production code solely to make a trivial implementation easy to assert. Do not weaken a test or change product behavior merely to make the suite green.

## Verify and report

Use focused local checks to resolve questions introduced by the test changes; leave complete suites, platform coverage and service integration to CI. Do not repeat passing checks without a relevant new change or failure. Broaden only around affected shared fixtures or CI wiring. Investigate an assertion’s ability to catch its intended defect when there is concrete doubt, not as a mandatory mutation exercise for every test.

Report concrete redundancies removed, coverage retained, meaningful gaps fixed, commands actually run and remaining limits. Distinguish executed checks from collection or source review. Install no new framework and introduce no coverage quota unless the task justifies it.

## Reading

[TestRail: How to Write Unit Tests](https://www.testrail.com/blog/how-to-write-unit-tests/) motivates focused behavior, readable setup, deterministic execution and useful boundaries. Apply the principles with judgment: one assertion is not a numerical rule, and broadening exception checks must not discard a promised error contract.
