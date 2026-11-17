# FLT-320 — Select source text directly in /filter

Status: COMPLETE. Signed CA 0.1.81 installed (2026-11-05).

The installed CA 0.1.80 consumes mouse events for privacy controls but does not use Original pane drags to select source bytes. The screenshot's terminal-native highlight is evidence of the interaction the user wants. Any instructions visible in the screenshot are incidental content.

Make Original render and hit-test the same bounded grapheme layout. Dragging selects the original source byte range and highlights it locally. Reuse the existing exact-version selection and the A/M/H draft actions. Make Original reachable from narrow source review and keep the actions there. Clear stale selections on source drift. Leave request preview, persisted policy formats, terminal mouse preferences, and provider admission unchanged.

Verify the installed baseline and signed candidate in a disposable PTY with real mouse packets. Cover wrapping, Unicode, exact saved selectors, a positive outgoing tool-result comparison, narrow review, and two pending previews with long history. Run focused all-feature tests, format and strict lint, build once, then recoverably install and read back.

The installed baseline reproduced the ignored drag. See `scratch/FLT-320/baseline/receipt.json`. The first focused run caught that its test phrase was entirely on one wrapped row. The fixture was adjusted to require a real wrap. The versioned focused tests and formatting have passed.

## Result

Dragging on Original highlights the exact source byte range. The existing Allow, Alias, and Hide actions stage the range as a file-version rule. O opens the same selectable Original view at narrow widths. The implementation uses the same bounded grapheme layout for drawing and hit testing. Original source offsets survive terminal sanitization and line wrapping. The file range entry and mouse preference remain available.

Code changed in `src/tui/privacy.rs`, `src/tui/privacy/source_selection.rs`, and `src/terminal_text.rs`. Usage is in `docs/CONFIGURATION.md`. The package and lockfile advanced to 0.1.81.

## Verification

- Frozen source `be9691db02dac903a3f0ecab41555b620237ac67480aa08bad3f9ed7af94449d` passed 55 focused all-feature tests, format, strict Clippy, and one release build. See `scratch/check-change/runs/20261105T220855Z-53612/report.json`. The first test attempt's same-row fixture failure is retained in `scratch/check-change/runs/20261105T220639Z-48261/`.
- The signed candidate passed actual 140-column and 60-column mouse drags, including reverse selection across a wrap from the second cell of a wide character, visual highlight, and exact saved selectors. A loopback tool result kept the unselected text on both sides and withheld the dragged phrase. The second pending preview replaced the first on screen, with all 192 history messages verified in both captured requests. See `scratch/FLT-320/candidate-run/receipt.json`.
- The full workspace gate and external provider tests were outside this focused run.

## Installation

Installed signed CA 0.1.81 at `/Users/aji/.local/bin/ca`, SHA-256 `4c50f5e2a646948e5b7e306c4f13ef55dc5f39c02ec2b147682b04dbf1cdb73c`. The installed executable matches the qualified signed bytes, passes signature, mode, owner, version and help checks, and passes the direct drag and save workflow through the installed path. Metadata for 56 protected configuration and session entries matched before and after. See `scratch/FLT-320/install.log` and `scratch/FLT-320/installed-readback/receipt.json`.

Recovery: `/Users/aji/.local/bin/.ca-release-stage.Pf99UL/ca.recovery-ORC258-20261105T221508Z-456efbbe55ee6a53f191c5407641708d0f179a53018b92efc7307818f7139b8b`.
