# Start here

This folder is the project's memory. Anyone (a person or an AI) should be able to continue the work from these files alone, with no other context.

## What this is, in three lines

PhysioPulse (called Clínica until 11 Nov 2026) is a clinic management app for one physiotherapy clinic in Portugal: patients, schedule, consents and money in one place. Mac and Windows desktop app (Tauri 2 + React + TypeScript) talking to a Rust server in Docker on the owner's ZimaOS home server, reachable over Tailscale until phase 3 is deployed, then at the clinic's HTTPS address (physiopulse-clinica.duckdns.org) with owner-approved devices. A second Rust service makes the clinic's public website (phase 4a) at physiopulse.duckdns.org, later physiopulse.pt. A local AI (Ollama and whisper, on the clinic's AI PC) helps the owner with notes, receipts and reports; nothing leaves the clinic. A staff iPhone app (phase 5, same code built with Tauri for iOS) shows today, the schedule and read-only patient files, and records sessions; Face ID on every reopen.

## Files in this folder

| File | Read it when |
| --- | --- |
| [STATUS.md](STATUS.md) | Always first: what's done, what's next, in order |
| [SPEC.md](SPEC.md) | You need to know what a feature should do |
| [DECISIONS.md](DECISIONS.md) | You wonder "why is it like this?" — every owner decision, with dates |
| [ARCHITECTURE.md](ARCHITECTURE.md) | You touch the server, the database, the Rust shell or the API |
| [DESIGN.md](DESIGN.md) | You touch anything visible (Apple HIG rules, tokens, patterns, copy) |
| [SECURITY.md](SECURITY.md) | Before shipping anything; it holds the release gates |
| [DEVELOPMENT.md](DEVELOPMENT.md) | Setting up, running, testing, code conventions |
| [OPEN_QUESTIONS.md](OPEN_QUESTIONS.md) | Things only the owner (or their accountant) can answer |
| [DPIA.md](DPIA.md), [PRIVACY_NOTICE.md](PRIVACY_NOTICE.md), [STAFF_GUIDE.md](STAFF_GUIDE.md) | Phase 4 gate papers (drafts for the owner): impact assessment, patient privacy notice, staff training |
| [UX-REVIEW-2026-11-23.md](UX-REVIEW-2026-11-23.md) | Planning the Nov 2026 UI/UX overhaul: prioritized list of what to change |
| [RELEASING.md](RELEASING.md) | You finished a change to the app: how to choose X.Y.Z and publish the update every computer installs from the in-app button |
| [LOCAL_AI.md](LOCAL_AI.md) | You touch the app's own C++ chat and dictation module (`src-tauri/native`) |
| [RECORDING_TEST.md](RECORDING_TEST.md) | The phase 2 gate: an invented two-person session to read aloud and judge the transcription |
| [references/](references/) | The owner's design reference images and current app screenshots |

## Who the owner is and how they like to work

- A physiotherapist in Portugal building this for their own clinic (2–3 computers, owner + therapists + receptionist). Also runs other self-hosted projects on the same ZimaOS server (an older web app called PhysioPulse, which will be retired and rebuilt as this clinic's patient portal, and "Lotus", a Tauri app whose look this app borrows).
- Writes in English, wants the app in Portuguese and English.
- Wants things shown, not described: clickable previews, screenshots, working builds.
- Wants to be asked with short multiple-choice questionnaires when decisions are needed.
- Insists on Apple HIG compliance and on the Lotus frosted-window look with this app's own colors.

## Live links (may be private to the owner)

- Feature spec document: https://claude.ai/artifact/9iZ97kLyTwkZkcW1Yr8qWU (SPEC.md here is the copy of record)
- Browser preview of the UI: https://claude.ai/artifact/G1Za7WkHRmd8fr8rZHUgfi (or open `dist-preview/index.html` after `npm run build:preview`)
