# Design

The owner requires **Apple Human Interface Guidelines** compliance on every UI change and the look below (12–13 Nov 2026 overhaul; it replaced the Lotus frosted frame and the page colours).

## Overhaul step 2 (23 Nov 2026)
- **Patient file opens on Resumo** (everyone; reception sees only appointments, prescriptions and consents): complaint, goals, regions and red flags (red band), the last two signed notes, next appointments, active prescriptions, each consent's state, latest test scores. Read-only; each card's button opens its tab. Built from the file the server already sent. Another page can open a file on a tab with `patientNav.tab` (`pages/Patients.tsx`).
- **Today › A seguir** replaces "O seu dia": the next or current person (time, name, therapist · kind) with **Abrir ficha** and **Nota de evolução**; the quarter-hour strip stays under it with the current quarter outlined. "Agenda de hoje" uses its spare room for **Amanhã**. Done rows are grey with a green tick; the only tags left are "Por pagar" and "Por fechar". "A tratar" icons are grey unless something is overdue (red). The ↗ buttons became "Ver agenda" / "Ver tudo".
- **Agenda:** one toolbar row under the title (‹ Hoje › · Dia/Semana/Mês · therapists on the trailing side); Nova marcação stays in the header.
- **Sections column** hugs its items (no full-height card). Settings groups it like the overview (Clínica, Sistema) and shows devices waiting as a count.
- **Note scales not yet measured** show an empty grey track and "Por medir".
- **Assistant** opens with one line and the suggestions; the model name is only in Settings.
- **Two-step code boxes** take fast typing, paste and autofill into any box.

## Overhaul step 1 (23 Nov 2026)
Owner: "i dont like the design and overall ui/ux … lets go, i trust you". Plan and reasons: `UX-REVIEW-2026-11-23.md`. This replaces the matching rules below.
- **Title bar:** leading = profile, assistant, Back/Forward, recording pill; centre = page pills, **always with their names** (only a browser window ≤900 px drops them); trailing = **one Menu button (≡)** next to the Windows caption buttons. It lists Ficheiro, Editar, Ver, Ir, (Janela), Ajuda; each opens its items towards the window (← opens, → goes back, ↑↓ move, Esc closes). Same command ids and shortcuts. macOS keeps the native menu bar and shows no ≡.
- **Settings left the title bar:** it is in the profile menu (Definições… Ctrl+,) and in the menus; the avatar gets a teal ring while Settings is open.
- **Type scale (tokens in `app.css`):** `--fs-cap` 12, `--fs-sub` 13, `--fs-body` 14, `--fs-head` 17, `--fs-title2` 22, `--fs-title` 28 (and `--fs-xs` 10). The native Mac app is one step smaller (11/12/13/15/20/26), as the HIG's 13 pt body. Never write a px font size; pick a token.
- **One secondary grey:** `--text3` is now an alias of `--text2`.
- **Corners (tokens):** `--r-sm` 8 (controls, menu rows), `--r-md` 12 (fields, menus, inner boxes), `--r-lg` 20 (cards), `--r-xl` 24 (tiles), 999 capsules. Never write a px radius.
- **Menus are solid** (`--raise`), rows 28 px tall.

## The look (overhaul, 12–13 Nov 2026)

Owner: "overhaul the appearance of the app by using this image and the phone app as examples … the accent color should be teal. avoid ai looking designs" (reference: a light finance dashboard with page pills at the top, white cards on a light grey canvas, capsule buttons, one orange filled button — `references/desktop-ref-dashboard.jpg`). Replaces the Lotus frosted frame, the painted wallpaper and the page colours.

