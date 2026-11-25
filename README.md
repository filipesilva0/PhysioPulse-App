# PhysioPulse

Clinic management for one physiotherapy clinic in Portugal: patients, schedule, consents and money.
Mac and Windows desktop app (Tauri 2 + React) and a Rust server on a ZimaOS home server, reachable only over Tailscale.

**Status:** phase 1 (accounts, two-step sign-in, roles, access log, encrypted backups) is built and tested; patients, schedule and finance still show sample data.

**New here? Read [`claude/README.md`](claude/README.md).** It has the status, the spec, every decision, the architecture, the design rules, the security gates and how to run and test everything. AI assistants: read [`CLAUDE.md`](CLAUDE.md) first.

```bash
npm install
npm run build:preview && open dist-preview/index.html   # click through the UI, no server needed
npm run tauri dev                                        # desktop app (needs the server)
```
