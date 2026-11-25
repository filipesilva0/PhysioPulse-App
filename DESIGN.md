---
name: PhysioPulse website
description: The clinic's public website and patient area, set as the clinic's printed appointment card.
colors:
  card-stock: "#0B7A70"
  card-stock-deep: "#075A53"
  teal-ink: "#08655D"
  date-stamp-violet: "#4A37A8"
  brand-paper: "#F3F1EC"
  inner-paper: "#FAF9F6"
  field-white: "#FFFFFF"
  on-stock: "#F3F1EC"
  on-stock-muted: "#CDEAE5"
  ink: "#12211E"
  ink-secondary: "#3F4D49"
  ink-tertiary: "#56635F"
  rule: "rgba(11, 122, 112, .42)"
  rule-soft: "rgba(11, 122, 112, .2)"
  correction-red: "#A9302B"
  night-paper: "#181714"
  night-inner-paper: "#201E1A"
  night-field: "#12110F"
  night-stock: "#0D5F58"
  night-ink: "#E9F0EE"
  night-teal-ink: "#6FD3C4"
  night-stamp-violet: "#B3A6FF"
  night-correction-red: "#F19C96"
typography:
  display:
    fontFamily: "Barlow Semi Condensed, Arial Narrow, system-ui, sans-serif"
    fontSize: "clamp(42px, 6.4vw, 84px)"
    fontWeight: 800
    lineHeight: 1.04
    letterSpacing: "-0.01em"
  headline:
    fontFamily: "Barlow Semi Condensed, Arial Narrow, system-ui, sans-serif"
    fontSize: "clamp(28px, 3.2vw, 38px)"
    fontWeight: 800
    lineHeight: 1.04
    letterSpacing: "-0.01em"
  title:
    fontFamily: "Barlow Semi Condensed, Arial Narrow, system-ui, sans-serif"
    fontSize: "24px"
    fontWeight: 800
    lineHeight: 1.04
  figure:
    fontFamily: "Barlow Semi Condensed, Arial Narrow, system-ui, sans-serif"
    fontSize: "50px"
    fontWeight: 800
    lineHeight: 1
    letterSpacing: "0"
    fontFeature: "\"tnum\", \"lnum\""
  body:
    fontFamily: "-apple-system, BlinkMacSystemFont, SF Pro Text, Segoe UI Variable Text, Segoe UI, system-ui, sans-serif"
    fontSize: "18px"
    fontWeight: 400
    lineHeight: 1.6
  label:
    fontFamily: "Barlow Semi Condensed, Arial Narrow, system-ui, sans-serif"
    fontSize: "15px"
    fontWeight: 600
    lineHeight: 1.3
    letterSpacing: "0.08em"
  button:
    fontFamily: "Barlow Semi Condensed, Arial Narrow, system-ui, sans-serif"
    fontSize: "20px"
    fontWeight: 600
    lineHeight: 1
    letterSpacing: "0.02em"
rounded:
  stamp: "4px"
  cut: "6px"
spacing:
  gutter: "20px"
  gutter-phone: "14px"
  row: "16px"
  field: "22px"
  sheet: "64px"
  container: "1180px"
components:
  button-primary:
    backgroundColor: "{colors.card-stock}"
    textColor: "{colors.on-stock}"
    typography: "{typography.button}"
    rounded: "{rounded.cut}"
    padding: "10px 22px"
    height: "50px"
  button-primary-hover:
    backgroundColor: "{colors.card-stock-deep}"
    textColor: "{colors.on-stock}"
  button-primary-large:
    backgroundColor: "{colors.card-stock}"
    textColor: "{colors.on-stock}"
    rounded: "{rounded.cut}"
    padding: "12px 30px"
    height: "58px"
  button-outline:
    backgroundColor: "transparent"
    textColor: "{colors.teal-ink}"
    typography: "{typography.button}"
    rounded: "{rounded.cut}"
    padding: "10px 22px"
    height: "50px"
  button-ghost:
    backgroundColor: "transparent"
    textColor: "{colors.ink-secondary}"
    rounded: "{rounded.cut}"
    padding: "10px 22px"
    height: "50px"
  button-danger:
    backgroundColor: "{colors.correction-red}"
    textColor: "{colors.field-white}"
    rounded: "{rounded.cut}"
    padding: "10px 22px"
    height: "50px"
  input:
    backgroundColor: "{colors.field-white}"
    textColor: "{colors.ink}"
    typography: "{typography.body}"
    rounded: "{rounded.cut}"
    padding: "12px 14px"
    height: "54px"
  input-date:
    backgroundColor: "{colors.field-white}"
    textColor: "{colors.date-stamp-violet}"
    rounded: "{rounded.cut}"
    padding: "12px 14px"
    height: "54px"
  choice-box:
    backgroundColor: "{colors.field-white}"
    textColor: "{colors.ink}"
    rounded: "{rounded.cut}"
    padding: "8px 14px"
    height: "48px"
  choice-box-checked:
    backgroundColor: "{colors.field-white}"
    textColor: "{colors.teal-ink}"
  printed-panel:
    backgroundColor: "{colors.inner-paper}"
    textColor: "{colors.ink}"
    rounded: "{rounded.cut}"
    padding: "clamp(22px, 3.4vw, 40px)"
  stamp:
    backgroundColor: "transparent"
    textColor: "{colors.date-stamp-violet}"
    rounded: "{rounded.stamp}"
    padding: "7px 10px 6px"
  nav-link:
    textColor: "{colors.ink-secondary}"
    padding: "10px 12px"
    height: "48px"
  nav-link-current:
    textColor: "{colors.teal-ink}"
