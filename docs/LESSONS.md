# Lessons

Every failure, newest first: what happened, why, what it cost, and the rule or check that prevents a repeat. Skim before similar work.

| Date | What happened | Root cause | Cost | Prevention |
| --- | --- | --- | --- | --- |
| 2026-11-03 | A watcher reported the push "still running" after it had finished | `pgrep -f "git push"` matched the watcher's own command line | One false alarm | Wait on a process by PID or on its exit, never by `pgrep -f` on a string the watcher itself contains |
| 2026-11-03 | Rewrite of the project brief refused | The Projects tool only rewrites a doc whose current text was read in this conversation | One round trip | `project_read` a project doc before `project_write` |
| 2026-11-03 | crates.io blocked in the morning, reachable in the evening session | Workspace network allowlist differs between sessions | A zero-dependency design driven by a temporary limit | Check registry access at session start; core deps vendored so either state works (`scripts/core-check.sh`) |
| 2026-11-03 | openrouteservice `/plans/` looked up from Claude's side, but the user saw a 404 | A fetch result can come from a cache; it does not prove a page is live | One wrong answer, one round trip | Check a page's live state in the browser pane before telling the user it works |
| 2026-11-03 | Click on "Create repository" (public) refused by a safety check | Creating public content needs the user's own click | One round trip | Plan public creations (repos, posts) as a user click, with the form pre-filled |
| 2026-11-03 | CI bindings step failed: `libhudnav_ffi.so` not found | `ffi` built only `staticlib` + `lib`; UniFFI library mode reads a `cdylib`. The step could not run locally (crates.io blocked) | One CI round trip, ~3 min | CLAUDE.md rules 4 and 5; same command in CI and `scripts/ios-build.sh` |
| 2026-11-03 | Diagnostic test failed: reason sanitizer kept 16 chars of a whole message | Sanitizer filtered characters instead of keeping only the leading code; part of a token could leak | Caught by the test before push | Sanitizers keep the minimum by default (leading code only) |
| 2026-11-03 | crates.io unreachable from Claude's cloud workspace | Network allowlist of the workspace | Restructure into a dependency-free core + separate `ffi` crate | Check registry access at session start; see newer row for the vendored setup |
