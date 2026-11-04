# Working rules for this repo

Read `docs/STATE.md` first, then only what the current task needs.

## Test-first
1. **Logic in `core/` is written test-first.** Write the failing test, watch it fail, then write the code. A test that passes before the code exists is checking nothing.
2. **Protocols are tested against recorded fixtures.** Every Bluelink, routing or charger-data exchange gets a sanitized request/response pair in `fixtures/` before the parser is written. Never commit real tokens, VINs, names or coordinates of the user's trips.
3. **Every bug gets a regression test first,** reproducing it, before the fix.

## The build path is tested too
4. **Run what CI runs before pushing.** If a CI step cannot run locally (network, platform), say so in the commit and keep that change small, so the first CI run is a cheap test.
5. **Each build step has a check that fails loudly:** e.g. the bindings step must find the library it reads. Prefer a failing script to a silent wrong artifact.
6. **Walking skeleton first:** prove the whole chain (Rust → Swift → iPhone → car) end to end with trivial logic before building features on it.
7. **Simulator before phone.** No build goes to the user's iPhone until `scripts/ios-sim-test.sh` passes in CI (macOS runner) and `scripts/e2e-linux.sh` passes: the phone test is only for what a mock cannot show (Hyundai's real servers, signing, the car).

## Learning from failures
8. **Every failure gets a row in `docs/LESSONS.md`,** newest first: what happened, root cause, cost, and the rule or check added. A failure caught by a test counts too.
9. **Skim `docs/LESSONS.md` before similar work,** and turn a lesson that repeats into a rule here.

## Communication
- Code, commits and docs in English.
- Keep `docs/STATE.md` current at the end of each work session; record decisions in `docs/DECISIONS.md`.
