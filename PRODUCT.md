# Product

<!-- impeccable:product-schema 1 -->

<!-- Written 23 Nov 2026 from the owner's recorded decisions in claude/ (README, SPEC, DECISIONS, DESIGN). Facts below are the owner's; nothing here is invented. -->

## Platform

web

## Users
- **Patients and future patients of one physiotherapy clinic in Portugal** (public website, physiopulse.duckdns.org, later physiopulse.pt): people in pain or recovering from surgery, often on a phone, deciding whether to book a first assessment, a session or a home visit; returning patients signing in to see their sessions, requests and invoices. Portuguese first, English second.
- **The clinic's own staff** (desktop app on Mac and Windows, iPhone app): the owner (a physiotherapist), therapists and a receptionist running the day — schedule, patient files, consents, money. 2–3 computers.

## Product Purpose
PhysioPulse runs one physiotherapy clinic: patients, schedule, consents and money in one place, on the clinic's own server. The public website presents the clinic, its services and prices, takes appointment requests, and (phase 4b) gives invited patients their own area. Success for the website: a visitor understands who treats them, what it costs and how to ask for a time, and sends a request.

## Positioning
A small clinic where each session is one-to-one with the same physiotherapist, unhurried assessment, home visits when you can't come in, and prices shown openly. Everything runs on the clinic's own server: nothing about patients goes to cloud services, the website uses no tracking, no cookies on public pages and nothing from other companies.

## Operating Context
- Website: plain HTML made on the server (Rust, `server/portal`), no JavaScript, strict CSP (`style-src 'self'`, no outside fonts or images), Portuguese at `/…`, English at `/en/…`. Texts, prices, team page and switches come from the app (Settings › Site e portal). The site can be unpublished ("being prepared"). The clinic's space may not be open yet ("Abertura em breve").
- App: Tauri 2 + React; Apple HIG is a standing owner rule; teal is the only accent; light/dark follow the system.

## Capabilities and Constraints
- Website pages: home, services and prices, team (owner chooses who shows; no photos yet), request an appointment (form), privacy policy, terms, patient area (/conta: sign-in, invite, reset, sessions, requests, cancel ≥24 h, invoices).
- No JavaScript, no web fonts from other servers, no third-party scripts or images; anything visual must be inline SVG, CSS or a file served by the site itself.
- Requests are requests: an appointment is only booked once the clinic confirms.
- Undecided: final domain (physiopulse.pt), real photos of the space and team (none exist yet).

## Brand Commitments
- Name: PhysioPulse. The mark: a pulse line whose beat rises into the four fingertips of an open hand, teal `#0B7A70` on paper `#F3F1EC` (`brand/`). Keep clear space; never recolour it red or put it on a busy photo. Wordmark in Manrope ExtraBold.
- Teal is the accent (owner, 12–13 Nov 2026). The owner repeatedly asked to "avoid AI looking designs" and said the current website "looks crazy AI" (23 Nov 2026).
- Sign-in screen: the die-cut pulse-hand line (owner's choice, 12 Nov 2026).

## Evidence on Hand
- Real: services and prices (the app's price list), opening hours, the owner's and therapists' titles/bios as the owner writes them, the clinic's contacts, NIF, ERS number when filled in.
- Absent and not to be invented: patient testimonials, reviews, ratings, number of patients treated, photos of the clinic or team, awards, insurer partnerships.

## Product Principles
1. Plain and honest: say what it costs, who treats you, and that a request is not yet a booking.
2. Health data stays in the clinic; the website shows nothing clinical and tracks nobody.
3. Portuguese first, written for people in pain on a phone.
4. Quiet, few words: no decoration that isn't information.

## Accessibility & Inclusion
WCAG 2.1 AA; text readable by older patients (large body text, high contrast, 44 px targets); works without JavaScript and on old phones; respects reduced motion.
