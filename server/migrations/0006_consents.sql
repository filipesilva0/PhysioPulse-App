-- Phase 2: consent texts need the owner's approval, and signed consents are saved as PDF in the archive
-- (owner decision 7 Nov 2026). Claude wrote the six first drafts from Portuguese law (claude/LEGAL.md);
-- they arrive as drafts and can't be used for signatures until the owner approves them.

ALTER TABLE consent_texts ADD COLUMN approved_at TIMESTAMPTZ;
ALTER TABLE consent_texts ADD COLUMN approved_by UUID REFERENCES staff(id);
-- Texts the owner wrote before this change were written by the owner: approved.
UPDATE consent_texts SET approved_at = created_at, approved_by = created_by;

-- Signed consents (and withdrawals) as PDF: one document per consent and event.
ALTER TABLE documents ADD COLUMN consent_id UUID REFERENCES consents(id) ON DELETE CASCADE;
ALTER TABLE documents ADD COLUMN consent_event TEXT CHECK (consent_event IN ('signed', 'revoked'));
ALTER TABLE documents DROP CONSTRAINT documents_kind;
ALTER TABLE documents ADD CONSTRAINT documents_kind
    CHECK (kind IN ('photo', 'report', 'imaging', 'prescription', 'letter', 'note', 'consent', 'other'));
CREATE UNIQUE INDEX documents_one_per_consent_event ON documents(consent_id, consent_event) WHERE consent_id IS NOT NULL;

-- Like signed-note PDFs, consent PDFs are part of the record: only the legal tools may remove them.
CREATE OR REPLACE FUNCTION documents_note_locked() RETURNS trigger AS $$
BEGIN
    IF OLD.kind IN ('note', 'consent') AND current_setting('clinica.legal_tool', true) IS DISTINCT FROM 'on' THEN
        RAISE EXCEPTION 'the PDF of a signed note or consent can''t be removed';
    END IF;
    RETURN COALESCE(NEW, OLD);
END; $$ LANGUAGE plpgsql;

-- ───────── first drafts (owner reviews and approves) ─────────
-- Paragraphs are separated by blank lines; "• " starts a list item. "a clínica" is filled in on the PDF
-- by the clinic's name in the header, so the texts stay generic.

INSERT INTO consent_texts (id, kind, version, lang, title, body, valid_months)
SELECT gen_random_uuid(), d.kind, COALESCE((SELECT max(version) FROM consent_texts t WHERE t.kind = d.kind AND t.lang = d.lang), 0) + 1,
       d.lang, d.title, d.body, d.valid_months
