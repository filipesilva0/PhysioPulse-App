# Releasing an update

Owner, 25 Nov 2026: "make it so i can update the app through an in app button. it can fetch the updates from github, but make it safe. i wanna essentially be able to tell you to do something and you release an update there so i can auto download it. (x.y.z) x= major update, y= minor update, z=patch, you choose all of those."

So: **when the owner asks for a change to the desktop app, the job ends with a release**, unless they say not to. Claude picks the version number.

## How it works

```
Claude changes the code ─► tools/release/publish.ps1 -Version X.Y.Z  (on the owner's PC)
   bumps the version, checks for secrets, commits, tags vX.Y.Z, pushes to GitHub
        ─► GitHub Actions (tools/release/release.yml, copied to .github/workflows/), ~20-30 min
           builds the Mac app (Apple silicon, with the on-device AI module) and the Windows installer,
           signs the update files with the owner's updater key, publishes the GitHub Release + latest.json
        ─► every installed PhysioPulse: Settings › Overview › "Check for Updates" (also checked after
           sign-in and twice a day) ─► "Update…" ─► download, signature check, install, restart
```

- **Repository:** https://github.com/filipesilva0/PhysioPulse-App, **public** (owner's choice, 25 Nov 2026: code and installers in one public repo). Never commit secrets: `publish.ps1` refuses key files, `.env` files, private keys and tokens, and files over 50 MB. Patient data never lives in this folder.
- **Update address** (compiled into the app): `https://github.com/filipesilva0/PhysioPulse-App/releases/latest/download/latest.json` (`src-tauri/tauri.conf.json › plugins.updater`).
- **Code:** `src-tauri/src/updates.rs` (commands `update_check`, `update_install`), `src/lib/updates.ts` (shared state, launch check), Settings › Overview › App row (`AppUpdates` in `src/pages/Settings.tsx`), menu item **Check for Updates…** (macOS app menu; Help on Windows; command id `check_updates`).

## Why it's safe

1. **Signature.** Each update file is signed with the owner's private updater key (minisign, made by `tools/release/new-signing-key.ps1`). The public half is compiled into the app. The app refuses any download whose signature doesn't verify, so a hacked GitHub account, release or download link can't push code to the clinic computers without that key.
2. **Signed version.** `requireSignedVersion: true`: the version is inside the signature (`tauri signer sign --app-version`, in the workflow), so a tampered `latest.json` can't offer an old, validly signed release under a new number.
3. **No downgrades.** The app only offers a version newer than its own.
4. **HTTPS only**, from github.com. The check sends nothing but the request (no clinic or patient data).
5. **The private key never touches the repo or Claude.** It's in `%USERPROFILE%\.physiopulse\updater.key` on the owner's PC (back it up with its password) and in the repository secrets `TAURI_SIGNING_PRIVATE_KEY` / `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. If it's lost, each computer needs one manual install of a build with a new key.
6. **Not yet:** Apple Developer ID / notarisation and a Windows code-signing certificate (no purchase for now, DECISIONS 10 Nov). The Mac app is ad-hoc signed; Windows shows "unknown publisher" on the first manual install only. Updates installed by the app aren't affected by either.

## Choosing the version (X.Y.Z)

The current version is in `package.json`. Compare with the last release (`claude/releases/`):

| Bump | When | Example |
| --- | --- | --- |
| **Patch** Z (0.15.0 → 0.15.1) | Only fixes, wording, small visual tweaks. Nothing new to learn. | "the receipt total was rounded wrong" |
| **Minor** Y (0.15.1 → 0.16.0) | Something new or noticeably changed, and everything that worked still works the same way. | a new screen, a new button, a new setting |
| **Major** X (0.16.0 → 1.0.0) | A big step that changes how people work or needs the server updated at the same time; also 1.0.0 when the owner calls it ready for real patient data (SECURITY.md gates). | redesigned navigation, a new required server version |

Several changes in one release: the biggest one decides. When unsure between two, take the bigger. Never reuse or go back.

## Steps for Claude (every release)

1. Finish and check the change as usual (CLAUDE.md rule 7: `npx tsc --noEmit`, build, tests, screenshots).
2. Choose X.Y.Z (table above).
3. Write `claude/releases/vX.Y.Z.md`:
   ```markdown
   # PhysioPulse X.Y.Z

   One or two short sentences in Portuguese (pt-PT) for the clinic: what's new. The app shows this paragraph next to "Atualizar".

   ## Changes
   - English details for the owner and for the next Claude.
   ```
4. Update `claude/STATUS.md` (what shipped in X.Y.Z).
5. Publish from the owner's PC: start a Remote Control session on the clinica folder and run
   `powershell -ExecutionPolicy Bypass -File tools\release\publish.ps1 -Version X.Y.Z`
   (or ask the owner to run it). It stops with a clear message if something's missing or looks secret.
6. Watch https://github.com/filipesilva0/PhysioPulse-App/actions until "Release" is green (~20-30 min). If a build fails, fix it, then release the next **patch** (a tag is never moved or reused).
7. Tell the owner: the version, what changed, and "Definições › Visão geral › Procurar atualizações" (or wait for the notice).

The server (ZimaOS) is not updated by this; server changes still deploy as in DEVELOPMENT.md.

## Setup (once)

Owner:
1. Create the **public** repository `PhysioPulse-App` on GitHub (empty: no README, no .gitignore, no licence).
2. Run `tools\release\new-signing-key.ps1` (right-click › Run with PowerShell): makes the key (asks for a password), writes the public key into `tauri.conf.json`, copies the private key to the clipboard and opens the repository's secrets page. Add `TAURI_SIGNING_PRIVATE_KEY` (paste) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (the password). Back up the key and password.
3. Tell Claude; Claude publishes the first release (0.15.0) with `publish.ps1`, which also connects this folder to GitHub (Git asks to sign in to GitHub once, in the browser).
4. **Install 0.15.0 by hand once on each computer** (older copies have no updater): Windows `PhysioPulse-windows-x64-setup.exe`, Mac `PhysioPulse-mac-apple-silicon.dmg` from the release page. On the Mac, because the app isn't notarised: drag to Applications, then the first time right-click › Open (if macOS says it's damaged: `xattr -cr /Applications/PhysioPulse.app` in Terminal, once). From then on, updates come from the button.

## Files

| File | What |
| --- | --- |
| `tools/release/release.yml` | The GitHub Actions workflow: build (newest macOS on Apple silicon + Windows), sign, publish. **Edit this copy**: `publish.ps1` copies it to `.github/workflows/release.yml` (Claude's folder tools can't write inside `.github`). |
| `tools/release/publish.ps1` | Bump, secret check, commit, tag, push |
| `tools/release/bump.mjs` | `major` / `minor` / `patch` / `X.Y.Z` in package.json, package-lock.json, Cargo.toml, Cargo.lock, tauri.conf.json |
| `tools/release/new-signing-key.ps1` | Once: the updater key pair |
| `claude/releases/vX.Y.Z.md` | Release notes (first paragraph shown in the app) |

Release files (stable names): `PhysioPulse-mac-apple-silicon.dmg`, `PhysioPulse-mac-apple-silicon.app.tar.gz` (+ `.sig`, the update), `PhysioPulse-windows-x64-setup.exe` (+ `.sig`, first install and update), `latest.json`.
