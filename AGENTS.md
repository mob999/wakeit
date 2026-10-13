# wakeit development conventions

wakeit is unreleased. Keep all product/package versions at 0.1.0 and config/message version at 1. Refactor protocols and database structures directly; do not add compatibility layers. PostgreSQL migrations must be a single complete 0001_initial.sql until release. Local runtime data and credentials must remain ignored. Before replacing local data, back it up and confirm no active Runs; never affect Agent credentials. Keep main history cohesive; use --force-with-lease only after checking the remote head. Preserve the ghdiff visual baseline in docs/design.md.

Keep product copy focused on user data, controls, and necessary operational errors or consequences. Do not add implementation notes, development history, or automatic-ID explanations to titles, forms, or footers.
