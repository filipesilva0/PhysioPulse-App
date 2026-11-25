# Open questions

Only the owner (or their accountant/adviser) can answer these. Ask with short multiple-choice questions; record answers in DECISIONS.md and tick them here.

## Blocking a phase
- [ ] **Sign off phase 0 screens?** (blocks: nothing technically, but it's the phase 0 gate)
- [x] **Company or sole trader?** 5 Nov: company → certified invoicing provider API (manual route until chosen).
- [ ] **Which certified invoicing provider?** InvoiceXpress, Vendus or Moloni (ask the accountant which they work with). (Phase 3/5)
- [x] **SMS provider and sender name** — 7 Nov: none for now; reminders go from the therapist's own phone (Messages/WhatsApp prefilled). Automatic SMS later.
- [x] **Encrypted disk on ZimaOS** — 7 Nov: not for now, owner's choice (home server, Tailscale only). Revisit if the server moves or holds more than one clinic.
- [ ] **Apple Developer account and Windows code-signing certificate:** who buys them? (Phase 1 gate) — 4 Nov: the owner has a Mac too, so both are needed. 10 Nov: not for now — updates will come from the clinic server, signed with the owner's updater key; first installs show "unknown publisher".

## Accountant / adviser
7 Nov: Claude researched these — see `claude/LEGAL.md`. Still worth one confirmation from the accountant.
- [ ] Are physiotherapy sessions VAT-exempt for this clinic, and what must every invoice show? (8 Nov: the price list now marks exempt items; classes start at 23 %. Is any class therapeutic?)
- [ ] How long must financial documents be kept?
- [ ] Is a DPIA mandatory for one clinic?

## Product
- [ ] **Website (phase 4a):** approve the privacy policy and terms on the site; check the home-page texts and the team bios; decide whether group classes (23 % VAT) should show on the site. Does the old site's code or Stripe set-up mention physiopulse.duckdns.org (to change when it moves to physioplus)?
- [x] **duckdns name for the staff server** (phase 3): 11 Nov: **physiopulse-clinica.duckdns.org**; the public website on physiopulse.duckdns.org; the old site to physioplus.duckdns.org. Owner to claim physiopulse-clinica and physioplus on duckdns.org.
- [ ] **Router:** can ports 443/80 be forwarded to ZimaOS (some internet providers block them or use CGNAT)? Does the router do NAT loopback?
- [x] **Local AI model:** 9 Nov: Ollama on the clinic PC at 100.86.97.21 with `gemma4:12b`; a short test answered in 38 s (10 s model load, ~22 tokens/s). ZimaOS (i5-8500T, 15 GB, no GPU) was far too slow for 12B. Still to judge: quality of a real letter draft in pt-PT.
- [x] **Does gemma4:12b read photos?** 10 Nov: yes — the owner read receipt photos with it on Windows. No second model needed.
- [ ] **Phase 4 gate:** fill in, review and sign `claude/DPIA.md`; approve `claude/PRIVACY_NOTICE.md`; train the team with `claude/STAFF_GUIDE.md`.
- [ ] **Public holidays:** close the clinic automatically on national holidays (and the municipal one — which municipality?), or add them as closures by hand?
- [ ] **AI PC always on?** Letter drafts only work while the AI PC (100.86.97.21 on Tailscale, 192.168.1.66 at home) is awake and Ollama is running. Fine, or should it start with Windows / never sleep?
- [ ] **Consent texts:** 7 Nov: Claude wrote drafts (seeded on the server as drafts). Owner to review and approve each in Settings › Consents.
- [x] **Should reception see prescriptions (credenciais)?** 4 Nov: yes, read-only (see DECISIONS.md). Should reception also be able to *enter* them at the desk? (Phase 2)
- [x] **Which tests/questionnaires first?** 10 Nov (audit phase C): forms with automatic scoring for the free ones (NPRS, SPADI, LEFS, PSFS, KOOS); ODI, NDI and QuickDASH stay as typed totals (licensed).
- [ ] **Wording of the test forms in Portuguese:** Claude translated the English originals (`src/lib/forms.ts`). If the clinic uses a validated Portuguese version on paper (e.g. SPADI-PT, LEFS-PT, KOOS-PT), check the wording matches it and send corrections.
- [ ] **KOOS on the card:** the card shows the mean of the five subscales (each subscale is also shown). Prefer another single number (e.g. KOOS4, or just Pain), or a card per subscale?
- [ ] **ODI, NDI, QuickDASH licences:** if the clinic gets them, the forms can be added like the others.
- [ ] **Employer Social Security rate:** 23,75 % to start. Confirm with the accountant, and which staff (if any) need their own rate (e.g. 0 for someone on recibos verdes).
- [x] Final prices: 5 Nov: suggested ladder (pack of 10 €170, 4/month €72, 8/month €136).
- [ ] The single-session hook text ("30 min €35, 60 min €55") predates the €20 price — update?
- [x] App name: 11 Nov: **PhysioPulse** (app, phone app, website). The pulse icon stays. Still to do: search INPI for the trademark and pick the .pt domain. 11 Nov: phase 3 starts on a free duckdns name; the .pt name comes later (a settings change).
- [x] Patient portal: stack — 11 Nov: a separate Rust service writing plain HTML (phase 4a built); patient sign-in at /conta (phase 4b), separate from staff accounts.
