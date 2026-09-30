# Multi-repository workspace — design QA

## Reference

- Source visual: `/Users/lirangabai/.codex/generated_images/019fc6ea-c6b2-7330-a8f9-9a4d931cb773/exec-0082d174-7131-4816-9f1c-974f4e274ca1.png`
- Target: native Choro desktop UI, dark theme, 1200 × 768 window.

## States to verify

1. Git panel shows a minimal repository dropdown only when the opened folder contains multiple repositories.
2. New Agent shows `Entire workspace` before Solo/branch and allows selecting one repository.
3. Multi-repository Ship keeps settings global, uses plain repository buttons on the left, and shows only the selected repository's files/content on the right.
4. `Generate content` applies to every included repository as one operation.

## Automated evidence

- Repository discovery tests cover root/nested repositories, generated-directory exclusions, and workspace-prefixed diffs.
- `cargo test -q -p ide-app`: 316 passed.
- Focused ide-core suites: local store 41 passed, agents 29 passed, repository discovery 3 passed.
- `cargo check -q -p ide-app`: passed after the final integration.
- Three clean independent playground repositories were detected on disk at `frontend`, `backend`, and `landing`.
- A live run exposed generated iOS dependency repositories under `DerivedData`; discovery now excludes `DerivedData`, `SourcePackages`, `.build`, and `.swiftpm`, with regression coverage.

## Visual comparison history

- Pass 1: opened `/Users/lirangabai/Documents/Ritmus/Internal_tools/CHORO_MULTI_PLAYGORUND` in Choro and confirmed the parent folder and all three child repositories in the folder picker.
- Pass 2: prepared a signed isolated QA bundle for the new binary so it could be targeted independently from the installed Choro build.
- Final capture: blocked because macOS locked before the native Git/composer/Ship screenshots could be captured.

## Result

Blocked for visual sign-off only. No P0/P1/P2 visual issue was observed, but the final screenshots and pixel comparison must be captured after macOS is unlocked; automated and compile verification pass.
