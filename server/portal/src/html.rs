//! The site's pages as plain HTML. Every value from the database goes through `esc`.

use crate::{href, BookForm, Data, Lang, Page, Service, Site};
use axum::http::StatusCode;
use std::fmt::Write;

/// What a page needs besides the database: the request form's values, errors and token.
#[derive(Default)]
pub struct Extra {
    pub form: BookForm,
    pub errors: Vec<(&'static str, String)>,
    pub token: Option<(String, String)>,
    pub sent: bool,
    pub status: StatusCode,
}

pub fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"), '<' => o.push_str("&lt;"), '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"), '\'' => o.push_str("&#39;"), c => o.push(c),
        }
    }
    o
}

/// Owner's text: blank lines make paragraphs, single line breaks stay.
fn paras(s: &str) -> String {
    s.split("\n\n").map(str::trim).filter(|p| !p.is_empty())
        .map(|p| format!("<p>{}</p>", esc(p).replace('\n', "<br>"))).collect()
}

pub fn money(cents: i32, l: Lang) -> String {
    let (e, c) = (cents / 100, (cents % 100).abs());
    let mut int = e.to_string();
    if e >= 1000 {
        let sep = if l == Lang::Pt { "\u{a0}" } else { "," };
        int = format!("{}{sep}{:03}", e / 1000, e % 1000);
    }
    if l == Lang::Pt { format!("{int},{c:02}\u{a0}€") } else { format!("€{int}.{c:02}") }
}

fn digits(s: &str) -> String { s.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect() }

pub(crate) const MARK_PATH: &str = "M40 300 H150 C165 300 168 262 182 262 C196 262 200 338 212 338 C224 338 230.0 300 230.0 246 L230.0 178 A20.0 20.0 0 0 1 270.0 178 L270.0 236 A3.0 3.0 0 0 0 276.0 236 L276.0 132 A20.0 20.0 0 0 1 316.0 132 L316.0 236 A3.0 3.0 0 0 0 322.0 236 L322.0 150 A20.0 20.0 0 0 1 362.0 150 L362.0 236 A2.0 2.0 0 0 0 366.0 236 L366.0 196 A20.0 20.0 0 0 1 406.0 196 L406.0 250 C406.0 300 412.0 326 426.0 326 C440.0 326 440.0 274 454.0 274 C468.0 274 468.0 300 486.0 300 H600";
/// The mark (the pulse line rising into an open hand), drawn in the text colour.
const LOGO: &str = r##"<svg class="logo" viewBox="20 90 600 270" aria-hidden="true"><path d="M40 300 H150 C165 300 168 262 182 262 C196 262 200 338 212 338 C224 338 230.0 300 230.0 246 L230.0 178 A20.0 20.0 0 0 1 270.0 178 L270.0 236 A3.0 3.0 0 0 0 276.0 236 L276.0 132 A20.0 20.0 0 0 1 316.0 132 L316.0 236 A3.0 3.0 0 0 0 322.0 236 L322.0 150 A20.0 20.0 0 0 1 362.0 150 L362.0 236 A2.0 2.0 0 0 0 366.0 236 L366.0 196 A20.0 20.0 0 0 1 406.0 196 L406.0 250 C406.0 300 412.0 326 426.0 326 C440.0 326 440.0 274 454.0 274 C468.0 274 468.0 300 486.0 300 H600"/></svg>"##;

fn other(l: Lang) -> Lang { if l == Lang::Pt { Lang::En } else { Lang::Pt } }