- **One flat canvas** (`--canvas` #ECEBE7 light, #121413 dark) from the title bar down, on every platform: no wallpaper, no grain, no frosted tint, no inset panel. Menus, sheets and toasts are solid.
- **Title bar (60 px)**, three parts (owner, 13 Nov 2026): leading = **round profile, Settings and assistant buttons**, then Back/Forward and the recording pill; **centre = the pages as pills** (icon in a circle + name; current page: white pill, teal circle); trailing = the in-window menus (Ficheiro, Editar, Ver, Ir, Ajuda) **on the same line as the Windows caption buttons** (top-right, 44 px tall). **No logo and no sidebar toggle anywhere** (button, View menu item and ⌃⌘S are gone; the sections column is always shown). **No left rail.** When there isn't room (≤1340 px with menus, ≤1100 px always) only the current pill keeps its name; the others show the icon with the name as tooltip and accessible label. In the browser below 760 px the pills become a floating bar at the bottom, like the phone's.
- **Teal is the only accent** (`--teal` #0B7A70 / #45C6B6; `--teal-text` #08665E for teal text in light mode so it passes 4.5:1). No page colours: `--c-*` all point to teal. Selection that isn't an action (selected day, current hour, active chip, "Agora") is **near-black ink** (`--ink`), as on the phone. Therapists keep their own quiet colours on the schedule (`--t1…--t5`) because they tell people apart. Initials are one grey style (no colour per person).
- **AI** is labelled with words and its icon; no violet (`--ai` is a neutral ink).
- **White cards, 20 px corners** (tiles 24 px), a barely visible shadow, no outlines. **Sections column** (Agenda, Finanças, Definições): a white rounded card down the leading side (248 px), like the bento boxes; the current section has a light teal fill and a teal icon (owner chose this on 13 Nov after rejecting a row of section capsules). Settings keeps its own layout (narrow centred column).
- **The assistant floats in front of everything** (owner, 13 Nov: "not a side panel, but a new panel in front"): a 420 px panel under its button, top-left; ✕, Esc or the button closes it. It no longer takes a column.
- **Capsule buttons** 36 px (white; teal for the one main action), round 36 px icon buttons, 38 px fields with 12 px corners, capsule segmented controls on a darker track, outlined status tags with a dot.
- **Summary figures** sit in one white card side by side with hairlines between them (no icons or trend arrows).
- **Today is a bento that fills the window and never scrolls** (owner, 12 Nov: "too much empty space … I like the bento boxes in resumo finanças … not a scrollable page, but a view more"): "O seu dia" (dark teal tile, a bar per quarter-hour 08–20 h, filled when booked), the figures card, "Agenda de hoje" and "A tratar". Each list shows only the rows that fit (`useFit` in `pages/Today.tsx`) and ends in "Mais N · Ver agenda / Ver tudo"; the ↗ buttons do the same (A tratar opens a sheet with the full list).
- Page titles 28 px bold, subtitle 13.5 px grey; section titles 16 px.

## Apple HIG rules applied (keep them)

**Type and layout**
- System font; 13 pt body, 22 pt page titles, 15 pt section titles, 11–12 pt labels. Tabular numbers for money and times.
- Page headers stick to the top and blur the content under them (scroll-edge effect) instead of a solid bar.

**Toolbar and menus**
- Title bar: no page title, no app or clinic name and no search field; the pages are pills in its centre. Leading: sidebar toggle, Back/Forward (and the in-window menus on Windows). Trailing: assistant (owner). Find (⌘F) opens a floating search field.
- One bar, integrated with the OS (owner, 11 Nov 2026). macOS: overlay title bar, traffic lights over the frosted bar, hidden title. Windows: no OS caption strip (`set_decorations(false)` in `lib.rs`); the frosted bar draws the Windows 11 caption buttons (`WinCaption` in `App.tsx`: 46 px, Minimize / Maximize-Restore / Close, Close red on hover) and keeps drag, double-click to maximise, Snap, shadow and rounded corners. The Windows menus have no app-name menu: Settings and Lock end File, About ends Help.
- Full menu bar in HIG order: App (About, Settings… ⌘,, Lock), File, Edit, View, Go, Window, Help. **Every toolbar action exists in a menu, with a shortcut.** macOS gets the native menu bar; Windows/preview get in-window menus.
- Menus work from the keyboard: ↑↓ items, ←→ switch menus, Home/End, Esc.
- Shortcuts: ⌘N New Appointment, ⇧⌘N New Patient, ⇧⌘P Record Payment, ⌘F Find, ⌘Z Undo, ⌘, Settings, ⌃⌘Q Lock, ⌥⌘A Assistant, ⌘[ Back, ⌘] Forward, ⌘1–5 pages. Windows uses Ctrl (and Ctrl+Alt+A for the assistant, Alt+←/→ for Back/Forward, Ctrl+L to lock).
- Help › Keyboard Shortcuts lists them all.
- Context menus (right-click, and the ⋮ button) on appointments: Open File, Progress Note…, Record Payment…, Call, Reschedule…, Cancel Appointment….

**Controls and copy**
- Buttons that open a window/sheet end in "…". English labels Title Case; Portuguese sentence case. Destructive actions name the action ("Cancel Appointment", "Deactivate").
- One or two accent (filled) buttons per view at most.
- Segmented controls for view switching (Day/Week/Month, patient tabs). Switches only in grouped settings rows; checkboxes elsewhere.
- Disable the primary button until required fields are filled; inline errors that say how to fix it.
- Sortable table headings (click to sort, again to reverse), alternating rows.

**Feedback**
- **Undo** for anything reversible: a frosted note at the top with an Undo button; Edit › Undo names the action ("Undo Decline Request"). Pass the label as the 3rd argument of `toast()`.
- **Alerts** only for irreversible actions; app icon on top; for destructive ones there's no default button and Cancel gets focus.
- **Sheets** (modal to the window) for forms: Cancel + one action, Esc cancels.
- Empty states say what's missing and offer the next step as a button.
- AI output is always labelled, shows specific progress text ("Reading the amount, date and supplier from the receipt…"), and offers Use/Retry/Discard or Apply/Cancel.

**Accessibility and environment**
- Light/dark follows the system by default; Settings › Appearance can force Light or Dark (owner, 9 Nov 2026; `src/lib/theme.ts`, `data-theme` on <html>, and the window frame via the `set_theme` command). Respect Reduce Transparency (solid surfaces), Increase Contrast, Reduce Motion.
- Every icon-only button has an accessible label and a tooltip (page pills show their shortcut).
- The window restores its previous state on launch (sections column, last patient).
- Window minimum 1024×640.

## Patterns in code

- **Dates and times**: never `<input type="date|time">` (they follow the computer's region, e.g. US "11/09/2026" and "05:00 PM"). Use `<DateInput>` (always dd/mm/aaaa, typed or from the calendar button, ↑/↓ change a day) and `<TimeInput>` (24-hour pop-up) from `components/ui.tsx`. Values stay ISO.
- **Sheets don't close on a click outside** — only Cancel/Close or Esc — so typed details are never lost.

- Strings: `const { L } = useApp(); L("Pré-visualização", "Preview")`.
- New sheet: build it with `<Sheet title action onAction>` from `components/ui.tsx`; open with `openSheet(<MySheet />)`.
- Confirmation: `confirm({ title, text, action, destructive, onConfirm })`.
- **Deletes are held, not clicked** (owner, 8 Nov 2026): anything that deletes data uses a 5-second hold button — `confirm({ …, hold: true })`, `<Sheet hold>` or `<HoldButton label onDone />` from `components/ui.tsx`. The red fill grows from the leading edge; letting go early cancels and says “Hold for 5 seconds”. Works with mouse, touch, Space and Enter. Undo-able removals (waitlist, class list, a test just recorded) keep their Undo note instead.
- Feedback: `toast(message, undoFn?, undoLabel?)`.
- Context menu: `openMenu(app, event, items)` from `components/menus.tsx`.
- New menu command: add the id to `menus` in `App.tsx`, to `run()` in `App.tsx`, and to `src-tauri/src/menu.rs`.
- Before calling UI work done: screenshot it in light and dark, at 1440×900 and at a narrow width.

## The iPhone app (phase 5, 12 Nov 2026)

Owner: "be a little more faithful to the example images" — `references/phone-ref-home-schedule.jpg`, `phone-ref-profile.png`, `phone-ref-signup.jpg`. Styles in `src/phone/phone.css`.
- **Monochrome**: warm grey canvas (#ECEAE6), near-white cards, near-black ink for selection and primary buttons (near-white on black in dark mode). Colour only for status (red recording, warnings) and the people's initials. No page colours on the phone.
- **Home (Hoje)**: greeting on the left, round icon buttons on the right (bell = what needs attention, calendar = schedule), a search pill, a "your day" card with a dark pill button and a square grid of the day (08–20 h, one square per booked quarter-hour), horizontal cards for things to approve, then session cards ("Hoje: 10:15").
- **Cards**: dark bar on the left (grey once done), title, status in grey ("A seguir", "Realizada"), people as small overlapping circles, time with a clock, ⋯ in the corner, the admin note in an inner box on the schedule.
- **Schedule**: round "Today" and ⋯ buttons around a centred title, month with arrows, weekday-over-number strip with the selected day in a raised white tile, an hour column with the current hour in a pill.
- **Inner screens** (Patients, File, Record, Account): centred title, round back button, profile card, small grey section headers in sentence case, rows ending in an arrow.
- **Tab bar**: a small floating capsule of icons (the current one in a dark circle) and one round dark button beside it for Record (red while recording). Icons only, as in the reference; each has a VoiceOver label.
- **Sign-in and lock screen**: the sage look of the sign-up reference (off-white page, sage fields with dark-green icons, full-width sage capsule button). No "or" / social sign-in row: the clinic has no such sign-in.
- **Quiet, few words** (owner, 12 Nov: "still looks so AI made"): no helper paragraphs or "read only" notes on the phone, footers only when they change what you do; no coloured initials (one grey style), no icons on list rows, no decorative ⋯ or figures that aren't actions or real data.

## Brand (12 Nov 2026)
The mark is a pulse line whose beat rises into the four fingertips of an open hand, teal `#0B7A70` on paper `#F3F1EC` (sources and rules in `brand/`, sheet https://claude.ai/artifact/MSJqiG7wyZeZ2hR5vvUKjD). Keep clear space of at least the tallest finger's height around it; below 24 px wide use only the icon tile. Never recolour it red or put it on a busy photo. Name: Manrope ExtraBold.

