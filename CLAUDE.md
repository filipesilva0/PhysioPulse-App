# CLAUDE.md

You're working on **PhysioPulse** (called Clínica until 11 Nov 2026): a Mac/Windows desktop app (Tauri 2 + React) and a Rust server (on a ZimaOS home server, reached over Tailscale) for running one physiotherapy clinic in Portugal.

**Read `claude/README.md` first.** It points to everything else: status, spec, decisions, architecture, design rules, security, how to run and test.

## Rules that always apply

1. **Apple Human Interface Guidelines, every time you touch UI.** The owner asked for this explicitly and repeatedly. See `claude/DESIGN.md`. If you have an Apple HIG skill available, read it before UI work.
2. **Bilingual.** Every user-facing string goes through `L("Português", "English")`. Portuguese (pt-PT) is the default. Portuguese uses sentence case; English buttons and menu items use Title Case.
3. **Roles are enforced on the server.** Hiding something in the UI is never enough. Only the owner sees finances, AI features, staff and the access log; reception never sees clinical information.
4. **Health data.** Never log patient data, never send it outside the clinic server (no cloud AI, no analytics), and write every read of a patient file to the access log. The access log is append-only (database trigger).
5. **The UI never sees the session token.** It lives in Rust memory (`src-tauri/src/server.rs`). All server calls go through `api()` in `src/lib/api.ts`.
6. **One command table.** Menu ids in `src-tauri/src/menu.rs` must match the `run()` switch in `src/App.tsx`.
7. **Prove it works.** Type-check (`npx tsc --noEmit`), build, run the server tests (`server/tests/e2e.py`), and screenshot UI changes in light and dark mode before saying something is done.
8. **Update `claude/STATUS.md`** when you finish a task, and `claude/DECISIONS.md` when the owner decides something.
9. **Ship it.** A change to the desktop app ends with a release (version X.Y.Z chosen by Claude), unless the owner says not to: `claude/RELEASING.md`. The repository is public: never commit secrets.
