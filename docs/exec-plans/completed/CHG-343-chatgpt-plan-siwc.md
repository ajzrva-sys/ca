# CHG-343 — ChatGPT plan (SIWC) provider

Status: Chat shim skips the provider filter, refreshes an expired plan token, and reports plan usage limits. Installed as 0.1.109. `chatgpt-shim` and `openai-chatgpt` do not open the filter preview, and the filter status row stays hidden on those routes. The local `chatgpt-shim` route stays a conversation. Other agents can call `ask_chatgpt` for one answer from that shim. Live browser sign-in is not done. Codex unchanged.

Hermes plan: `/Users/aji/.hermes/plans/2026-11-11_200926-ca-chatgpt-plan-siwc.md`

## Contract

- New provider id `openai-chatgpt`, display ChatGPT plan, starter `gpt-6.1-sol`.
- Isolated OAuth store; no Codex/API-key fallback.
- TDD selectors in the Hermes plan. Focused `check-change.py` after each green.

## Progress

- Task 0 stub
- Tasks 1–15 TDD plus aggregate Task 16
- Latest receipt: `scratch/check-change/runs/20261112T024416Z-9972/report.json`
- Remaining: live SIWC in a disposable CA_HOME (user present); signed install only if requested
