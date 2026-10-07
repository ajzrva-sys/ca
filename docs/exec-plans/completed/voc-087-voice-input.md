# VOC-087 — Voice Input

Status: COMPLETE
Owner/session: current Codex task
Last updated: 2026-08-12
Archive: completed after all focused, repository-wide, release, and disposable-TUI gates passed.

## Goal

Add explicit push-to-talk voice input that records only after a deliberate user action, transcribes through one configured local or provider backend, and inserts the bounded transcript into the composer as an editable draft requiring ordinary submission.

## Oracle Boundary

- The reviewed Ante documentation does not describe a voice-input TUI.
- Ante's hosted API documentation explicitly excludes audio endpoints.
- This is an oracle-informed `ca` extension, not Cursor or Ante parity.

## Non-Goals

- Always-on listening, wake words, background microphone access, TTS, or automatic submission.
- Persisting audio in configuration, sessions, snapshots, usage records, or diagnostics.
- Installing microphone/transcription runtimes or weakening provider/secret boundaries.

## Characterization

- `tui::event` currently discards key-release events, so portable hold/release semantics cannot be assumed. The visible interaction will use an explicit press-to-start / press-to-stop Space control and label that behavior honestly.
- The composer already owns a process-only editable input draft; completed transcription should be inserted at its UTF-8 cursor without appending a canonical message.
- Provider runtime credentials are resolved only at orchestration boundaries; the new provider transcriber must keep that rule and expose only typed secret-free failures.
- No current audio dependency or runtime state exists.

## Design

- Add disabled-by-default `VoiceConfig` with a closed backend enum: local command or configured provider.
- Local commands use an executable plus argument vector with one `{audio}` placeholder, no shell, a bounded timeout, cleared sensitive environment, bounded UTF-8 stdout, and a process-owned temporary WAV removed after the attempt.
- Provider transcription uses one exact configured provider and model, a fixed `/audio/transcriptions` path, multipart upload, the existing credential source, strict deadlines, bounded JSON/text response parsing, and secret-free typed failures.
- macOS capture uses the default input device behind a small typed capture boundary. Audio is capped by duration and bytes and encoded as mono PCM WAV only in process memory; unsupported platforms report a safe unavailable status.
- `/voice` opens a modal. Space starts and stops recording, Esc cancels/discards, and completed text is inserted at the existing composer cursor. Transcription never submits.
- One generation-scoped operation may run at a time. Cancellation and newer recordings discard stale audio/results.
- Bare mode may use voice only when configured in the normal config, but no voice setting mutation is added in this slice.

## Acceptance

- Command discovery/availability, config validation/redaction, disabled state, modal controls, cancellation/stale generation, draft insertion, and session/request immutability have focused tests.
- Capture/transcriber tests use fakes, generated WAV, temporary files, and loopback HTTP only; no real microphone, provider, config, Keychain, or session is touched.
- Provider requests contain exactly one bounded audio part and model, omit authorization for credentialless providers, and classify auth/access/rate/timeout/transport/invalid response safely.
- Local commands receive only the temporary audio path, cannot invoke a shell, time out with process cleanup, and never retain the WAV.
- Wide/narrow deterministic render snapshots cover disabled, ready, recording, transcribing, failure, and draft-ready states.
- Focused tests, full tests, formatting, strict Clippy, release build, and help/version/profile smokes pass before completion.

## Progress

- [x] Review oracle boundary and current input/config/provider seams.
- [x] Add failing characterization tests and typed configuration/runtime boundaries.
- [x] Implement capture/transcription orchestration and modal state machine.
- [x] Add render/input/request/cleanup coverage and a hermetic walkthrough.
- [x] Run repository-wide gates and reconcile project documentation.

## Completion Record

Completed on 2026-08-12. `cargo test voice --no-fail-fast` passed 22 tests, and the repository-wide gate passed 1054 unit tests plus 33 CLI tests, formatting, strict Clippy, and the release build. Release help/version/named-profile smokes passed in a disposable home. A released TUI walkthrough proved command discovery, disabled guidance, and the configured Ready modal; it deliberately did not press Space, so no microphone permission, audio capture, or provider request occurred. Generated WAV, fake process trees, and loopback HTTP cover capture/transcription behavior hermetically. The disposable root was moved recoverably to `/Users/aji/.Trash/ca-voc087-smoke-20260812`; real config, Keychain, sessions, microphone input, providers, oracle files, and the installed binary were untouched.
