# Portuguese law — what the app relies on

Researched by Claude on 7 Nov 2026 at the owner's request. A starting point, not legal advice: confirm the VAT point with the accountant once, and have the consent texts read by a lawyer if the clinic grows.

## VAT (IVA)
- Physiotherapy is **exempt** under art. 9.º, n.º 1 of the CIVA (paramedical professions), when: (a) it's done by a qualified physiotherapist (DL 261/93 annex item 7, DL 320/99, registered with the Ordem dos Fisioterapeutas), and (b) it has a therapeutic purpose (diagnosing, treating or restoring health). The Tax Authority treats the exemption as **objective**: it applies whatever the provider's legal form, so a company qualifies. ([PIV 30921](https://info.portaldasfinancas.gov.pt/pt/informacao_fiscal/informacoes_vinculativas/CIVA/PIV_30921.pdf), [OCC](https://www.occ.pt/index.php/pt-pt/noticias/iva-isencoes-9))
- **Classes** (e.g. clinical Pilates) are exempt only when they're therapeutic and given by a physiotherapist. Fitness-style classes are taxed at 23 %. → the price list should mark which items are exempt.
- An exempt invoice must state the reason; the usual wording is **"Isento — artigo 9.º, n.º 1 do CIVA"**. The certified invoicing software adds it once the item is set as exempt.
- Exempt under art. 9.º means no VAT can be deducted on purchases.

## Keeping records
| What | How long | Basis |
| --- | --- | --- |
| Invoices, receipts, accounting | **10 years** | art. 52.º CIVA; art. 123.º CIRC ([OCC](https://www.occ.pt/index.php/pt-pt/noticias/iva-arquivo-de-faturas-e-outros-documentos)) |
| Clinical record | **20 years** after the last contact (recommended) | No specific law for private clinics; 20 years matches the general limitation period for liability (art. 309.º Código Civil). Advice of the Ordem dos Médicos ([FAQ](https://www.omsul.pt/Portals/0/docs/FAQ-ACESSO-A-INFORMACAO.pdf)); RGPD art. 5(1)(e) and Lei 58/2019 art. 21 require a defined period. |
| Signed consents | Same as the clinical record | They're part of it (DGS Norma 015/2013). |
| Complaints | At least 3 years (ERS can ask) | Prudent default. |

→ The app's "Delete patient" tool should warn when the last contact is less than 20 years ago (to do).

## Data protection (RGPD)
- Legal basis for care: art. 9(2)(h) RGPD + Lei 58/2019 art. 29 (health professionals bound by secrecy). **Consent is not the legal basis for the clinical record**; the "data" consent is the information notice + the optional uses (contact by email/SMS, sharing with the referring doctor or insurer).
- **DPIA:** the CNPD list (Regulamento 1/2018) requires one for "transmission of health data by electronic devices" (item 1) and for health data combined with new technologies (item 6.1). A small clinic with its own server is a borderline case; **doing a short DPIA is the safe choice** (a 2–3 page document; Claude can draft it). ([list](https://abreuadvogados.com/wp-content/uploads/2021/03/lista_de_tratamentos_de_dp_sujeitos_a_avaliacao.pdf))
- Encryption at rest isn't mandatory by name (RGPD art. 32 says "appropriate"), but it's the first thing CNPD looks at after a loss. Owner decided against it for now (DECISIONS.md, 7 Nov).
- Patients have the right to a copy of their record (art. 15) → "Export Clinical Record…" covers it.

## Informed consent (DGS Norma 015/2013)
- Written consent is required for invasive acts; for physiotherapy that means **dry needling, acupuncture-like techniques, invasive electrolysis**. Plain physiotherapy only needs informed consent (oral is valid, written is better proof).
- The form must: identify the clinic and the professional, describe the act and its purpose, the benefits, frequent and serious risks, alternatives, the risks of not treating, say the person had time to think and can withdraw at any time without losing care. **Two copies**: one in the record, one for the patient (→ the PDF can be printed or sent).
- **Minors:** 16 or older with enough discernment consent themselves; under 16 a parent/guardian consents and the minor is heard.
- Withdrawal needs no form and must be recorded in the file (→ the app saves a withdrawal PDF).
- Consent is renewed when the situation changes (no fixed expiry). The app uses 12 months for treatment and invasive techniques as a prompt to re-check.

## Photos/videos and marketing
- Photos/videos for the record are part of care; for **teaching, social media or website** separate, specific, freely given consent is needed (RGPD art. 7, 9(2)(a)), refusable without effect on care, withdrawable.
- Marketing by SMS/email needs prior consent (Lei 41/2004, art. 13.º-A). Appointment reminders are not marketing.

## Complaints (ERS)
- Every provider registered with the ERS must have the **Livro de Reclamações** (paper and electronic).
- Paper complaints: enter them in the ERS system (SGREC) **within 10 working days** of the complaint, with the reply sent to the patient and a summary (Regulamento ERS 65/2015, art. 9.º). ([ERS alert 01/2025](https://ers.pt/media/qtiehaor/ers-alerta-de-supervisao-01-2025.pdf), [Reg. 65/2015](https://www.ers.pt/media/f0nlpybn/reg65_15.pdf))
- The reply must say whether the clinic accepts the facts and, if not, why.
- → The complaints log counts 10 working days (Portuguese public holidays excluded).