fn head(title: &str, l: Lang, page: Option<Page>, desc: &str) -> String {
    let alt = page.map(|p| format!(r#"<link rel="alternate" hreflang="pt" href="{}"><link rel="alternate" hreflang="en" href="{}">"#, href(p, Lang::Pt), href(p, Lang::En))).unwrap_or_default();
    format!(r#"<!doctype html><html lang="{lang}"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>{title}</title><meta name="description" content="{desc}"><meta name="color-scheme" content="light dark"><link rel="icon" href="/icon.svg" type="image/svg+xml"><link rel="stylesheet" href="/site.css">{alt}</head>"#,
        lang = if l == Lang::Pt { "pt-PT" } else { "en" }, title = esc(title), desc = esc(desc))
}

pub(crate) fn layout(d: &Data, page: Page, l: Lang, title: &str, body: &str) -> String {
    let s = &d.site;
    let name = esc(s.name());
    let mut nav = String::new();
    for (p, label) in [(Page::Home, l.t("Início", "Home")), (Page::Services, l.t("Serviços e preços", "Services and prices")), (Page::Team, l.t("Equipa", "Team"))] {
        let cur = if p == page { r#" aria-current="page""# } else { "" };
        let _ = write!(nav, r#"<a href="{}"{cur}>{label}</a>"#, href(p, l));
    }
    let book = if s.requests_open { format!(r#"<a class="btn pri" href="{}"><span class="long">{}</span><span class="short" aria-hidden="true">{}</span></a>"#, href(Page::Book, l), l.t("Pedir marcação", "Request an Appointment"), l.t("Marcar", "Book")) } else { String::new() };
    let switch = format!(r#"<a class="lang" href="{}" hreflang="{code}" lang="{code}" aria-label="{aria}">{short}</a>"#, href(page, other(l)), code = other(l).code(),
        aria = l.t("Read this page in English", "Ler esta página em português"), short = if l == Lang::Pt { "EN" } else { "PT" });
    let full_title = if page == Page::Home { format!("{} — {}", s.name(), l.t(&s.tagline_pt, &s.tagline_en)) } else { format!("{title} — {}", s.name()) };
    format!(r##"{head}<body><a class="skip" href="#main">{skip}</a><header class="top"><div class="bar"><a class="brand" href="{home}">{LOGO}<span>{name}</span></a><nav aria-label="{navl}">{nav}</nav><div class="acts">{switch}{book}</div></div></header><main id="main">{body}</main>{foot}</body></html>"##,
        head = head(&full_title, l, Some(page), l.t(&s.intro_pt, &s.intro_en)), skip = l.t("Saltar para o conteúdo", "Skip to content"),
        home = href(Page::Home, l), navl = l.t("Principal", "Main"), foot = footer(s, l))
}

fn footer(s: &Site, l: Lang) -> String {
    let mut c = String::new();
    if !s.phone.trim().is_empty() { let _ = write!(c, r#"<a href="tel:{}">{}</a>"#, esc(&digits(&s.phone)), esc(&s.phone)); }
    if !s.email.trim().is_empty() { let _ = write!(c, r#"<a href="mailto:{0}">{0}</a>"#, esc(&s.email)); }
    let place = if s.opening_soon || s.address.trim().is_empty() { l.t("Abertura em breve", "Opening soon").to_string() } else { esc(&s.address) };
    let mut legal = vec![];
    if !s.nif.trim().is_empty() { legal.push(format!("NIF {}", esc(&s.nif))); }
    if !s.ers_number.trim().is_empty() { legal.push(format!("{} {}", l.t("Registo ERS n.º", "ERS registration no."), esc(&s.ers_number))); }
    format!(r#"<footer class="foot"><div class="cols"><div><p class="fn">{name}</p><p>{place}</p><p class="contact">{c}</p></div><div><p class="fh">{info}</p><p class="links"><a href="{priv}">{privl}</a><a href="{terms}">{termsl}</a><a href="{acc}">{accl}</a></p></div><div><p class="fh">{compl}</p><p class="links"><a href="https://www.livroreclamacoes.pt/Inicio/" rel="noopener">{book}</a><a href="https://www.ers.pt/" rel="noopener">{ers}</a></p></div></div><p class="small">{legal}</p></footer>"#,
        name = esc(s.name()), info = l.t("Informação", "Information"),
        priv = href(Page::Privacy, l), privl = l.t("Política de privacidade", "Privacy policy"),
        terms = href(Page::Terms, l), termsl = l.t("Termos de utilização", "Terms of use"),
        acc = href(Page::Account, l), accl = l.t("Área de utente", "Patient area"),
        compl = l.t("Reclamações", "Complaints"), book = l.t("Livro de Reclamações Eletrónico", "Electronic Complaints Book"),
        ers = l.t("Entidade Reguladora da Saúde", "Health Regulatory Authority (ERS)"), legal = legal.join(" · "))
}

pub fn render(d: &Data, page: Page, l: Lang, x: &Extra) -> String {
    // Before publishing, only the legal pages are shown in full, so the owner can read them first.
    if !d.site.published && !matches!(page, Page::Privacy | Page::Terms) {
        return coming_soon(&d.site, l, page);
    }
    match page {
        Page::Home => layout(d, page, l, "", &home(d, l)),
        Page::Services => layout(d, page, l, l.t("Serviços e preços", "Services and prices"), &services(d, l)),
        Page::Team => layout(d, page, l, l.t("Equipa", "Team"), &team(d, l)),
        Page::Book => layout(d, page, l, l.t("Pedir marcação", "Request an appointment"), &book(d, l, x)),
        Page::Privacy => layout(d, page, l, l.t("Política de privacidade", "Privacy policy"), &privacy(&d.site, l)),
        Page::Terms => layout(d, page, l, l.t("Termos de utilização", "Terms of use"), &terms(&d.site, l)),
        Page::Account => layout(d, page, l, l.t("Área de utente", "Patient area"), &account(&d.site, l)),
    }
}

// ───────── pages ─────────

fn visit<'a>(d: &'a Data, kind: &str) -> Option<&'a Service> {
    d.services.iter().find(|s| s.kind == "visit" && s.visit_kind.as_deref() == Some(kind))
}

fn hours(s: &Site, l: Lang) -> String {
    let days = [("mon", "Segunda", "Monday"), ("tue", "Terça", "Tuesday"), ("wed", "Quarta", "Wednesday"), ("thu", "Quinta", "Thursday"),
                ("fri", "Sexta", "Friday"), ("sat", "Sábado", "Saturday"), ("sun", "Domingo", "Sunday")];
    let mut o = String::from(r#"<dl class="hours">"#);
    for (k, pt, en) in days {
        let v = &s.opening_hours[k];
        let t = match (v.get(0).and_then(|x| x.as_str()), v.get(1).and_then(|x| x.as_str())) {
            (Some(a), Some(b)) => format!("{}–{}", esc(a), esc(b)),
            _ => l.t("Encerrado", "Closed").to_string(),
        };
        let _ = write!(o, "<dt>{}</dt><dd>{t}</dd>", l.t(pt, en));
    }
    o.push_str("</dl>");
    o
}

pub(crate) fn date_text(d: time::Date, l: Lang) -> String {
    let m = d.month() as usize - 1;
    const PT: [&str; 12] = ["janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro", "outubro", "novembro", "dezembro"];
    const EN: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    if l == Lang::Pt { format!("{} de {}", d.day(), PT[m]) } else { format!("{} {}", d.day(), EN[m]) }
}

fn closures(d: &Data, l: Lang) -> String {
    if d.closures.is_empty() { return String::new(); }
    let items: String = d.closures.iter().map(|c| {
        let when = if c.starts_on == c.ends_on { date_text(c.starts_on, l) } else { format!("{} – {}", date_text(c.starts_on, l), date_text(c.ends_on, l)) };
        let label = if c.label.trim().is_empty() { String::new() } else { format!(" — {}", esc(&c.label)) };
        format!("<li>{when}{label}</li>")
    }).collect();
    format!(r#"<div class="note"><p class="h">{}</p><ul>{items}</ul></div>"#, l.t("Dias em que estaremos encerrados", "Days we'll be closed"))
}

pub(crate) fn contacts(s: &Site, l: Lang) -> String {
    let mut o = String::new();
    if !s.phone.trim().is_empty() { let _ = write!(o, r#"<p><span class="k">{}</span><a href="tel:{}">{}</a></p>"#, l.t("Telefone", "Phone"), esc(&digits(&s.phone)), esc(&s.phone)); }
    if !s.email.trim().is_empty() { let _ = write!(o, r#"<p><span class="k">Email</span><a href="mailto:{0}">{0}</a></p>"#, esc(&s.email)); }
    let place = if s.opening_soon || s.address.trim().is_empty() { l.t("O espaço da clínica abre em breve. Até lá, fazemos avaliações e sessões ao domicílio.", "The clinic's space opens soon. Until then, we do assessments and sessions at home.").to_string() } else { esc(&s.address) };
    let _ = write!(o, r#"<p><span class="k">{}</span>{place}</p>"#, l.t("Onde", "Where"));
    o
}

/// The opening hours in a few lines: "Segunda a sexta · 08:00–20:00".
fn hours_short(s: &Site, l: Lang) -> Vec<(String, String)> {
    let keys = ["mon", "tue", "wed", "thu", "fri", "sat", "sun"];
    let pt = ["segunda", "terça", "quarta", "quinta", "sexta", "sábado", "domingo"];
    let en = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
    let val = |k: &str| { let v = &s.opening_hours[k]; match (v.get(0).and_then(|x| x.as_str()), v.get(1).and_then(|x| x.as_str())) { (Some(a), Some(b)) => Some(format!("{a}–{b}")), _ => None } };
    let mut out = vec![];
    let mut i = 0;
    while i < 7 {
        let v = val(keys[i]);
        let mut j = i;
        while j + 1 < 7 && val(keys[j + 1]) == v { j += 1; }
        if let Some(h) = v {
            let day = |n: usize| if l == Lang::Pt { pt[n].to_string() } else { en[n].to_string() };
            let mut d = if j > i { format!("{} {} {}", day(i), l.t("a", "to"), day(j)) } else { day(i) };
            if let Some(f) = d.get(0..1) { d = f.to_uppercase() + &d[1..]; }
            out.push((d, h));
        }
        i = j + 1;
    }
    out
}

fn home(d: &Data, l: Lang) -> String {
    let s = &d.site;
    let book = href(Page::Book, l);
    let price = |k: &str| visit(d, k).filter(|_| s.show_prices).map(|v| money(v.price_cents, l));
    let qs = if l == Lang::Pt { "servico" } else { "service" };

    // Facts under the buttons, all from the clinic's own data.
    let mut facts: Vec<String> = vec![];
    if s.opening_soon { facts.push(l.t("Abertura em breve", "Opening soon").into()); }
    if s.home_visits { facts.push(l.t("Também ao domicílio", "Home visits too").into()); }
    if let Some((dd, hh)) = hours_short(s, l).first() { facts.push(format!("{} {}", esc(dd), esc(hh))); }
    let facts = facts.join(" · ");

    // The panel: the mark, then what each visit costs. On a phone it becomes the request box ("O que precisa?").
    let mut opts = vec![("assessment", l.t("Avaliação inicial", "First assessment"), l.t("Primeira vez", "First time"))];
    opts.push(("session", l.t("Sessão", "Session"), l.t("Com o mesmo fisioterapeuta", "With the same physiotherapist")));
    if s.home_visits { opts.push(("home", l.t("Ao domicílio", "Home visit"), l.t("Vamos a sua casa", "We come to your home"))); }
    let rows: String = opts.iter().enumerate().map(|(i, (v, t, sub))| {
        let p = price(v).map(|p| format!(r#"<em>{p}</em>"#)).unwrap_or_default();
        format!(r#"<label class="row"><input type="radio" name="{qs}" value="{v}"{}><span><b>{t}</b><small>{sub}</small></span>{p}</label>"#, if i == 0 { " checked" } else { "" })
    }).collect();
    let panel = if s.requests_open {
        format!(r#"<form class="panel" method="get" action="{book}"><svg class="panel-mark" viewBox="20 90 600 270" aria-hidden="true"><path d="{MARK_PATH}"/></svg><fieldset><legend>{ask}</legend>{rows}</fieldset><button class="btn pri wide" type="submit">{go} →</button></form>"#,
            ask = l.t("O que precisa?", "What do you need?"), go = l.t("Pedir marcação", "Request an Appointment"))
    } else {
        format!(r#"<div class="panel"><svg class="panel-mark" viewBox="20 90 600 270" aria-hidden="true"><path d="{MARK_PATH}"/></svg><fieldset><legend>{}</legend>{rows}</fieldset></div>"#, l.t("Preços", "Prices"))
    };
    let tag = esc(l.t(&s.tagline_pt, &s.tagline_en));
    // The last words of the promise in teal (the owner's text, split at its last two words).
    let words: Vec<&str> = tag.split(' ').collect();
    let h1 = if words.len() > 3 { format!("{} <span>{}</span>", words[..words.len() - 3].join(" "), words[words.len() - 3..].join(" ")) } else { tag.clone() };
    let ctas = if s.requests_open {
        format!(r#"<p class="ctas"><a class="btn pri" href="{book}">{} →</a><a class="btn soft" href="{}">{}</a></p>"#, l.t("Pedir marcação", "Request an Appointment"), href(Page::Services, l), l.t("Ver preços", "See Prices"))
    } else { String::new() };

    let points = [
        (l.t("Sessões individuais", "One-to-one sessions"), l.t("Uma pessoa de cada vez.", "One person at a time.")),
        (l.t("O mesmo fisioterapeuta", "The same physiotherapist"), l.t("Do início ao fim do tratamento.", "From the start to the end of treatment.")),
        (l.t("Preços à vista", "Prices up front"), l.t("Sabe quanto paga antes de marcar.", "You know the price before you book.")),
    ];
    let points: String = points.iter().map(|(t, p)| format!("<li><b>{t}</b>{p}</li>")).collect();

    let steps = [
        (l.t("Pede", "You ask"), l.t("Aqui, por telefone ou por email. Basta o nome e um contacto.", "Here, by phone or by email. A name and a contact is enough.")),
        (l.t("Confirmamos", "We confirm"), l.t("Respondemos com uma hora que lhe dê jeito.", "We answer with a time that suits you.")),
        (l.t("Primeira avaliação", "First assessment"), l.t("Ouvimos, avaliamos e combinamos o plano consigo.", "We listen, assess you and agree the plan with you.")),
    ];
    let steps: String = steps.iter().map(|(t, p)| format!("<li><b>{t}</b><span>{p}</span></li>")).collect();
    let mut c = String::new();
    if !s.phone.trim().is_empty() { let _ = write!(c, r#"<p><span class="k">{}</span><a href="tel:{}">{}</a></p>"#, l.t("Telefone", "Phone"), esc(&digits(&s.phone)), esc(&s.phone)); }
    if !s.email.trim().is_empty() { let _ = write!(c, r#"<p><span class="k">Email</span><a href="mailto:{0}">{0}</a></p>"#, esc(&s.email)); }
    let place = if s.opening_soon || s.address.trim().is_empty() { l.t("O espaço abre em breve. Até lá, fazemos avaliações e sessões ao domicílio.", "The space opens soon. Until then, we do assessments and sessions at home.").to_string() } else { esc(&s.address) };
    let _ = write!(c, r#"<p><span class="k">{}</span>{place}</p>"#, l.t("Onde", "Where"));

    format!(r#"<section class="hero"><div class="hero-text"><h1>{h1}</h1><div class="lead">{intro}</div>{ctas}<p class="facts">{facts}</p></div>{panel}</section>
<ul class="points">{points}</ul>
<section class="duo"><div class="box"><h2>{howl}</h2><ol class="steps">{steps}</ol></div><div class="box"><h2>{hl}</h2>{hours}{closed}</div></section>
<section class="box contact"><h2>{cl}</h2><div class="contact-grid">{c}</div></section>"#,
        intro = paras(l.t(&s.intro_pt, &s.intro_en)),
        howl = l.t("Como marcar", "How to book"), hl = l.t("Horário", "Opening hours"), hours = hours(s, l), closed = closures(d, l), cl = l.t("Contactos", "Contacts"))
}

fn services(d: &Data, l: Lang) -> String {
    let s = &d.site;
    let groups: [(&str, &str, Box<dyn Fn(&Service) -> bool>); 4] = [
        ("Consultas e sessões", "Appointments and sessions", Box::new(|v: &Service| v.kind == "visit" && v.applies_to == "session" && (s.home_visits || v.visit_kind.as_deref() != Some("home")))),
        ("Pacotes", "Packs", Box::new(|v: &Service| v.kind == "pack" && v.applies_to == "session")),
        ("Planos mensais", "Monthly plans", Box::new(|v: &Service| v.kind == "monthly" && v.applies_to == "session")),
        ("Aulas de grupo", "Group classes", Box::new(|v: &Service| v.applies_to == "class")),
    ];
    let mut o = format!(r#"<section class="phead"><h1>{}</h1><p>{}</p></section>"#, l.t("Serviços e preços", "Services and prices"),
        if s.show_prices { l.t("Preços por pessoa, em euros. Os pacotes contam a partir da primeira sessão.", "Prices per person, in euros. Packs count from the first session.") }
        else { l.t("Fale connosco para saber os preços.", "Get in touch for prices.") });
    let mut any_taxed = false;
    for (pt, en, f) in groups.iter() {
        let items: Vec<&Service> = d.services.iter().filter(|v| f(v)).collect();
        if items.is_empty() { continue; }
        let mut rows = String::new();
        for v in items {
            let mut sub = vec![];
            if v.kind == "pack" {
                if let Some(n) = v.sessions { sub.push(format!("{n} {}", if v.applies_to == "class" { l.t("aulas", "classes") } else { l.t("sessões", "sessions") })); }
                if let Some(m) = v.valid_months { sub.push(format!("{} {m} {}", l.t("válido", "valid for"), if m == 1 { l.t("mês", "month") } else { l.t("meses", "months") })); }
            }
            if v.kind == "monthly" { sub.push(l.t("pago por mês", "paid monthly").to_string()); }
            sub.push(if v.vat_exempt { l.t("isento de IVA", "VAT exempt").to_string() } else { any_taxed = true; l.t("IVA incluído", "VAT included").to_string() });
            let price = if s.show_prices { format!(r#"<span class="fig">{}{}</span>"#, money(v.price_cents, l), if v.kind == "monthly" { l.t("/mês", "/month") } else { "" }) } else { String::new() };
            let _ = write!(rows, r#"<li><span class="what"><b>{}</b><small>{}</small></span><span class="leader" aria-hidden="true"></span>{price}</li>"#, esc(l.t(&v.name_pt, &v.name_en)), sub.join(" · "));
        }
        let _ = write!(o, r#"<section class="sheet"><h2>{}</h2><ul class="pricelist">{rows}</ul></section>"#, l.t(pt, en));
    }
    let _ = write!(o, r#"<p class="small fine">{}{}</p>"#,
        l.t("A fisioterapia com fim terapêutico está isenta de IVA (art. 9.º, n.º 1 do Código do IVA).", "Physiotherapy with a therapeutic purpose is VAT exempt (article 9(1) of the Portuguese VAT Code)."),
        if any_taxed { l.t(" Os restantes preços incluem IVA à taxa em vigor.", " Other prices include VAT at the current rate.") } else { "" });
    if s.requests_open {
        let _ = write!(o, r#"<p class="ctas"><a class="btn pri lg" href="{}">{}</a></p>"#, href(Page::Book, l), l.t("Pedir marcação", "Request an Appointment"));
    }
    o
}

fn team(d: &Data, l: Lang) -> String {
    let mut o = format!(r#"<section class="phead"><h1>{}</h1><p>{}</p></section>"#, l.t("Equipa", "Team"),
        l.t("Cada utente é acompanhado sempre pelo mesmo fisioterapeuta.", "Each patient always sees the same physiotherapist."));
    if d.team.is_empty() {
        let _ = write!(o, r#"<section class="sheet"><p>{}</p></section>"#, l.t("Em breve apresentamos aqui a nossa equipa.", "We'll introduce our team here soon."));
        return o;
    }
    o.push_str(r#"<section class="sheet"><ul class="people">"#);
    for p in d.team.iter() {
        let title = l.t(&p.title_pt, &p.title_en);
        let title = if title.trim().is_empty() { l.t("Fisioterapeuta", "Physiotherapist") } else { title };
        let _ = write!(o, r#"<li><h2>{}</h2><p class="lbl">{}</p>{}</li>"#, esc(&p.name), esc(title), paras(l.t(&p.bio_pt, &p.bio_en)));
    }
    o.push_str("</ul></section>");
    o
}

pub(crate) fn field_err(x: &Extra, f: &str) -> Option<String> {
    x.errors.iter().find(|(k, _)| *k == f).map(|(_, m)| m.clone())
}

fn book(d: &Data, l: Lang, x: &Extra) -> String {
    let s = &d.site;
    let title = format!(r#"<section class="phead"><h1>{}</h1></section>"#, l.t("Pedir marcação", "Request an appointment"));
    if x.sent {
        return format!(r#"{title}<div class="card done" role="status"><h2>{}</h2><p>{}</p><p>{}</p><p><a class="btn" href="{}">{}</a></p></div>"#,
            l.t("Pedido enviado", "Request sent"),
            l.t("Obrigado. Vamos ver a agenda e respondemos-lhe por telefone ou email, normalmente no próprio dia útil.", "Thank you. We'll check the schedule and get back to you by phone or email, usually the same working day."),
            l.t("A marcação só fica feita quando a confirmarmos.", "The appointment is only booked once we confirm it."),
            href(Page::Home, l), l.t("Voltar ao início", "Back to Home"));
    }
    if !s.requests_open {
        return format!(r#"{title}<div class="card"><p>{}</p>{}</div>"#, l.t("De momento não recebemos pedidos pelo site. Fale connosco:", "We aren't taking requests on the website right now. Get in touch:"), contacts(s, l));
    }
    let f = &x.form;
    let mut o = title;
    if !x.errors.is_empty() {
        let items: String = x.errors.iter().map(|(k, m)| if *k == "form" { format!("<li>{}</li>", esc(m)) } else { format!(r##"<li><a href="#f-{k}">{}</a></li>"##, esc(m)) }).collect();
        let _ = write!(o, r#"<div class="errors" role="alert"><p class="h">{}</p><ul>{items}</ul></div>"#, l.t("Falta corrigir alguns dados:", "A few details need fixing:"));
    }
    let err = |k: &str| field_err(x, k).map(|m| format!(r#"<p class="err" id="e-{k}">{}</p>"#, esc(&m))).unwrap_or_default();
    let inv = |k: &str| if field_err(x, k).is_some() { format!(r#" aria-invalid="true" aria-describedby="e-{k}""#) } else { String::new() };
    let checked = |a: &str, b: &str| if a == b { " checked" } else { "" };

    let mut svc = String::new();
    let mut opts = vec![("assessment", l.t("Avaliação inicial", "First assessment"), l.t("Primeira vez connosco", "Your first time with us")),
                        ("session", l.t("Sessão", "Session"), l.t("Já é nosso utente", "You're already a patient"))];
    if s.home_visits { opts.push(("home", l.t("Ao domicílio", "Home visit"), l.t("Vamos a sua casa", "We come to your home"))); }
    for (i, (v, t, sub)) in opts.iter().enumerate() {
        let id = if i == 0 { r#" id="f-service""# } else { "" };
        let p = visit(d, v).filter(|_| s.show_prices).map(|x| format!(r#"<span class="fig">{}</span>"#, money(x.price_cents, l))).unwrap_or_default();
        let _ = write!(svc, r#"<label class="line"><input type="radio" name="service" value="{v}"{id}{}{} required><span class="what"><b>{t}</b><small>{sub}</small></span><span class="leader" aria-hidden="true"></span>{p}</label>"#, checked(&f.service, v), inv("service"));
    }
    let now = time::OffsetDateTime::now_utc();
    let today = now.to_offset(crate::lisbon_offset(now)).date();
    let (min, max) = (today + time::Duration::days(1), today + time::Duration::days(62));
    let parts = [("morning", l.t("Manhã", "Morning")), ("afternoon", l.t("Tarde", "Afternoon")), ("any", l.t("Tanto faz", "Either"))];
    let mut part = String::new();
    for (i, (v, t)) in parts.iter().enumerate() {
        let id = if i == 0 { r#" id="f-part""# } else { "" };
        let _ = write!(part, r#"<label class="chip"><input type="radio" name="part" value="{v}"{id}{} required><span>{t}</span></label>"#, checked(if f.part.is_empty() { "any" } else { &f.part }, v));
    }
    let (t, sg) = x.token.clone().unwrap_or_default();
    let _ = write!(o, r#"<form class="stub torn form" method="post" action="{action}" novalidate>
<p class="lead">{lead}</p>
<fieldset><legend>{svcl}</legend>{svc}{e_service}</fieldset>
<div class="row2"><div class="f"><label for="f-name">{namel}</label><input id="f-name" name="name" autocomplete="name" maxlength="120" value="{name}" required{i_name}>{e_name}</div>
<div class="f"><label for="f-phone">{phonel}</label><input id="f-phone" name="phone" type="tel" autocomplete="tel" maxlength="40" value="{phone}"{i_phone}>{e_phone}</div></div>
<div class="row2"><div class="f"><label for="f-email">Email</label><input id="f-email" name="email" type="email" autocomplete="email" maxlength="200" value="{email}"{i_email}>{e_email}<p class="hint">{contact_hint}</p></div>
<div class="f"><label for="f-day">{dayl}</label><input id="f-day" name="day" type="date" min="{min}" max="{max}" value="{day}" required{i_day}>{e_day}</div></div>
<fieldset><legend>{partl}</legend><div class="chips">{part}</div>{e_part}</fieldset>
<div class="f"><label for="f-address">{addrl}</label><input id="f-address" name="address" autocomplete="street-address" maxlength="300" value="{address}"{i_address}>{e_address}</div>
<div class="f"><label for="f-message">{msgl}</label><textarea id="f-message" name="message" rows="3" maxlength="1000"{i_message}>{message}</textarea><p class="hint">{msg_hint}</p>{e_message}</div>
<div class="hp" aria-hidden="true"><label for="f-website">{hp}</label><input id="f-website" name="website" tabindex="-1" autocomplete="off"></div>
<input type="hidden" name="t" value="{t}"><input type="hidden" name="s" value="{sg}">
<div class="f"><label class="check"><input id="f-privacy" type="checkbox" name="privacy"{privchk}{i_privacy}><span>{priv}</span></label>{e_privacy}</div>
<p class="ctas"><button class="btn pri lg" type="submit">{send}</button></p>
<p class="small">{note}</p></form>{closed}"#,
        action = crate::href(Page::Book, l),
        lead = l.t("Diga-nos o que precisa e quando lhe dá jeito. Respondemos por telefone ou email para combinar a hora.", "Tell us what you need and when suits you. We'll call or email you to agree the time."),
        svcl = l.t("O que pretende", "What would you like"), e_service = err("service"),
        namel = l.t("Nome", "Name"), name = esc(&f.name), i_name = inv("name"), e_name = err("name"),
        phonel = l.t("Telemóvel", "Mobile phone"), phone = esc(&f.phone), i_phone = inv("phone"), e_phone = err("phone"),
        email = esc(&f.email), i_email = inv("email"), e_email = err("email"),
        contact_hint = l.t("Basta o telemóvel ou o email.", "A phone number or an email is enough."),
        dayl = l.t("Dia que prefere", "Preferred day"), day = esc(&f.day), i_day = inv("day"), e_day = err("day"),
        partl = l.t("Período do dia", "Time of day"), e_part = err("part"),
        addrl = if s.home_visits { l.t("Morada (só para ao domicílio)", "Address (home visits only)") } else { l.t("Localidade (opcional)", "Town (optional)") },
        address = esc(&f.address), i_address = inv("address"), e_address = err("address"),
        msgl = l.t("Mensagem (opcional)", "Message (optional)"), message = esc(&f.message), i_message = inv("message"), e_message = err("message"),
        msg_hint = l.t("Não precisa de descrever aqui problemas de saúde: falamos disso na consulta.", "There's no need to describe health problems here: we'll talk about it at the appointment."),
        hp = l.t("Deixe este campo em branco", "Leave this field empty"),
        privchk = if f.privacy == "on" { " checked" } else { "" }, i_privacy = inv("privacy"), e_privacy = err("privacy"),
        priv = format!("{} <a href=\"{}\">{}</a>.", l.t("Li a", "I have read the"), href(Page::Privacy, l), l.t("política de privacidade", "privacy policy")),
        send = l.t("Enviar pedido", "Send Request"),
        note = l.t("Isto é um pedido: a marcação só fica feita quando a confirmarmos.", "This is a request: the appointment is only booked once we confirm it."),
        closed = closures(d, l));
    o
}

fn who(s: &Site, l: Lang) -> String {
    let mut parts = vec![format!("<b>{}</b>", esc(s.name()))];
    if !s.nif.trim().is_empty() { parts.push(format!("NIF {}", esc(&s.nif))); }
    if !s.opening_soon && !s.address.trim().is_empty() { parts.push(esc(&s.address)); }
    if !s.ers_number.trim().is_empty() { parts.push(format!("{} {}", l.t("registo na ERS n.º", "ERS registration no."), esc(&s.ers_number))); }
    let mut c = vec![];
    if !s.email.trim().is_empty() { c.push(format!(r#"<a href="mailto:{0}">{0}</a>"#, esc(&s.email))); }
    if !s.phone.trim().is_empty() { c.push(esc(&s.phone)); }
    format!("{}. {} {}.", parts.join(", "), l.t("Contacto:", "Contact:"), c.join(" · "))
}

/// Draft written by Claude for the owner to approve (claude/LEGAL.md, PRIVACY_NOTICE.md); not legal advice.
fn privacy(s: &Site, l: Lang) -> String {
    let body = if l == Lang::Pt { format!(r#"
<h2>Quem trata os seus dados</h2><p>{who}</p>
<h2>Este site</h2><p>As páginas públicas deste site não usam cookies, não têm estatísticas de visitas nem conteúdos de outras empresas (tipos de letra, mapas, vídeos ou redes sociais). Não guardamos o endereço IP de quem o visita; para travar abusos do formulário guardamos durante dois dias apenas um código derivado dele, que não permite saber quem é.</p>
<h2>Pedidos de marcação</h2><p>Quando faz um pedido de marcação guardamos o que escreve no formulário — nome, telemóvel e/ou email, o serviço, o dia e o período que prefere, a morada (só para visitas ao domicílio) e a mensagem — para lhe responder e combinar a marcação. A base legal é tomar as medidas que pede antes de um contrato de prestação de serviços (RGPD, art. 6.º, n.º 1, al. b)). Se escrever algo sobre a sua saúde, é tratado para a prestação de cuidados de saúde por profissionais sujeitos a sigilo (art. 9.º, n.º 2, al. h); Lei 58/2019, art. 29.º).</p>
<p>Só a equipa da clínica vê os pedidos. Se não chegar a ser nosso utente, o pedido é apagado ao fim de 90 dias. Se for, os dados passam para o seu processo e seguem as regras abaixo.</p>
<h2>Área de utente</h2><p>Se a clínica lhe der um código de convite, pode criar uma conta com o seu email e uma palavra-passe para ver as suas sessões, pedidos e faturas, pedir uma hora e cancelar uma sessão. Guardamos o email, a palavra-passe cifrada (nunca a própria), o código de dois passos se o ativar, e quando entrou pela última vez. Enquanto tem a sessão aberta, o site guarda um único cookie, necessário para saber que é você; apaga-se ao sair ou ao fim de 12 horas. A área de utente nunca mostra notas clínicas, testes nem documentos clínicos. Se deixar de ser nosso utente ou pedir, retiramos o acesso e apagamos a conta.</p>
<h2>Utentes</h2><p>Para o tratar guardamos o seu processo clínico (história clínica, notas de cada sessão, testes, prescrições e documentos), os consentimentos que assinar e os pagamentos. A base legal é a prestação de cuidados de saúde (art. 9.º, n.º 2, al. h) do RGPD) e as obrigações legais e fiscais. Só os profissionais que precisam têm acesso, cada um com a sua conta, e cada abertura do processo fica registada.</p>
<h2>Onde ficam</h2><p>Num servidor da própria clínica, em Portugal. Não usamos serviços de nuvem para estes dados nem os vendemos ou cedemos para publicidade. As cópias de segurança são cifradas. A clínica usa programas de inteligência artificial instalados nos seus próprios computadores para ajudar a escrever alguns documentos; nada sai da clínica e tudo é revisto por um fisioterapeuta.</p>
<h2>A quem podem ser comunicados</h2><p>Ao contabilista (só dados de faturação), à Autoridade Tributária, à ERS em caso de reclamação e, só a seu pedido ou com o seu consentimento, ao seu médico, seguradora ou subsistema de saúde.</p>
<h2>Durante quanto tempo</h2><p>Pedidos de quem não se torna utente: 90 dias. Processo clínico e consentimentos: 20 anos após o último contacto. Faturas: 10 anos. Reclamações: pelo menos 3 anos.</p>
<h2>Os seus direitos</h2><p>Pode pedir acesso aos seus dados e uma cópia, a correção, o apagamento (dentro dos prazos legais), a limitação ou a portabilidade, e opor-se ao tratamento. Escreva-nos para o contacto acima; respondemos no prazo de um mês. Pode também apresentar reclamação à Comissão Nacional de Proteção de Dados (<a href="https://www.cnpd.pt/" rel="noopener">www.cnpd.pt</a>).</p>
<p class="small">Versão 2 · 23 de novembro de 2026</p>"#, who = who(s, l)) } else { format!(r#"
<h2>Who handles your data</h2><p>{who}</p>
<h2>This website</h2><p>The public pages of this website use no cookies, have no visitor statistics and loads nothing from other companies (fonts, maps, videos or social media). We don't store visitors' IP addresses; to stop abuse of the form we keep, for two days, only a code derived from it that can't tell us who you are.</p>
<h2>Appointment requests</h2><p>When you request an appointment we keep what you write in the form — name, mobile number and/or email, the service, the day and time of day you prefer, the address (home visits only) and your message — to answer you and arrange the appointment. The legal basis is taking the steps you ask for before a contract for services (GDPR art. 6(1)(b)). If you write anything about your health, it is handled for the provision of health care by professionals bound by secrecy (art. 9(2)(h); Portuguese Law 58/2019, art. 29).</p>
<p>Only the clinic's team sees requests. If you don't become a patient, the request is deleted after 90 days. If you do, the details move to your record and follow the rules below.</p>
<h2>Patient area</h2><p>If the clinic gives you an invite code, you can create an account with your email and a password to see your sessions, requests and invoices, ask for a time and cancel a session. We keep the email, the password in encrypted form (never the password itself), the two-step code if you turn it on, and when you last signed in. While you're signed in the site keeps a single cookie, needed to know it's you; it's deleted when you sign out or after 12 hours. The patient area never shows clinical notes, tests or clinical documents. If you stop being a patient, or ask us to, we remove access and delete the account.</p>
<h2>Patients</h2><p>To treat you we keep your clinical record (medical history, notes from each session, tests, prescriptions and documents), the consents you sign and your payments. The legal basis is the provision of health care (GDPR art. 9(2)(h)) and our legal and tax obligations. Only the professionals who need it have access, each with their own account, and every time your record is opened it is logged.</p>
<h2>Where it's kept</h2><p>On the clinic's own server, in Portugal. We don't use cloud services for this data and never sell or share it for advertising. Backups are encrypted. The clinic uses artificial-intelligence programs installed on its own computers to help write some documents; nothing leaves the clinic and everything is reviewed by a physiotherapist.</p>
<h2>Who it may be shared with</h2><p>The accountant (billing data only), the Tax Authority, the ERS if there's a complaint, and — only at your request or with your consent — your doctor, insurer or health scheme.</p>
<h2>How long</h2><p>Requests from people who don't become patients: 90 days. Clinical record and consents: 20 years after the last contact. Invoices: 10 years. Complaints: at least 3 years.</p>
<h2>Your rights</h2><p>You can ask for access to your data and a copy, correction, deletion (within the legal limits), restriction or portability, and object to processing. Write to us at the contact above; we answer within one month. You can also complain to the Portuguese data protection authority (<a href="https://www.cnpd.pt/" rel="noopener">www.cnpd.pt</a>).</p>
<p class="small">Version 2 · 23 November 2026</p>"#, who = who(s, l)) };
    format!(r#"<section class="phead"><h1>{}</h1></section><article class="card prose">{body}</article>"#, l.t("Política de privacidade", "Privacy policy"))
}

fn terms(s: &Site, l: Lang) -> String {
    let body = if l == Lang::Pt { format!(r#"
<h2>Quem somos</h2><p>{who}</p>
<h2>O que este site faz</h2><p>Este site apresenta a clínica, os seus serviços e preços, e permite pedir uma marcação. Um pedido feito aqui não é uma marcação: só fica marcado quando a clínica o confirmar por telefone ou email. Para uma situação urgente, ligue 112.</p>
<h2>Preços</h2><p>Os preços mostrados são em euros e indicam se têm IVA incluído ou se estão isentos. Podem mudar; vale o preço em vigor no dia da sessão, exceto pacotes e planos já comprados, que mantêm o preço da compra.</p>
<h2>Informação de saúde</h2><p>Os textos deste site são informação geral e não substituem uma avaliação por um profissional de saúde.</p>
<h2>Reclamações</h2><p>Pode reclamar no <a href="https://www.livroreclamacoes.pt/Inicio/" rel="noopener">Livro de Reclamações Eletrónico</a>, no livro em papel da clínica ou diretamente junto da Entidade Reguladora da Saúde (<a href="https://www.ers.pt/" rel="noopener">www.ers.pt</a>).</p>
<h2>Lei aplicável</h2><p>Aplica-se a lei portuguesa.</p>
<p class="small">Versão 1 · 11 de novembro de 2026</p>"#, who = who(s, l)) } else { format!(r#"
<h2>Who we are</h2><p>{who}</p>
<h2>What this website does</h2><p>This website presents the clinic, its services and prices, and lets you request an appointment. A request made here is not a booking: it is only booked once the clinic confirms it by phone or email. In an emergency, call 112.</p>
<h2>Prices</h2><p>Prices are in euros and say whether VAT is included or exempt. They may change; the price on the day of the session applies, except for packs and plans already bought, which keep the price they were bought at.</p>
<h2>Health information</h2><p>The texts on this website are general information and don't replace an assessment by a health professional.</p>
<h2>Complaints</h2><p>You can complain in the <a href="https://www.livroreclamacoes.pt/Inicio/" rel="noopener">Electronic Complaints Book</a>, in the clinic's paper book, or directly to the Health Regulatory Authority (<a href="https://www.ers.pt/" rel="noopener">www.ers.pt</a>).</p>
<h2>Applicable law</h2><p>Portuguese law applies.</p>
<p class="small">Version 1 · 11 November 2026</p>"#, who = who(s, l)) };
    format!(r#"<section class="phead"><h1>{}</h1></section><article class="card prose">{body}</article>"#, l.t("Termos de utilização", "Terms of use"))
}

fn account(s: &Site, l: Lang) -> String {
    format!(r#"<section class="phead"><h1>{}</h1></section><div class="card"><p class="pill">{}</p><p>{}</p>{}</div>"#,
        l.t("Área de utente", "Patient area"), l.t("Em breve", "Coming soon"),
        l.t("Em breve vai poder ver as suas marcações, sessões e faturas aqui, e pedir os seus documentos. Até lá, fale connosco:", "Soon you'll be able to see your appointments, sessions and invoices here, and request your documents. Until then, get in touch:"),
        contacts(s, l))
}

fn coming_soon(s: &Site, l: Lang, page: Page) -> String {
    let mut c = String::new();
    if !s.phone.trim().is_empty() { let _ = write!(c, r#"<a class="btn" href="tel:{}">{}</a>"#, esc(&digits(&s.phone)), esc(&s.phone)); }
    if !s.email.trim().is_empty() { let _ = write!(c, r#"<a class="btn" href="mailto:{0}">{0}</a>"#, esc(&s.email)); }
    format!(r#"{head}<body class="center"><main class="card soon">{LOGO}<h1>{name}</h1><p>{text}</p><p class="ctas">{c}</p><p class="small"><a href="{privacy}">{privl}</a> · <a href="{other}" lang="{oc}">{ol}</a></p></main></body></html>"#,
        privacy = href(Page::Privacy, l), privl = l.t("Política de privacidade", "Privacy policy"),
        head = head(s.name(), l, Some(page), ""), name = esc(s.name()),
        text = if c.is_empty() { l.t("O nosso site está a ser preparado. Volte em breve.", "Our website is being prepared. Please come back soon.") } else { l.t("O nosso site está a ser preparado. Até lá, fale connosco:", "Our website is being prepared. Until then, get in touch:") },
        other = href(page, other(l)), oc = other(l).code(), ol = l.t("English", "Português"))
}

pub fn unavailable(l: Lang) -> String {
    format!(r#"{head}<body class="center"><main class="card soon">{LOGO}<h1>{t}</h1><p>{p}</p></main></body></html>"#,
        head = head("PhysioPulse", l, None, ""), t = l.t("Voltamos já", "We'll be right back"),
        p = l.t("O site está com um problema momentâneo. Tente daqui a pouco.", "The website has a momentary problem. Please try again shortly."))
}

pub fn not_found(l: Lang, name: &str) -> String {
    let name = if name.trim().is_empty() { "PhysioPulse" } else { name.trim() };
    format!(r#"{head}<body class="center"><main class="card soon">{LOGO}<h1>{t}</h1><p>{p}</p><p class="ctas"><a class="btn pri" href="{home}">{b}</a></p></main></body></html>"#,
        head = head(name, l, None, ""), t = l.t("Página não encontrada", "Page not found"),
        p = l.t("Esta página não existe ou mudou de sítio.", "This page doesn't exist or has moved."),
        home = href(Page::Home, l), b = l.t("Ir para o início", "Go to Home"))
}

/// The "we'll be back soon" page with the axolotl mascot (an original drawing). Self-contained, so it can be
/// copied to wherever it's served from when the home server is down (decided when the .pt domain exists).
pub fn downtime(phone: &str) -> String {
    let call = if phone.trim().is_empty() { String::new() } else { format!(r#"<p class="call">Precisa de nós? Ligue <a href="tel:{0}">{1}</a></p><p class="call" lang="en">Need us? Call <a href="tel:{0}">{1}</a></p>"#, esc(&digits(phone)), esc(phone)) };
    format!(r#"<!doctype html><html lang="pt-PT"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><meta name="color-scheme" content="light dark"><title>Voltamos já · We'll be back soon</title><style>
:root{{--bg:#F4F3F0;--card:#fff;--t:#16201D;--t2:#525C59;--a:#0B7A70}}@media (prefers-color-scheme:dark){{:root{{--bg:#191B1A;--card:#232625;--t:#ECF0EE;--t2:#A9B2AF;--a:#45C6B6}}}}
*{{box-sizing:border-box}}body{{margin:0;min-height:100vh;display:grid;place-items:center;background:var(--bg);color:var(--t);font:17px/1.5 -apple-system,BlinkMacSystemFont,"Segoe UI",system-ui,sans-serif;padding:24px}}
main{{background:var(--card);border-radius:24px;padding:32px 28px;max-width:520px;text-align:center;box-shadow:0 1px 3px rgba(0,0,0,.08)}}h1{{font-size:28px;margin:8px 0 4px}}p{{margin:6px 0;color:var(--t2)}}.call{{margin-top:12px;color:var(--t)}}.call a{{white-space:nowrap}}a{{color:var(--a)}}svg{{width:220px;max-width:70%;height:auto}}</style></head><body><main>
{AXOLOTL}
<h1>Voltamos já</h1><p>Estamos a pôr as páginas no lugar. Volte daqui a pouco.</p><h1 lang="en">We'll be back soon</h1><p lang="en">We're putting the pages back in place. Please check back shortly.</p>{call}</main></body></html>"#)
}

/// An axolotl in a hard hat stacking page blocks. Its feathery gills read as a pulse; axolotls regrow lost
/// tissue, a gentle picture of recovery.
const AXOLOTL: &str = r##"<svg viewBox="0 0 240 200" role="img" aria-label="Um axolote de capacete a empilhar blocos · An axolotl in a hard hat stacking blocks">
<g transform="translate(150 96)"><rect x="0" y="62" width="58" height="34" rx="7" fill="#C3C8EE"/><rect x="6" y="28" width="46" height="32" rx="7" fill="#A9D9CF"/><rect x="14" y="-4" width="34" height="30" rx="7" fill="#EED6B0" transform="rotate(-6 31 11)"/>
<path d="M8 76h12M8 84h28M14 42h12M14 50h24" stroke="#fff" stroke-width="3" stroke-linecap="round"/></g>
<path d="M58 150c-30 4-46-6-48-18 14 8 30 8 44 2z" fill="#E59AB0"/>
<ellipse cx="96" cy="140" rx="54" ry="30" fill="#F2B5C6"/>
<path d="M70 166c-4 10-12 14-18 12M118 166c4 10 12 14 18 12" stroke="#E59AB0" stroke-width="9" stroke-linecap="round" fill="none"/>
<path d="M140 128c8-6 14-4 18 2" stroke="#E59AB0" stroke-width="9" stroke-linecap="round" fill="none"/>
<g fill="none" stroke="#B23C60" stroke-width="4" stroke-linecap="round" stroke-linejoin="round">
<path d="M58 78l-14-4 6-8 6 10-16 2"/><path d="M60 92l-20 4 8 6 2-12-12 12"/><path d="M60 106l-18 10 10 2-4-10-8 14"/>
<path d="M140 78l14-4-6-8-6 10 16 2"/><path d="M138 92l20 4-8 6-2-12 12 12"/><path d="M138 106l18 10-10 2 4-10 8 14"/></g>
<ellipse cx="99" cy="96" rx="44" ry="36" fill="#F7C6D3"/>
<circle cx="84" cy="92" r="5" fill="#16201D"/><circle cx="114" cy="92" r="5" fill="#16201D"/><circle cx="86" cy="90" r="1.6" fill="#fff"/><circle cx="116" cy="90" r="1.6" fill="#fff"/>
<path d="M88 108q11 9 22 0" stroke="#16201D" stroke-width="3" fill="none" stroke-linecap="round"/>
<circle cx="74" cy="104" r="5" fill="#EE9BB2" opacity=".7"/><circle cx="124" cy="104" r="5" fill="#EE9BB2" opacity=".7"/>
<path d="M60 70q39-44 78 0z" fill="#F2B83A"/><rect x="54" y="66" width="90" height="9" rx="4.5" fill="#E0A21F"/><rect x="95" y="34" width="8" height="34" rx="4" fill="#E0A21F"/>
</svg>"##;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn escaping() {
        assert_eq!(esc(r#"<a href="x">'&'</a>"#), "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;");
        assert_eq!(paras("a\n\nb<i>\nc"), "<p>a</p><p>b&lt;i&gt;<br>c</p>");
    }
    #[test]
    fn prices() {
        assert_eq!(money(3000, Lang::Pt), "30,00\u{a0}€");
        assert_eq!(money(17050, Lang::En), "€170.50");
        assert_eq!(money(123456, Lang::Pt), "1\u{a0}234,56\u{a0}€");
        assert_eq!(money(123456, Lang::En), "€1,234.56");
    }
    #[test]
    fn every_page_is_listed() { assert_eq!(crate::PAGES.len(), 7); }
}
