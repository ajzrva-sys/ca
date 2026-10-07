# Cursor clean-runtime observation — blocked

Date: 2026-08-09

Planned slice: local `--help`/startup boundary, followed by a 100×30 startup/composer capture only if a clean local exit was confirmed.

Reference artifact: `oracles/cursor-CLI-reverse/cursor-agent/backup/2026-07-14/index.js` (read-only).

Environment:

- macOS arm64;
- bundled Node reports `v24.5.0`;
- empty temporary `HOME` and `TMPDIR`;
- network denied with the macOS sandbox;
- no provider/auth environment and no interaction with `ca`.

Attempted input: `--help` passed to the dated clean bundle using the bundled Node executable.

Observed result: the bundle exited nonzero before producing CLI help or a TUI. Direct execution is not a supported standalone entrypoint; its diagnostic consisted of bundled implementation text and is intentionally not retained as a fixture.

Why no fallback runtime was used:

- the dated `cursor-local` wrapper is a locally modified authless/provider launcher, references a developer-specific external installation, and contains sensitive runtime configuration;
- the live `cursor-agent/` tree is documented as patched;
- neither can establish stock Cursor behavior or a clean version.

Consequences:

- Cursor version: unavailable;
- terminal dimensions: the planned 100×30 TTY stage was not reached;
- keystrokes: none;
- user-visible Cursor output: none captured;
- parity status: blocked and `Unverified`;
- all oracle-informed behavior continues to be documented as a `ca extension`.

No oracle file was modified. The empty temporary directories used for the failed launch were removed.