---

# Design System: PhysioPulse website

This file covers the public website and patient area served by `server/portal` only. The desktop app keeps its own design record in `claude/DESIGN.md`; nothing here applies to it.

## Overview

**Creative North Star: "The Appointment Card"**

The site is set like the small printed card a physiotherapy patient keeps in the wallet. Teal card stock is the cover (the home page's first panel, the footer, the single-card "being prepared" page); the brand's paper is the printed inside, where everything a patient fills in or reads sits on ruled lines. One perforation with punched notches tears the cover from the request stub. A second ink, date-stamp violet, is pressed onto dates and onto one rubber stamp per page. Nothing is lit, blurred or lifted: every surface is a flat ink on stock or paper.

Density is that of a printed form, not a brochure: rules every row, small-caps field labels, dotted leaders running from a service to its price, and figures (prices, opening hours, dates, amounts due) set as the loudest type on the page, louder than most headings. Reading text is the system sans at 18px for older patients on phones; the condensed display face carries names, headings, labels, buttons and every number.

Dark mode keeps the card metaphor on a warm near-black paper (night paper) with a deeper stock, a pale teal ink and a light violet stamp. The world rejects the wellness template: no stock smiles, no blurred blobs, no frosted header, no soft rounded cards.

**Key Characteristics:**
- Two materials only: teal card stock and brand paper (warm near-black at night).
- Flat inks, no gradients, blur, glow or soft shadows.
- Printed form grammar: ruled rows, dotted leaders, small-caps labels, dashed tear lines.
- Figures in Barlow Semi Condensed 800 with tabular lining numerals, larger than the text around them.
- Date-stamp violet for dates, the date field, focus and the one stamp per page.
- 6px die-cut corners on every box; 4px on rubber stamps.
- No JavaScript, no outside fonts or images; everything is CSS, inline SVG or a self-hosted file.

## Colors

Two printed materials and two inks: teal stock and warm paper, near-black ink, and a single violet for dates and stamps.

### Primary
- **Card Stock Teal** (card-stock): the brand teal as a whole field. The home cover, the footer, the "being prepared" card, the primary button fill, the masthead rule under the header, the perforation and the 3px rule that opens each sheet. Never used as small text on paper; that job belongs to Teal Ink.
- **Deep Stock** (card-stock-deep): the primary button's hover fill only.
- **Teal Ink** (teal-ink): teal printed on paper, at 7:1. Links, field labels and legends, the current nav item, numbered step counters, outline button text, and the checked state of a price line or choice box.

### Secondary
- **Date-Stamp Violet** (date-stamp-violet): the second ink. Dates in the patient area, the value inside a date field, the rotated rubber stamp ("Abertura em breve", "Em breve"), status tags ("Pedido de mudança enviado"), the focus outline and text selection. On the teal cover the stamp is printed in pale violet so it reads on stock.

### Tertiary
- **Correction Red** (correction-red): errors only. The error summary box (on a 7% tint), invalid field borders, inline error text and the destructive button.

### Neutral
- **Brand Paper** (brand-paper): the page background, the masthead and the punched notches of the perforation; also the text colour on stock (on-stock).
- **Inner Paper** (inner-paper): the printed inside of the card: the request stub, the perforation strip and printed panels.
- **Field White** (field-white): fill of inputs, choice boxes and chips, so the fields read as blanks to fill in.
- **Stock Mist** (on-stock-muted): secondary text on stock (cover intro, hours labels, footer headings and fine print).
- **Ink** (ink): body text and figures. **Ink Secondary** (ink-secondary) for descriptions, legal prose and nav; **Ink Tertiary** (ink-tertiary) for hints, placeholders and service subtitles.
- **Rule** (rule) and **Soft Rule** (rule-soft): teal printed at 42% and 20%. Rule draws ruled rows, dotted leaders, panel and input borders; Soft Rule draws unselected boxes, chips, ghost buttons and the language switch.
- **Night set**: night-paper, night-inner-paper, night-field, night-stock, night-ink, night-teal-ink, night-stamp-violet and night-correction-red replace their day counterparts under `prefers-color-scheme: dark`; the rules become pale teal (rgba(111, 211, 196, .42) and .18).

### Named Rules
**The Two Inks Rule.** Teal and near-black do all the work; violet appears only on dates, the date field, focus, selection, and stamps. If violet is on something that is not a date or a stamp, it is wrong.

**The One Stamp Rule.** At most one rotated rubber stamp per page. Status tags in a list are small upright stamps and do not count against it.

**The Flat Ink Rule.** No gradients, blur, glow, translucency over content or soft shadows anywhere. Tints exist only as flat fills (the 8% to 14% teal behind a checked option or an outline button's hover, the 7% red behind errors).

## Typography

**Display Font:** Barlow Semi Condensed 600 and 800, self-hosted (with Arial Narrow, system-ui)
**Body Font:** the system sans: -apple-system, SF Pro Text, Segoe UI Variable Text, Segoe UI, system-ui

**Character:** A condensed, sturdy grotesque that looks set by a printer for a card, paired with the reader's own system face for comfortable reading. The display face carries anything a patient scans; the system face carries anything they read.

### Hierarchy
- **Display** (800, clamp(42px, 6.4vw, 84px), 1.04): page titles. The cover title is held to clamp(40px, 4vw, 48px) so the figures beside it stay louder.
- **Headline** (800, clamp(28px, 3.2vw, 38px), 1.04): sheet headings; panel headings at 30px; team names at clamp(34px, 3.6vw, 46px).
- **Title** (800, 24px to 26px, 1.04): service names in the visit list, legal subheads, list titles in the patient area.
- **Figure** (800, 50px, 1, tabular lining numerals, no tracking): prices on the request stub (48px), the price list (34px), hours on the cover (34px) and in the hours table (24px), the amount due (40px), the phone number (28px), the date field (22px) and dates in lists (18px). Figures never wrap.
- **Body** (400, 18px, 1.6): reading text; leads at 19px to 21px; secondary at 16px. Prose is held to 60ch to 70ch.
- **Label** (600, 15px to 16px, 0.08em, uppercase): field labels, legends, contact keys and the "por pagar" key. Footer headings use 0.12em.
- **Nav and buttons** (600, 18px to 22px, uppercase with 0.06em for nav; sentence case with 0.02em for buttons).

### Named Rules
**The Loud Figures Rule.** A price, time, date or amount is always set in the display face at 800 with tabular lining numerals, and always larger than the label or name it belongs to. On the request stub the price (48px) is bigger than the stub's heading (30px).

**The Field Label Rule.** Uppercase small labels name a field or a value directly beneath them (a form field, a phone number, a job title under a name, the amount due). They are never a decorative line above a heading.

## Layout

One centred column, 1180px wide with a 20px gutter (14px in the header on phones). Pages are a stack of printed sheets: each sheet opens with a 3px teal rule, 64px below the last, with its heading under it. Rows inside a sheet are separated by 1px rules with 12px to 16px vertical padding, never by gaps between boxes. Two-sheet rows use a 56px gutter. Forms and legal text sit at 820px; the patient area narrows to 600px. Main content ends with 72px of space before the footer.

The home card is a three-column grid: cover (1.2fr), a 34px perforation strip, and the stub (1fr). At 960px and below the card turns: the cover stacks over a 30px horizontal perforation and the stub, the notches drop, and two-sheet rows stack with a 40px gap. At 760px and below the nav wraps to its own scrollable row, the header button shortens its label, the cover keeps only name, promise and stamp, the stub keeps only what to ask for (day and part of day move to the next page), and prices on the stub drop to 26px so the whole card fits one phone screen.

Every target is at least 44px; most are 48px to 58px.

## Elevation & Depth

None. The system is flat: no box shadows anywhere. Depth is printed, not lit: a teal field against paper, a 2px or 3px rule, a dashed tear line, a punched half-circle notch in the paper colour. A pressed state is a colour change (fill darkens, border turns from soft rule to stock), never a lift.

### Named Rules
**The Printed Depth Rule.** If a surface needs separating, give it a rule or put it on stock. Never a shadow.

## Shapes

Every box is die-cut with the same small corner (6px): buttons, inputs, choice boxes, chips, panels, the error box, the language switch and the card itself. Rubber stamps and tags use a tighter 4px with a 2px or 3px violet border; the cover stamp is rotated -5 degrees (-3 degrees on the "coming soon" panel, -4 degrees on phones). The only circles are the perforation's punched notches (26px, half-clipped) and radio buttons. Lines carry the form language: solid 1px rules between rows, 2px dotted leaders, 3px dashed tear lines, 2px dashed borders on notes.

## Components

### Buttons
Die-cut, flat ink, set in the display face.
- **Shape:** 6px corners, 2px stock border, 50px tall (58px large, 44px small).
- **Primary:** stock fill, paper text, 10px 22px. Hover darkens to Deep Stock.
- **Outline (default):** transparent with stock border and Teal Ink text; hover lays a flat 14% teal tint and turns the text to Ink. Transition is colour only, 180ms cubic-bezier(.2,.7,.2,1).
- **Ghost:** soft-rule border, Ink Secondary text, for secondary account actions.
- **Danger:** Correction Red fill and border, white text.
- **On stock:** on the "being prepared" card, outline buttons switch to paper borders and text.

### Price line (signature)
The request stub's service choice. A radio, the service name (19px, bold) with a 15px subtitle, a 2px dotted leader in Rule teal, and the price as a figure. Rows are ruled top and bottom, at least 56px tall. Checking a line turns its name and price to Teal Ink. The price list page uses the same grammar without the radio, with 34px figures.

### Perforation (signature)
A 34px strip of inner paper between cover and stub, edged top and bottom by 2px stock, with a 3px dashed vertical tear line and a punched half-notch at each end. When the card turns on narrow screens it becomes a horizontal dashed line without notches.

### Stamp and tag
- **Stamp:** display 800, 17px, uppercase, 0.1em, violet text and 3px violet border, 4px corners, rotated. One per page.
- **Tag:** the same ink at 14px with a 2px border, upright, for a request's status in a list.

### Inputs / Fields
- **Style:** white field, 2px Rule border, 6px corners, 12px 14px padding, 54px tall, body text.
- **Date field:** the value is printed in the date-stamp ink, display 800 at 22px with tabular numerals.
- **Focus:** 3px violet outline offset 1px, border turns to stock.
- **Error:** border turns Correction Red; a 16px red message sits under the field; the form opens with a red-bordered summary on a 7% red tint linking to each field.
- **Labels:** uppercase display labels in Teal Ink above every field.

### Choice boxes and option cards
Radios and checkboxes for part of day, service or contact preference: white fill, 2px soft-rule border, 6px corners, 48px to 50px tall. Checked turns the border to stock and the text to Teal Ink; larger option cards also take an 8% teal fill. Native controls use stock as their accent colour, 22px square.

### Printed panel
Inner paper, 2px Rule border, 6px corners, padding clamp(22px, 3.4vw, 40px). Holds forms, legal texts and every block of the patient area. Inside, lists are ruled rows with the date in violet figures and actions to the right.

### Navigation
The masthead sits on paper with a 3px stock rule beneath it. Brand mark and name (display 800, 24px) on the left; nav links in uppercase display 600 at 18px, Ink Secondary, 48px tall; hover underlines; the current page is Teal Ink with a 3px underline offset 8px. To the right, a 48px language switch (soft-rule box) and the primary request button. On phones the nav takes its own scrollable row.

### Footer
The back of the card: a full stock field, three columns (name and contacts, information links, complaints links) over fine print in Stock Mist. Headings are uppercase 15px with 0.12em tracking.

## Do's and Don'ts

### Do:
- **Do** put every price, hour, date and amount in Barlow Semi Condensed 800 with tabular lining numerals, larger than its label.
- **Do** separate rows with 1px Rule lines and open each sheet with a 3px stock rule.
- **Do** run a dotted leader from a service name to its price.
- **Do** use 6px corners on every box and 4px on stamps and tags.
- **Do** print dates and the date field's value in date-stamp violet.
- **Do** keep body text at 18px in the system sans, and targets at 44px or more.
- **Do** redefine every ink for dark mode on warm night paper rather than inverting.

### Don't:
- **Don't** use gradients, blur, glow, frosted surfaces or box shadows.
- **Don't** use violet for anything other than dates, the date field, focus, selection and stamps, and don't print more than one rotated stamp per page.
- **Don't** set small teal text in Card Stock Teal on paper; use Teal Ink.
- **Don't** put uppercase labels above headings as decoration; a label names the field or value under it.
- **Don't** add photos, stock imagery, testimonials or decorative illustration; the only image is the brand mark.
- **Don't** load fonts, scripts or images from other servers.
