---
version: 1
slug: "server-portal-src-html-rs"
primary_target: "server/portal/src/html.rs"
related_targets: ["server/portal/static/site.css","server/portal/src/account.rs"]
---

## Scope
Public website (server/portal): home, services and prices, team, request form, legal pages, patient area. Mode: Persuade (home), Read (legal), Operate (patient area). Audience: people in pain in Portugal, often on a phone, often older; Portuguese first. Action: ask for an appointment (service, day, part of day). Proof: real prices, real hours, the real people who treat you. No photos, testimonials or claims exist; none invented.

## Direction contract
THESIS: The site is the clinic's appointment card — the small printed card a physiotherapy patient keeps in the wallet, with the clinic's name on the cover, printed fields inside and dates filled in. It refuses the wellness template: stock smiles, blurred blobs, frosted header, soft rounded cards.
OWN-WORLD: Teal card stock (brand #0B7A70) as whole fields; paper (#F3F1EC, brand) as the printed inner panels; one second ink, date-stamp violet, for dates and the one stamp per page. Flat inks only: no gradients, blur or glow. Printed form grammar: ruled lines, small-caps field labels, dotted leaders, perforated tear lines, 4px die-cut corners. Barlow Semi Condensed 800 for display and figures, system sans for reading.
STORY: Visitor sees whose clinic this is and what a session costs, picks what they need and when, sends the request, and knows the clinic confirms by phone.
FIRST VIEWPORT: The card fills the viewport: teal cover left (name, one-line promise, hours) and the paper inside right with a working request stub — service, day, morning/afternoon, "Continuar" — separated by a perforation. Prices for assessment/session/home printed as the card's first ruled lines beneath.
FORM: own grounded list #6 (the appointment card); raises: flat separately-printed inks only (from the woodblock print, declined); figures are the loudest type — prices, times and dates set largest (from the nixie counter, declined). seed:262984ecf2be8991
FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance
