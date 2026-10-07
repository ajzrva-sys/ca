# Initial public GitHub source publication

## Purpose

User request: build ca, create thorough documentation, and push to the user's
ajzrva-sys GitHub account. The user selected Public visibility and additionally
requested TUI screenshots. The destination is `ajzrva-sys/ca`, branch `main`.

## Scope and decisions

The live application root is `ca/` under the parent workspace; it originally
had no Git metadata. Create an initial history there. Publish application and
workspace source, manifests, fixtures, tools, prompts, policies, and engineering
docs. Exclude generated output, runtime state, scratch work, and oracle/reference
archives. Preserve application behavior and historical evidence.

Add a portable README and user/developer guides rather than requiring readers
to begin with long internal continuity records. Capture the actual signed TUI
through a PTY and a loopback fixture provider; document the sample reply and
capture method. Do not label the images as live-provider validation.

The repo references `.agent/PLANS.md`, but that file is absent from the starting
snapshot. This record uses explicit purpose, decisions, validation, and outcome
sections and records the missing-file boundary in the developer guide.

## Validation

See [PUBLICATION_VALIDATION.md](../../PUBLICATION_VALIDATION.md) for command
outcomes, the first restricted test failure, the complete rerun, build identity,
source inventory checks, and screenshot receipts. Logs are retained locally in
`scratch/publication-20261006/`, which is excluded from Git.

The fresh signed release matches the installed executable byte-for-byte, so no
replacement is needed. Runtime configuration and running sessions remain intact.
Long-duration and real-model retrieval checks remain separate historical tasks.

## Outcome

README, guides, generated CLI reference, five TUI screenshots, and source import
prepared for the public repository. Final validation and publication evidence
are recorded in the linked publication report and continuity entry.