FROM (VALUES
('treatment', 'pt', 'Consentimento informado para tratamento de fisioterapia', 12, $t$
Fui informado(a) pelo(a) fisioterapeuta, de forma clara e com tempo para fazer perguntas, sobre:

• a avaliação feita e o diagnóstico em fisioterapia da minha situação;
• os objetivos do tratamento e o plano proposto (técnicas, número e frequência previstos das sessões), que pode ser ajustado ao longo do tratamento;
• os benefícios esperados;
• os riscos e incómodos mais frequentes — dor ou rigidez transitória após a sessão, cansaço muscular, vermelhidão da pele — e os raros mas graves, que me foram explicados;
• as alternativas existentes e o que pode acontecer se não fizer tratamento.

Comprometo-me a informar o(a) fisioterapeuta sobre doenças, medicação, gravidez, cirurgias, dispositivos implantados (por exemplo pacemaker) e qualquer alteração do meu estado de saúde.

Sei que posso fazer perguntas a qualquer momento, recusar uma técnica em concreto e retirar este consentimento quando quiser, sem ter de justificar e sem perder o direito a ser atendido(a). Técnicas invasivas (por exemplo punção seca) têm um consentimento próprio.

Declaro que compreendi a informação e aceito o tratamento proposto. Este documento é feito em duplicado: um exemplar fica no meu processo e posso pedir o outro.
$t$),
('treatment', 'en', 'Informed consent for physiotherapy treatment', 12, $t$
The physiotherapist has explained to me clearly, and with time to ask questions:

• the assessment carried out and the physiotherapy diagnosis of my condition;
• the goals of treatment and the proposed plan (techniques, expected number and frequency of sessions), which may be adjusted as treatment goes on;
• the expected benefits;
• the most common risks and discomforts — temporary pain or stiffness after a session, muscle tiredness, skin redness — and the rare but serious ones, which were explained to me;
• the alternatives, and what may happen if I am not treated.

I will tell the physiotherapist about illnesses, medication, pregnancy, surgery, implanted devices (such as a pacemaker) and any change in my health.

I know I can ask questions at any time, refuse any particular technique and withdraw this consent whenever I want, without giving a reason and without losing my right to care. Invasive techniques (such as dry needling) have their own consent.

I confirm that I understood this information and I accept the proposed treatment. This document is made in two copies: one stays in my record and I may ask for the other.
$t$),
('data', 'pt', 'Informação sobre o tratamento de dados pessoais e de saúde (RGPD)', NULL, $t$
A clínica é a responsável pelo tratamento dos seus dados pessoais e de saúde.

Para que servem: prestar os cuidados de fisioterapia, manter o seu processo clínico, marcar e lembrar sessões, faturar e cumprir obrigações legais. A base legal é a prestação de cuidados de saúde por profissionais sujeitos a sigilo (artigo 9.º, n.º 2, alínea h) do RGPD e artigo 29.º da Lei n.º 58/2019) e o cumprimento de obrigações legais e fiscais. Para estes fins não é preciso o seu consentimento.

Onde ficam: num servidor da própria clínica, em Portugal. Não são enviados para serviços na nuvem nem usados para publicidade. Só os profissionais que precisam têm acesso, e cada consulta do processo fica registada.

Com quem são partilhados: apenas quando a lei obriga (por exemplo, a Autoridade Tributária) ou quando pedir (por exemplo, relatório para o seu médico, seguradora ou subsistema).

Por quanto tempo: o processo clínico é guardado 20 anos após o último contacto; os documentos fiscais 10 anos.

Os seus direitos: pode pedir acesso e cópia do seu processo, retificação, limitação ou apagamento (quando a lei permitir) e apresentar reclamação à Comissão Nacional de Proteção de Dados (www.cnpd.pt). Pode exercê-los na receção ou por escrito.

Contactos: autorizo que a clínica me contacte por SMS, chamada, email ou WhatsApp para marcações, lembretes e assuntos do meu tratamento. Se recusar, os contactos serão apenas presenciais ou por chamada quando indispensável.
$t$),
('data', 'en', 'Information about the use of personal and health data (GDPR)', NULL, $t$
The clinic is the controller of your personal and health data.

What it's used for: providing physiotherapy care, keeping your clinical record, booking and reminding you of sessions, invoicing and meeting legal duties. The legal basis is the provision of health care by professionals bound by secrecy (article 9(2)(h) GDPR and article 29 of Portuguese Law 58/2019) and legal and tax obligations. Your consent is not needed for these purposes.

Where it's kept: on the clinic's own server, in Portugal. It is not sent to cloud services or used for advertising. Only the professionals who need it have access, and every time your record is opened it is logged.

Who it's shared with: only when the law requires it (for example the Tax Authority) or when you ask (for example a report for your doctor, insurer or health scheme).

How long: the clinical record is kept for 20 years after your last contact; tax documents for 10 years.

Your rights: you can ask for access to and a copy of your record, correction, restriction or erasure (where the law allows), and complain to the Portuguese data protection authority (www.cnpd.pt). You can do so at reception or in writing.

Contact: I allow the clinic to contact me by SMS, phone call, email or WhatsApp about appointments, reminders and my treatment. If I refuse, contact will be in person, or by phone only when essential.
$t$),
('media', 'pt', 'Autorização para fotografias e vídeos', NULL, $t$
Durante o tratamento podem ser úteis fotografias ou vídeos (por exemplo da postura, da marcha ou de um exercício) para avaliar a evolução.

Fotografias e vídeos para o processo clínico: ficam apenas no meu processo, no servidor da clínica, e só são vistos pelos profissionais que me acompanham.

Ao assinar, autorizo esta recolha para fins clínicos. A clínica não usa as minhas imagens para ensino, redes sociais, site ou publicidade sem um pedido e autorização à parte, para cada uso.

Posso recusar sem qualquer efeito no meu tratamento e retirar esta autorização a qualquer momento; as imagens já recolhidas são então apagadas, exceto se forem necessárias ao registo clínico.
$t$),
('media', 'en', 'Permission for photos and videos', NULL, $t$
During treatment, photos or videos (for example of posture, walking or an exercise) can help assess progress.

Photos and videos for the clinical record: they stay only in my record, on the clinic's server, and are seen only by the professionals treating me.

By signing, I allow them to be taken for clinical purposes. The clinic does not use my images for teaching, social media, its website or advertising without a separate request and permission for each use.

I can refuse without any effect on my treatment and withdraw this permission at any time; images already taken are then deleted, unless they are needed for the clinical record.
$t$),
('marketing', 'pt', 'Autorização para comunicações da clínica', NULL, $t$
Autorizo a clínica a enviar-me, por SMS, email ou WhatsApp, informação sobre novos serviços, aulas, horários, campanhas e conselhos de saúde em geral.

Estas mensagens nunca usam informação sobre o meu diagnóstico ou tratamento e não são partilhadas com terceiros.

Esta autorização é opcional: recusar não tem qualquer efeito no meu tratamento. Posso retirá-la a qualquer momento, na receção ou respondendo à própria mensagem. Lembretes de sessões não são comunicações deste tipo.
$t$),
('marketing', 'en', 'Permission for news from the clinic', NULL, $t$
I allow the clinic to send me, by SMS, email or WhatsApp, information about new services, classes, opening hours, offers and general health advice.

These messages never use information about my diagnosis or treatment and are not shared with anyone else.

This permission is optional: refusing has no effect on my treatment. I can withdraw it at any time, at reception or by replying to the message itself. Session reminders are not messages of this kind.
$t$),
('guardian', 'pt', 'Consentimento do representante legal (utente menor)', NULL, $t$
Declaro que sou pai, mãe ou representante legal do(a) menor identificado(a) neste documento e que tenho poder para consentir no seu tratamento.

Fui informado(a), de forma clara e com tempo para fazer perguntas, sobre a avaliação, os objetivos e o plano de tratamento de fisioterapia, os benefícios esperados, os riscos e incómodos mais frequentes e os raros mas graves, as alternativas e o que pode acontecer se não fizer tratamento.

O(a) menor foi ouvido(a) e a sua opinião foi tida em conta, de acordo com a idade e maturidade. A partir dos 16 anos, com o discernimento necessário, é o(a) próprio(a) utente quem consente.

Comprometo-me a informar sobre doenças, medicação e qualquer alteração do estado de saúde do(a) menor. Sei que posso retirar este consentimento a qualquer momento, sem justificar e sem prejuízo dos cuidados.

Aceito o tratamento proposto. Técnicas invasivas, fotografias e vídeos têm consentimentos próprios. Este documento é feito em duplicado.
$t$),
('guardian', 'en', 'Consent of the legal representative (patient under 18)', NULL, $t$
I declare that I am the parent or legal representative of the minor named in this document and that I have the authority to consent to their treatment.

It was explained to me clearly, and with time to ask questions: the assessment, the goals and the plan of physiotherapy treatment, the expected benefits, the most common risks and discomforts and the rare but serious ones, the alternatives, and what may happen without treatment.

The minor was heard and their opinion was taken into account according to their age and maturity. From 16, with enough understanding, the patient consents themselves.

I will report illnesses, medication and any change in the minor's health. I know I can withdraw this consent at any time, without giving a reason and without any effect on their care.

I accept the proposed treatment. Invasive techniques, photos and videos have their own consents. This document is made in two copies.
$t$),
('invasive', 'pt', 'Consentimento informado para técnicas invasivas (punção seca)', 12, $t$
O(a) fisioterapeuta propôs-me técnicas invasivas, em particular punção seca: a introdução de uma agulha fina e estéril, sem medicamento, em pontos do músculo, para reduzir a dor e melhorar a função.

Fui informado(a), de forma clara e com tempo para fazer perguntas, sobre:

• o objetivo e como a técnica é feita, com agulhas de uso único;
• os benefícios esperados;
• os efeitos frequentes: dor durante a punção, dor muscular nas 24–48 horas seguintes, pequenas nódoas negras ou sangramento no local, cansaço ou tonturas ligeiras;
• os riscos raros mas graves: desmaio, infeção, lesão de um nervo ou vaso e, quando se trabalha junto ao tórax, pneumotórax (entrada de ar entre o pulmão e a parede do tórax), que exige observação médica urgente;
• as alternativas (outras técnicas de fisioterapia sem agulha) e o que pode acontecer se não fizer esta técnica.

Declaro que informei o(a) fisioterapeuta se tenho medo de agulhas, alergia a metais, problemas de coagulação ou toma de anticoagulantes, diabetes, epilepsia, gravidez, infeção ou doença que afete as defesas, ou dispositivos implantados.

Sei que posso recusar ou pedir para parar a qualquer momento, sem justificar e sem prejuízo do restante tratamento. Aceito a técnica proposta. Este documento é feito em duplicado.
$t$),
('invasive', 'en', 'Informed consent for invasive techniques (dry needling)', 12, $t$
The physiotherapist has proposed invasive techniques, in particular dry needling: inserting a thin sterile needle, with no medicine, into points in a muscle to reduce pain and improve function.

It was explained to me clearly, and with time to ask questions:

• the purpose and how the technique is done, with single-use needles;
• the expected benefits;
• common effects: pain during needling, muscle soreness for 24–48 hours, small bruises or bleeding at the site, tiredness or mild dizziness;
• rare but serious risks: fainting, infection, injury to a nerve or blood vessel and, when working near the chest, pneumothorax (air between the lung and the chest wall), which needs urgent medical care;
• the alternatives (other physiotherapy techniques without needles) and what may happen if I don't have this technique.

I have told the physiotherapist if I am afraid of needles, allergic to metals, have bleeding problems or take blood thinners, have diabetes, epilepsy, am pregnant, have an infection or a condition that weakens immunity, or have implanted devices.

I know I can refuse or ask to stop at any time, without giving a reason and without any effect on the rest of my treatment. I accept the proposed technique. This document is made in two copies.
$t$)
) AS d(kind, lang, title, valid_months, body);

-- Trim the leading/trailing newline from the dollar-quoted bodies.
UPDATE consent_texts SET body = btrim(body, E'\n') WHERE approved_at IS NULL;
