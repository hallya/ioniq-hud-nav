# Decisions

One line each, newest first. Settled decisions are not reopened without new facts.

- 2026-11-03 · Core may use serde + serde_json, vendored under `vendor/` and built offline by `scripts/core-check.sh` (CI too); no other core dependency without a new decision. `ffi/` is not vendored (79 MB) and resolves from crates.io. New fact: crates.io became reachable from the cloud workspace; Bluelink replies are nested JSON.
- 2026-11-03 · Test-first for core logic and protocols (fixtures), CI steps reproducible locally; rules in CLAUDE.md.
- 2026-11-03 · Repo, code, docs and commits in English.
- 2026-11-03 · Repo is public: free CI, nothing secret or personal ever committed.
- 2026-11-03 · ~~Core has zero dependencies~~ (superseded above); UniFFI lives in a separate `ffi` crate.
- 2026-11-03 · Chargers outside France: OpenStreetMap by default (no key); OCM re-evaluated in step 2 by measured coverage.
- 2026-11-03 · Builds and installs run on the user's Mac via Terminal (computer use); free Apple Personal Team, 7-day profiles.
- 2026-11-03 · The car guides: the app only sends the next destination through Bluelink EU; native nav drives the HUD.
- 2026-11-03 · iPhone computes everything; no server, no paid service.
