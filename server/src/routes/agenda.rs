//! The schedule, for the assistant and for one-press booking (owner, 11 Nov 2026: "book it automatically with a
//! press of a button"; the assistant "adds appointments, understands the ones I'm talking about, changes them").
//!
//! - **Free times** (`free_slots`): the first times that fit a day, a part of the day and a length, inside the
//!   opening hours, outside closures, not clashing with a therapist's booked appointments (travel time counts).
//! - **Booking requests with one press**: `GET /booking-requests/:id/suggest` proposes a time and says whether the
//!   person already has a file; `POST /booking-requests/:id/book` makes the file if needed and books, in one
//!   transaction. Undo is the usual Reopen, which now also removes a file the press made if nothing else uses it.
//! - **The assistant's schedule jobs** (book, move, cancel, mark done/missed, accept/decline a request, add to the
//!   waitlist, free times, questions about the agenda). The AI sees the agenda of the coming two weeks with patient
//!   names (owner's choice, 11 Nov 2026), each line with a short reference (A12, R3) it answers with; the server
//!   checks everything again against the database and prepares a preview that waits for Apply. Never clinical notes,
//!   contacts or money. Open to whoever has the AI switch on (reception included); see `assistant::ask`.

use super::assist::{date_of, fold, DAYS};
use super::assistant::{hm, Proposal};
use super::schedule::{self, tz, NewAppointment, Repeat};
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::{Postgres, Transaction};
use uuid::Uuid;

type Tx<'a> = Transaction<'a, Postgres>;

pub const WD_PT: [&str; 7] = ["seg", "ter", "qua", "qui", "sex", "sáb", "dom"];
pub const WD_EN: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

fn say(en: bool, pt: &str, e: &str) -> String {
    if en { e.to_string() } else { pt.to_string() }
}

/// "qui 13/11, 10:00" / "Thu 13/11, 10:00".
pub fn when(day: &str, dow: i32, time: &str, en: bool) -> String {
    let w = if en { WD_EN } else { WD_PT }[((dow - 1).clamp(0, 6)) as usize];
    let d = if day.len() >= 10 { format!("{}/{}", &day[8..10], &day[5..7]) } else { day.to_string() };
    if time.is_empty() { format!("{w} {d}") } else { format!("{w} {d}, {time}") }
}

pub fn kind_name(kind: &str, en: bool) -> String {
    match kind {
        "assessment" => say(en, "Avaliação", "Assessment"),
        "home" => say(en, "Domicílio", "Home visit"),
        "class" => say(en, "Aula", "Class"),
        "block" => say(en, "Bloqueio", "Blocked time"),
        _ => say(en, "Sessão", "Session"),
    }
}

/// The usual length (the New Appointment sheet uses the same).
pub fn minutes_for(kind: &str) -> i32 {
    if matches!(kind, "assessment" | "home") { 60 } else { 45 }
}

pub(crate) fn service_kind(service: &str) -> &'static str {
    match service { "assessment" => "assessment", "home" => "home", _ => "session" }
}

fn utc(col: &str) -> String {
    format!("to_char(({col}) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"')")
}

// ───────── people ─────────

#[derive(Clone)]
pub struct Person { pub id: Uuid, pub name: String }

/// Who can be booked: active owner and therapists, oldest first (the same list as the Schedule).
pub async fn therapists(s: &AppState) -> ApiResult<Vec<Person>> {
    let v: Vec<(Uuid, String)> = sqlx::query_as("SELECT id, name FROM staff WHERE active AND role IN ('owner', 'therapist') ORDER BY created_at").fetch_all(&s.pool).await?;
    Ok(v.into_iter().map(|(id, name)| Person { id, name }).collect())
}

/// "Ana" → the only therapist with the word "ana" in their name. Err(n) = none (0) or several.
pub(crate) fn find_therapist(list: &[Person], name: &str) -> Result<Person, usize> {
    let q = fold(name);
    if q.is_empty() {
        return Err(0);
    }
    if let Some(p) = list.iter().find(|p| fold(&p.name) == q) {
        return Ok(p.clone());
    }
    let m: Vec<&Person> = list.iter().filter(|p| { let n = fold(&p.name); let w: Vec<&str> = n.split(' ').collect(); q.split(' ').all(|x| w.contains(&x)) }).collect();
    if m.len() == 1 { Ok(m[0].clone()) } else { Err(m.len()) }
}

#[derive(Clone)]
pub struct Patient { pub id: Uuid, pub name: String, pub number: i32, pub phone: String, pub therapist_id: Option<Uuid> }

impl Patient {
    pub fn label(&self) -> String { format!("{} (nº {})", self.name, self.number) }
}

pub enum Found { One(Patient), Many(Vec<Patient>), None }

/// "nº 12", "n.º 12", "#12", "numero 12" → 12.
fn number_in(text: &str) -> Option<i32> {
    let t = fold(&text.replace(['º', '°', '.'], ""));
    let w: Vec<&str> = t.split(' ').collect();
    for (i, x) in w.iter().enumerate() {
        if matches!(*x, "n" | "no" | "nº" | "numero" | "number" | "num") {
            if let Some(n) = w.get(i + 1).and_then(|n| n.parse().ok()) { return Some(n); }
        }
        if let Some(n) = x.strip_prefix('n').and_then(|n| n.parse().ok()) { return Some(n); }
    }
    text.split('#').nth(1).and_then(|r| r.chars().take_while(|c| c.is_ascii_digit()).collect::<String>().parse().ok())
}

/// The patient the person means. Exact name, else every word typed appears in the name; a number decides.
pub async fn find_patient(s: &AppState, text: &str) -> ApiResult<Found> {
    let all: Vec<(Uuid, String, i32, Option<String>, Option<Uuid>)> = sqlx::query_as(
        "SELECT id, name, number, phone, therapist_id FROM patients WHERE deleted_at IS NULL AND anonymised_at IS NULL ORDER BY active DESC, number")
        .fetch_all(&s.pool).await?;
    let all: Vec<Patient> = all.into_iter().map(|(id, name, number, phone, therapist_id)| Patient { id, name, number, phone: phone.unwrap_or_default(), therapist_id }).collect();
    if let Some(n) = number_in(text) {
        if let Some(p) = all.iter().find(|p| p.number == n) {
            return Ok(Found::One(p.clone()));
        }
    }
    // Words that aren't part of a name: "(nº 12)", "a", "o", "da".
    let q: String = fold(text).split(' ').filter(|w| w.parse::<i32>().is_err() && !matches!(*w, "n" | "no" | "numero" | "a" | "o" | "sr" | "sra" | "dona")).collect::<Vec<_>>().join(" ");
    if q.is_empty() {
        return Ok(Found::None);
    }
    let exact: Vec<&Patient> = all.iter().filter(|p| fold(&p.name) == q).collect();
    let m: Vec<&Patient> = if exact.is_empty() {
        all.iter().filter(|p| { let n = fold(&p.name); let w: Vec<&str> = n.split(' ').collect(); q.split(' ').all(|x| w.contains(&x)) }).collect()
    } else { exact };
    Ok(match m.len() {
        0 => Found::None,
        1 => Found::One(m[0].clone()),
        _ => Found::Many(m.into_iter().take(6).cloned().collect()),
    })
}

// ───────── the agenda ─────────

#[derive(Clone, sqlx::FromRow)]
pub struct Appt {
    pub id: Uuid, pub kind: String, pub status: String, pub day: String, pub dow: i32, pub start: String, pub end: String,
    pub starts_at: String, pub ends_at: String, pub therapist_id: Option<Uuid>, pub therapist: String,
    pub patient_id: Option<Uuid>, pub patient: String, pub number: Option<i32>, pub title: String, pub series: bool,
}

impl Appt {
    pub fn who(&self, en: bool) -> String {
        match self.kind.as_str() {
            "class" => if self.title.is_empty() { kind_name("class", en) } else { self.title.clone() },
            "block" => if self.title.is_empty() { kind_name("block", en) } else { self.title.clone() },
            _ => match self.number { Some(n) => format!("{} (nº {n})", self.patient), None => self.patient.clone() },
        }
    }
    pub fn when(&self, en: bool) -> String { format!("{}–{}", when(&self.day, self.dow, &self.start, en), self.end) }
    pub fn minutes(&self) -> i64 {
        let m = |t: &str| t.get(..2).and_then(|h| h.parse::<i64>().ok()).unwrap_or(0) * 60 + t.get(3..5).and_then(|x| x.parse::<i64>().ok()).unwrap_or(0);
        (m(&self.end) - m(&self.start)).max(0)
    }
    /// What the app shows in a list: {id, when, title, sub, kind, status, patient_id}.
    pub fn item(&self, en: bool) -> Value {
        let mut sub = vec![kind_name(&self.kind, en)];
        if !self.therapist.is_empty() { sub.push(self.therapist.clone()); }
        if self.status != "booked" { sub.push(status_name(&self.status, en)); }
        json!({ "id": self.id, "when": self.when(en), "title": self.who(en), "sub": sub.join(" · "), "kind": self.kind, "status": self.status, "patient_id": self.patient_id })
    }
}

pub fn status_name(st: &str, en: bool) -> String {
    match st {
        "done" => say(en, "Realizada", "Done"),
        "no_show" => say(en, "Faltou", "Missed"),
        "cancelled" => say(en, "Cancelada", "Cancelled"),
        _ => say(en, "Marcada", "Booked"),
    }
}

fn appt_sql(filter: &str) -> String {
    format!(
        "SELECT ap.id, ap.kind, ap.status, to_char(ap.starts_at AT TIME ZONE $1, 'YYYY-MM-DD') AS day, extract(isodow FROM ap.starts_at AT TIME ZONE $1)::int AS dow,
                to_char(ap.starts_at AT TIME ZONE $1, 'HH24:MI') AS start, to_char(ap.ends_at AT TIME ZONE $1, 'HH24:MI') AS \"end\", {} AS starts_at, {} AS ends_at,
                ap.therapist_id, COALESCE(st.name, '') AS therapist, ap.patient_id, COALESCE(p.name, '') AS patient, p.number, ap.title,
                ap.series_id IS NOT NULL AS series
         FROM appointments ap LEFT JOIN staff st ON st.id = ap.therapist_id LEFT JOIN patients p ON p.id = ap.patient_id
         WHERE {filter} ORDER BY ap.starts_at, st.name, ap.id",
        utc("ap.starts_at"), utc("ap.ends_at"))
}

/// Appointments from `back` days ago to `ahead` days on (cancelled ones left out), at most `max`.
pub async fn agenda(s: &AppState, back: i32, ahead: i32, max: i64) -> ApiResult<Vec<Appt>> {
    let sql = appt_sql("ap.status <> 'cancelled' AND (ap.starts_at AT TIME ZONE $1)::date BETWEEN (now() AT TIME ZONE $1)::date - $2 AND (now() AT TIME ZONE $1)::date + $3") + " LIMIT $4";
    Ok(sqlx::query_as::<_, Appt>(&sql).bind(tz()).bind(back).bind(ahead).bind(max).fetch_all(&s.pool).await?)
}

#[derive(Clone)]
pub struct Req {
    pub id: Uuid, pub name: String, pub phone: String, pub email: String, pub patient_id: Option<Uuid>, pub service: String,
    pub day: String, pub dow: i32, pub time: String, pub part: String, pub source: String, pub message: String, pub address: String, pub status: String,
}

impl Req {
    /// When the person asked for: "sex 14/11, de manhã" (website) or "sex 14/11, 10:00".
    pub fn asked(&self, en: bool) -> String {
        let t = if self.source == "website" {
            match self.part.as_str() { "morning" => say(en, "de manhã", "morning"), "afternoon" => say(en, "à tarde", "afternoon"), _ => say(en, "a qualquer hora", "any time") }
        } else { self.time.clone() };
        when(&self.day, self.dow, &t, en)
    }
}

const REQ_SQL: &str = "SELECT id, name, phone, email, patient_id, service, to_char(preferred_at AT TIME ZONE $1, 'YYYY-MM-DD'),
        extract(isodow FROM preferred_at AT TIME ZONE $1)::int, to_char(preferred_at AT TIME ZONE $1, 'HH24:MI'), day_part, source, message, address, status
    FROM booking_requests";
type ReqRow = (Uuid, String, String, String, Option<Uuid>, String, String, i32, String, String, String, String, String, String);
fn req_of(r: ReqRow) -> Req {
    Req { id: r.0, name: r.1, phone: r.2, email: r.3, patient_id: r.4, service: r.5, day: r.6, dow: r.7, time: r.8, part: r.9, source: r.10, message: r.11, address: r.12, status: r.13 }
}

pub async fn pending_requests(s: &AppState) -> ApiResult<Vec<Req>> {
    let rows: Vec<ReqRow> = sqlx::query_as(&format!("{REQ_SQL} WHERE status = 'pending' AND kind <> 'cancelled' ORDER BY preferred_at LIMIT 40")).bind(tz()).fetch_all(&s.pool).await?;
    Ok(rows.into_iter().map(req_of).collect())
}

async fn request(tx: &mut Tx<'_>, id: Uuid, lock: bool) -> ApiResult<Req> {
    let sql = format!("{REQ_SQL} WHERE id = $2{}", if lock { " FOR UPDATE" } else { "" });
    let r: Option<ReqRow> = sqlx::query_as(&sql).bind(tz()).bind(id).fetch_optional(&mut **tx).await?;
    r.map(req_of).ok_or(AppError::NotFound)
}

// ───────── free times ─────────

#[derive(Clone, Debug)]
pub struct Slot { pub starts_at: String, pub ends_at: String, pub day: String, pub dow: i32, pub time: String, pub therapist_id: Uuid, pub therapist: String }

impl Slot {
    pub fn label(&self, en: bool) -> String { format!("{} · {}", when(&self.day, self.dow, &self.time, en), self.therapist) }
    pub fn item(&self, en: bool, minutes: i32) -> Value {
        json!({ "when": when(&self.day, self.dow, &self.time, en), "title": self.therapist, "sub": format!("{minutes} min"),
                "starts_at": self.starts_at, "ends_at": self.ends_at, "therapist_id": self.therapist_id })
    }
}

fn to_min(t: &str) -> i32 {
    t.get(..2).and_then(|h| h.parse::<i32>().ok()).unwrap_or(0) * 60 + t.get(3..5).and_then(|m| m.parse::<i32>().ok()).unwrap_or(0)
}

/// What to look for.
pub struct Want<'a> {
    pub day: &'a str,          // first day to look at (YYYY-MM-DD)
    pub part: &'a str,         // "" | morning | afternoon | any
    pub around: Option<&'a str>, // a time asked for: closest first
    pub minutes: i32,
    pub travel: i32,
    pub therapists: Vec<Person>, // in order of preference
    pub days: i32,             // how many days to look at
    pub max: usize,
    pub one_per_time: bool,    // one therapist per time (lists); false = every therapist free then
}

/// The first free times that fit, inside opening hours, never in the past (30 minutes from now at the earliest).
/// Morning = starting before 13:00, afternoon = from 13:00. Whole-clinic blocked time (closures) blocks everyone.
pub async fn free_slots(s: &AppState, w: &Want<'_>) -> ApiResult<Vec<Slot>> {
    let (hours,): (Value,) = sqlx::query_as("SELECT opening_hours FROM clinic WHERE id = 1").fetch_one(&s.pool).await?;
    let ids: Vec<Uuid> = w.therapists.iter().map(|t| t.id).collect();
    let mut out: Vec<Slot> = Vec::new();
    if ids.is_empty() || w.minutes <= 0 {
        return Ok(out);
    }
    let days: Vec<(String, i32)> = sqlx::query_as("SELECT d::date::text, extract(isodow FROM d)::int FROM generate_series($1::date, $1::date + $2, interval '1 day') d")
        .bind(w.day).bind((w.days - 1).clamp(0, 60)).fetch_all(&s.pool).await?;
    for (day, dow) in days {
        let h = &hours[DAYS[((dow - 1).clamp(0, 6)) as usize]];
        let (Some(o), Some(c)) = (h[0].as_str(), h[1].as_str()) else { continue };
        let (open, close) = (to_min(o), to_min(c));
        let mut times: Vec<i32> = (0..).map(|i| open + i * 15).take_while(|t| t + w.minutes <= close).filter(|t| match w.part {
            "morning" => *t < 13 * 60, "afternoon" => *t >= 13 * 60, _ => true }).collect();
        if let Some(a) = w.around.map(to_min) {
            times.sort_by_key(|t| ((t - a).abs(), *t));
        }
        if times.is_empty() {
            continue;
        }
        let labels: Vec<String> = times.iter().map(|t| format!("{:02}:{:02}", t / 60, t % 60)).collect();
        let rows: Vec<(String, String, String, Uuid)> = sqlx::query_as(&format!(
            "SELECT hh.h, {}, {}, th.id
             FROM unnest($3::text[]) WITH ORDINALITY AS hh(h, i)
             CROSS JOIN LATERAL (SELECT ($1::date + hh.h::time) AT TIME ZONE $2 AS s) g
             CROSS JOIN unnest($4::uuid[]) WITH ORDINALITY AS th(id, o)
             WHERE g.s >= now() + interval '30 minutes'
               AND NOT EXISTS (SELECT 1 FROM appointments x WHERE x.status IN ('booked', 'done') AND (x.therapist_id = th.id OR x.therapist_id IS NULL)
                     AND tstzrange(x.starts_at - make_interval(mins => x.travel_minutes), x.ends_at + make_interval(mins => x.travel_minutes))
                      && tstzrange(g.s - make_interval(mins => $6), g.s + make_interval(mins => $5 + $6)))
             ORDER BY hh.i, th.o LIMIT 400",
            utc("g.s"), utc("g.s + make_interval(mins => $5)")))
            .bind(&day).bind(tz()).bind(&labels).bind(&ids).bind(w.minutes).bind(w.travel.max(0)).fetch_all(&s.pool).await?;
        for (time, st, en, tid) in rows {
            if w.one_per_time && out.iter().any(|x| x.starts_at == st) {
                continue;
            }
            let name = w.therapists.iter().find(|t| t.id == tid).map(|t| t.name.clone()).unwrap_or_default();
            out.push(Slot { starts_at: st, ends_at: en, day: day.clone(), dow, time, therapist_id: tid, therapist: name });
            if out.len() >= w.max {
                return Ok(out);
            }
        }
    }
    Ok(out)
}

/// Therapists in order of preference: `first` (if any) then everyone else.
fn ordered(list: &[Person], first: &[Option<Uuid>]) -> Vec<Person> {
    let mut out: Vec<Person> = Vec::new();
    for f in first.iter().flatten() {
        if let Some(p) = list.iter().find(|p| p.id == *f) {
            if !out.iter().any(|x| x.id == p.id) { out.push(p.clone()); }
        }
    }
    for p in list {
        if !out.iter().any(|x| x.id == p.id) { out.push(p.clone()); }
    }
    out
}

// ───────── booking requests with one press ─────────

/// Patients that look like the person who asked: same phone (last 9 digits), same e-mail or the same name.
async fn lookalikes(s: &AppState, r: &Req) -> ApiResult<Vec<Value>> {
    let phone: String = r.phone.chars().filter(|c| c.is_ascii_digit()).collect();
    let phone = if phone.len() >= 9 { phone[phone.len() - 9..].to_string() } else { String::new() };
    let rows: Vec<(Uuid, i32, String, String)> = sqlx::query_as(
        "SELECT id, number, name, CASE WHEN $1 <> '' AND right(regexp_replace(COALESCE(phone, ''), '[^0-9]', '', 'g'), 9) = $1 THEN 'phone'
                                       WHEN $2 <> '' AND lower(COALESCE(email, '')) = $2 THEN 'email' ELSE 'name' END
         FROM patients WHERE deleted_at IS NULL AND anonymised_at IS NULL
           AND (($1 <> '' AND right(regexp_replace(COALESCE(phone, ''), '[^0-9]', '', 'g'), 9) = $1)
             OR ($2 <> '' AND lower(COALESCE(email, '')) = $2) OR lower(trim(name)) = $3)
         ORDER BY number LIMIT 5")
        .bind(&phone).bind(r.email.trim().to_lowercase()).bind(r.name.trim().to_lowercase()).fetch_all(&s.pool).await?;
    Ok(rows.into_iter().map(|(id, number, name, why)| json!({ "id": id, "number": number, "name": name, "reason": why })).collect())
}

/// The times to offer for a request: its day and part of the day first, then the next two weeks.
async fn request_slots(s: &AppState, r: &Req, therapist: Option<Uuid>, max: usize) -> ApiResult<Vec<Slot>> {
    let kind = service_kind(&r.service);
    let list = therapists(s).await?;
    let today: String = sqlx::query_scalar("SELECT ((now() AT TIME ZONE $1)::date)::text").bind(tz()).fetch_one(&s.pool).await?;
    let day = if r.day < today { today } else { r.day.clone() };
    let part = if r.source == "website" { r.part.as_str() } else { "" };
    let around = if r.source == "website" { None } else { Some(r.time.as_str()) };
    let travel = if kind == "home" { 15 } else { 0 };
    let w = Want { day: &day, part, around, minutes: minutes_for(kind), travel, therapists: ordered(&list, &[therapist]), days: 1, max, one_per_time: true };
    let mut out = free_slots(s, &w).await?;
    if out.len() < max {
        // Nothing (or not enough) that day: the same part of the day on the next 14 days.
        let next: String = sqlx::query_scalar("SELECT ($1::date + 1)::text").bind(&day).fetch_one(&s.pool).await?;
        let w2 = Want { day: &next, around: None, days: 14, max: max - out.len(), ..w };
        out.extend(free_slots(s, &w2).await?);
    }
    Ok(out)
}

/// Everyone: what one press would book for this request — a time, a therapist, and the patient's file.
pub async fn suggest_request(State(s): State<AppState>, _a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    let r = request(&mut tx, id, false).await?;
    tx.commit().await?;
    if r.status != "pending" {
        return Err(AppError::Conflict("This request was already answered."));
    }
    let linked: Option<(Uuid, String, i32, Option<Uuid>)> = match r.patient_id {
        Some(p) => sqlx::query_as("SELECT id, name, number, therapist_id FROM patients WHERE id = $1 AND deleted_at IS NULL AND anonymised_at IS NULL").bind(p).fetch_optional(&s.pool).await?,
        None => None,
    };
    let slots = request_slots(&s, &r, linked.as_ref().and_then(|l| l.3), 4).await?;
    let kind = service_kind(&r.service);
    let slot = |x: &Slot| json!({ "starts_at": x.starts_at, "ends_at": x.ends_at, "therapist_id": x.therapist_id, "therapist_name": x.therapist, "day": x.day, "time": x.time });
    Ok(Json(json!({
        "kind": kind, "minutes": minutes_for(kind),
        "slot": slots.first().map(slot), "alternatives": slots.iter().skip(1).map(slot).collect::<Vec<_>>(),
        "patient": linked.map(|l| json!({ "id": l.0, "name": l.1, "number": l.2 })),
        "lookalikes": if r.patient_id.is_some() { vec![] } else { lookalikes(&s, &r).await? },
    })))
}

#[derive(Deserialize)]
pub struct BookReq {
    starts_at: String,
    ends_at: Option<String>,
    therapist_id: Uuid,
    /// An existing file to use instead of making one (when the request looks like a known patient).
    patient_id: Option<Uuid>,
    #[serde(default)] allow_overlap: bool,
}

pub struct Booked { pub appointment_id: Uuid, pub patient_id: Uuid, pub created: bool, pub overlaps: Value }

/// Makes the file if needed, books and marks the request accepted — all in `tx`.
pub async fn book_request_in(tx: &mut Tx<'_>, a: &Auth, id: Uuid, starts_at: &str, ends_at: Option<&str>, therapist: Uuid, patient: Option<Uuid>, allow: bool) -> ApiResult<Booked> {
    let r = request(tx, id, true).await?;
    if r.status != "pending" {
        return Err(AppError::Conflict("This request was already answered."));
    }
    // A cancellation a patient made on the website is only there to be seen, not booked.
    let notice: bool = sqlx::query_scalar("SELECT kind = 'cancelled' FROM booking_requests WHERE id = $1").bind(id).fetch_one(&mut **tx).await?;
    if notice {
        return Err(AppError::Conflict("This request was already answered."));
    }
    let kind = service_kind(&r.service);
    let (pid, created) = match patient.or(r.patient_id) {
        Some(p) => {
            let ok: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM patients WHERE id = $1 AND deleted_at IS NULL AND anonymised_at IS NULL)").bind(p).fetch_one(&mut **tx).await?;
            if !ok {
                return Err(AppError::BadRequest("Choose the patient."));
            }
            (p, false)
        }
        None => {
            let p = Uuid::new_v4();
            let blank = |x: &str| if x.trim().is_empty() { None } else { Some(x.trim().to_string()) };
            sqlx::query("INSERT INTO patients (id, name, phone, email, address, created_by) VALUES ($1, $2, $3, $4, $5, $6)")
                .bind(p).bind(r.name.trim()).bind(blank(&r.phone)).bind(blank(&r.email)).bind(blank(&r.address)).bind(a.staff.id).execute(&mut **tx).await?;
            (p, true)
        }
    };
    let ends: String = match ends_at {
        Some(e) => e.to_string(),
        None => sqlx::query_scalar(&format!("SELECT {}", utc("$1::timestamptz + make_interval(mins => $2)"))).bind(starts_at).bind(minutes_for(kind)).fetch_one(&mut **tx).await?,
    };
    let appt = NewAppointment {
        kind: kind.into(), starts_at: starts_at.into(), ends_at: ends, therapist_id: Some(therapist), patient_id: Some(pid), prescription_id: None,
        title: String::new(), color: String::new(), address: if kind == "home" { r.address.clone() } else { String::new() },
        travel_minutes: if kind == "home" { 15 } else { 0 }, capacity: None,
        admin_note: if r.message.trim().is_empty() { String::new() } else { r.message.chars().take(500).collect() }, repeat: None, allow_overlap: allow,
    };
    let (ids, _, o) = schedule::create_in(tx, a, &appt).await?;
    sqlx::query("UPDATE booking_requests SET status = 'accepted', appointment_id = $2, decided_by = $3, decided_at = now(), patient_id = $4, created_patient = $5 WHERE id = $1")
        .bind(id).bind(ids[0]).bind(a.staff.id).bind(pid).bind(created).execute(&mut **tx).await?;
    // Phase 4b: a patient asked on the website to move a session; booking the new time cancels the old one.
    let old: Option<Uuid> = sqlx::query_scalar("SELECT about_appointment_id FROM booking_requests WHERE id = $1 AND kind = 'move'").bind(id).fetch_one(&mut **tx).await.ok().flatten();
    if let Some(old) = old {
        let n = sqlx::query("UPDATE appointments SET status = 'cancelled' WHERE id = $1 AND status = 'booked'").bind(old).execute(&mut **tx).await?.rows_affected();
        if n > 0 {
            super::money::on_status(tx, &[old], "cancelled").await?;
        }
    }
    Ok(Booked { appointment_id: ids[0], patient_id: pid, created, overlaps: o })
}

/// Everyone: book a request with one press (the suggested time, or another one picked in the app).
pub async fn book_request(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(b): Json<BookReq>) -> ApiResult<Json<Value>> {
    let mut tx = s.pool.begin().await?;
    let bk = book_request_in(&mut tx, &a, id, &b.starts_at, b.ends_at.as_deref(), b.therapist_id, b.patient_id, b.allow_overlap).await?;
    tx.commit().await?;
    if bk.created {
        auth::log(&s.pool, Some(a.staff.id), "patient.created", Some(("patient", bk.patient_id.to_string())), &a.device, "booking request").await;
    }
    auth::log(&s.pool, Some(a.staff.id), "booking_request.accepted", Some(("patient", bk.patient_id.to_string())), &a.device, &id.to_string()).await;
    Ok(Json(json!({ "appointment_id": bk.appointment_id, "patient_id": bk.patient_id, "patient_created": bk.created, "overlaps": bk.overlaps })))
}

/// A file made for a request goes again on Undo — only if nothing else was recorded for that person.
pub async fn drop_new_patient(tx: &mut Tx<'_>, pid: Uuid) -> ApiResult<bool> {
    let used: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM appointments WHERE patient_id = $1) OR EXISTS (SELECT 1 FROM class_attendees WHERE patient_id = $1)
             OR EXISTS (SELECT 1 FROM notes WHERE patient_id = $1) OR EXISTS (SELECT 1 FROM anamnesis WHERE patient_id = $1)
             OR EXISTS (SELECT 1 FROM prescriptions WHERE patient_id = $1) OR EXISTS (SELECT 1 FROM test_results WHERE patient_id = $1)
             OR EXISTS (SELECT 1 FROM consents WHERE patient_id = $1) OR EXISTS (SELECT 1 FROM documents WHERE patient_id = $1)
             OR EXISTS (SELECT 1 FROM payments WHERE patient_id = $1) OR EXISTS (SELECT 1 FROM patient_plans WHERE patient_id = $1)
             OR EXISTS (SELECT 1 FROM waitlist WHERE patient_id = $1) OR EXISTS (SELECT 1 FROM recordings WHERE patient_id = $1)
             OR EXISTS (SELECT 1 FROM discharge_letters WHERE patient_id = $1) OR EXISTS (SELECT 1 FROM complaints WHERE patient_id = $1)
             OR (SELECT count(*) FROM booking_requests WHERE patient_id = $1) > 1")
        .bind(pid).fetch_one(&mut **tx).await?;
    if used {
        return Ok(false);
    }
    sqlx::query("UPDATE booking_requests SET patient_id = NULL WHERE patient_id = $1").bind(pid).execute(&mut **tx).await?;
    sqlx::query("DELETE FROM patients WHERE id = $1").bind(pid).execute(&mut **tx).await?;
    Ok(true)
}

/// Undo for accept/decline (also the Reopen button): the appointment goes if nothing happened to it yet, and a file
/// the booking made goes if nothing else uses it. Returns whether a file was removed.
pub async fn reopen_in(tx: &mut Tx<'_>, id: Uuid) -> ApiResult<bool> {
    let row: Option<(Option<Uuid>, Option<Uuid>, bool)> = sqlx::query_as("SELECT appointment_id, patient_id, created_patient FROM booking_requests WHERE id = $1 FOR UPDATE")
        .bind(id).fetch_optional(&mut **tx).await?;
    let Some((appt, pid, created)) = row else { return Err(AppError::NotFound) };
    if let Some(ap) = appt {
        let n = sqlx::query("DELETE FROM appointments WHERE id = $1 AND status = 'booked' AND NOT EXISTS (SELECT 1 FROM notes WHERE appointment_id = $1)
                               AND NOT EXISTS (SELECT 1 FROM payments WHERE appointment_id = $1)")
            .bind(ap).execute(&mut **tx).await?.rows_affected();
        if n == 0 {
            return Err(AppError::Conflict("The appointment from this request has already been used, so it can’t be undone."));
        }
    }
    // A move from the website: Undo puts the old session back (only if its time is still free of changes).
    sqlx::query("UPDATE appointments SET status = 'booked' WHERE id = (SELECT about_appointment_id FROM booking_requests WHERE id = $1 AND kind = 'move' AND status = 'accepted')
                 AND status = 'cancelled' AND starts_at > now()").bind(id).execute(&mut **tx).await?;
    sqlx::query("UPDATE booking_requests SET status = 'pending', appointment_id = NULL, decided_by = NULL, decided_at = NULL, created_patient = FALSE WHERE id = $1")
        .bind(id).execute(&mut **tx).await?;
    let mut dropped = false;
    if created {
        if let Some(p) = pid {
            sqlx::query("UPDATE booking_requests SET patient_id = NULL WHERE id = $1").bind(id).execute(&mut **tx).await?;
            dropped = drop_new_patient(tx, p).await?;
            if !dropped {
                sqlx::query("UPDATE booking_requests SET patient_id = $2 WHERE id = $1").bind(id).bind(p).execute(&mut **tx).await?;
            }
        }
    }
    Ok(dropped)
}

// ───────── the assistant ─────────

/// What the AI was shown this time: A1… are appointments, R1… requests (refs are rebuilt on each message).
pub struct Seen { pub appts: Vec<Appt>, pub reqs: Vec<Req> }

impl Seen {
    /// "A12" (the 12th line the AI was shown) or, from the assistant's tools, the start of the id ("3f9c2a1b").
    pub(crate) fn appt(&self, r: &str) -> Option<&Appt> {
        let r = r.trim().trim_start_matches(['A', 'a']).trim_start_matches([':', '-', ' ']);
        if let Some(p) = id_prefix(r) {
            return self.appts.iter().find(|a| a.id.simple().to_string().starts_with(&p));
        }
        let n: usize = r.parse().ok()?;
        self.appts.get(n.checked_sub(1)?)
    }
    pub(crate) fn req(&self, r: &str) -> Option<&Req> {
        let r = r.trim().trim_start_matches(['R', 'r']).trim_start_matches([':', '-', ' ']);
        if let Some(p) = id_prefix(r) {
            return self.reqs.iter().find(|x| x.id.simple().to_string().starts_with(&p));
        }
        let n: usize = r.parse().ok()?;
        self.reqs.get(n.checked_sub(1)?)
    }
}

/// The short reference the assistant's tools show for a row: the first 8 hex digits of its id.
pub fn short_ref(id: Uuid) -> String {
    id.simple().to_string()[..8].to_string()
}

/// "3f9c2a1b" (6 or more hex digits) → the start of an id; "12" → None (a line number).
fn id_prefix(r: &str) -> Option<String> {
    let r = r.to_ascii_lowercase();
    (r.len() >= 6 && r.len() <= 32 && r.chars().all(|c| c.is_ascii_hexdigit())).then_some(r)
}

/// What the assistant's tools can point at: 60 days back to 180 ahead (the server keeps it, the AI sees only
/// what a tool shows), and every waiting request.
pub async fn seen_wide(s: &AppState) -> ApiResult<Seen> {
    Ok(Seen { appts: agenda(s, 60, 180, 5000).await?, reqs: pending_requests(s).await? })
}

/// The agenda for the AI: 3 days back, 14 ahead, at most 250 lines; pending requests; who can be booked.
pub async fn context(s: &AppState) -> ApiResult<(String, Seen)> {
    let appts = agenda(s, 3, 14, 250).await?;
    let reqs = pending_requests(s).await?;
    let ths = therapists(s).await?;
    let mut out = String::from("\nTherapists who can be booked: ");
    out.push_str(&ths.iter().map(|t| t.name.as_str()).collect::<Vec<_>>().join(", "));
    out.push_str("\n\nAgenda (ref | date | weekday | start–end | type | patient or title | therapist | status | series?):\n");
    for (i, x) in appts.iter().enumerate() {
        out.push_str(&format!("A{} | {} | {} | {}–{} | {} | {} | {} | {}{}\n", i + 1, x.day, WD_EN[((x.dow - 1).clamp(0, 6)) as usize], x.start, x.end, x.kind,
            x.who(true), if x.therapist.is_empty() { "whole clinic" } else { &x.therapist }, x.status, if x.series { " | series" } else { "" }));
    }
    if appts.is_empty() {
        out.push_str("(nothing booked)\n");
    }
    out.push_str("\nBooking requests waiting for an answer (ref | name | wants | day | time or part of day | source):\n");
    for (i, r) in reqs.iter().enumerate() {
        out.push_str(&format!("R{} | {} | {} | {} | {} | {}\n", i + 1, r.name, service_kind(&r.service), r.day,
            if r.source == "website" { if r.part.is_empty() { "any" } else { &r.part } } else { &r.time }, r.source));
    }
    if reqs.is_empty() {
        out.push_str("(none)\n");
    }
    Ok((out, Seen { appts, reqs }))
}

pub const SKILLS_HELP: &str = "\
- book: book an appointment. patient = the person's name as said (or as in the agenda); date = YYYY-MM-DD; time = HH:MM; kind = session, assessment or home \
(default session); minutes = length if said; therapist = name if said; repeat_weekdays = ISO weekdays 1–7 and repeat_count = how many in total, only if it repeats \
(\"twice a week for 8 sessions\" → weekdays + 8); phone if said; address for a home visit.\n\
- move: change the time, day or therapist of ONE appointment. appointment = its ref (e.g. A12); date and/or time = the new ones; therapist if it changes.\n\
- cancel: cancel appointments. refs = their refs.\n\
- mark: mark appointments as done (status = done) or missed (status = no_show). refs = their refs.\n\
- accept_request: book a waiting request. request = its ref (e.g. R2); date/time/therapist only if the user gave them.\n\
- decline_request: decline a waiting request. request = its ref.\n\
- waitlist_add: put someone on the waitlist for an earlier slot. patient; repeat_weekdays = days that suit them; part = morning/afternoon/any; therapist if said.\n\
- free_slots: find free times. date = first day to look at; part = morning/afternoon/any; kind or minutes; therapist if said.\n\
- agenda: a question about the schedule (\"what do I have tomorrow?\", \"when is Maria's next session?\", \"how busy is Thursday?\"). \
reply = the answer in one to three short sentences, using only the agenda; refs = the appointments the answer is about.\n\
Use refs exactly as written in the agenda. When the user says \"her\", \"that one\" or \"the 10 o'clock\", work it out from the agenda and the conversation. \
Dates: work them out from today's date (\"Thursday\" = the next Thursday from today, today included).";

fn str_of(it: &Value, k: &str) -> String {
    it[k].as_str().unwrap_or("").trim().to_string()
}

/// An answer that isn't a change: a sentence, maybe a list of appointments or times, maybe choices to tap.
pub struct Said { pub text: String, pub list: Option<Vec<Value>>, pub choices: Option<Vec<Value>> }

pub enum Built { Change(Proposal), Said(Said) }

fn said(text: String) -> Built {
    Built::Said(Said { text, list: None, choices: None })
}

fn choice(label: String, send: String) -> Value {
    json!({ "label": label, "send": send })
}

/// The appointments the AI pointed at, checked: refs, else the patient named (and the day, if given).
fn pick_appts<'a>(seen: &'a Seen, it: &Value) -> Vec<&'a Appt> {
    let mut refs: Vec<String> = it["refs"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    let one = str_of(it, "appointment");
    if !one.is_empty() { refs.insert(0, one); }
    let mut out: Vec<&Appt> = Vec::new();
    for r in refs.iter().take(20) {
        if let Some(a) = seen.appt(r) {
            if !out.iter().any(|x| x.id == a.id) { out.push(a); }
        }
    }
    if out.is_empty() {
        let who = fold(&str_of(it, "patient"));
        let day = date_of(&str_of(it, "date"));
        if !who.is_empty() {
            out = seen.appts.iter().filter(|a| a.kind != "block" && a.status == "booked" && {
                let n = fold(&a.patient); let w: Vec<&str> = n.split(' ').collect(); who.split(' ').all(|x| w.contains(&x)) }
                && day.as_ref().is_none_or(|d| &a.day == d)).collect();
        }
    }
    out
}

/// Which one? — the appointments as choices to tap.
fn which(list: &[&Appt], en: bool) -> Built {
    Built::Said(Said {
        text: say(en, "Qual destas?", "Which one?"),
        list: None,
        choices: Some(list.iter().take(6).map(|a| choice(format!("{} · {}", a.who(en), a.when(en)), format!("{} {}", say(en, "A de", "The one on"), when(&a.day, a.dow, &a.start, en)))).collect()),
    })
}

/// Turns what the AI understood into a checked preview (or a question back). `en` = the app's language.
pub async fn build(s: &AppState, a: &Auth, skill: &str, it: &Value, seen: &Seen, en: bool, ai_reply: Option<String>) -> ApiResult<Built> {
    let ths = therapists(s).await?;
    let today: String = sqlx::query_scalar("SELECT ((now() AT TIME ZONE $1)::date)::text").bind(tz()).fetch_one(&s.pool).await?;
    match skill {
        "agenda" => {
            let picked = pick_appts(seen, it);
            let text = ai_reply.unwrap_or_else(|| say(en, "Aqui está.", "Here it is."));
            let list: Vec<Value> = picked.iter().take(30).map(|x| x.item(en)).collect();
            Ok(Built::Said(Said { text, list: if list.is_empty() { None } else { Some(list) }, choices: None }))
        }
        "free_slots" => {
            let day = date_of(&str_of(it, "date")).filter(|d| *d >= today).unwrap_or(today.clone());
            let kind = match str_of(it, "kind").as_str() { k @ ("assessment" | "home") => k.to_string(), _ => "session".into() };
            let minutes = it["minutes"].as_i64().filter(|m| (15..=240).contains(m)).map(|m| m as i32).unwrap_or(minutes_for(&kind));
            let th = match str_of(it, "therapist") { t if t.is_empty() => None, t => match find_therapist(&ths, &t) { Ok(p) => Some(p), Err(_) => return Ok(said(say(en, "Não encontrei esse terapeuta.", "I couldn’t find that therapist."))) } };
            let list = match &th { Some(p) => vec![p.clone()], None => ths.clone() };
            let part = str_of(it, "part");
            let w = Want { day: &day, part: &part, around: None, minutes, travel: if kind == "home" { 15 } else { 0 }, therapists: list, days: 7, max: 8, one_per_time: true };
            let slots = free_slots(s, &w).await?;
            if slots.is_empty() {
                return Ok(said(say(en, "Não há horas livres nesses dias dentro do horário de funcionamento.", "There are no free times on those days within opening hours.")));
            }
            Ok(Built::Said(Said {
                text: say(en, &format!("Horas livres para {} de {minutes} min:", kind_name(&kind, false).to_lowercase()), &format!("Free times for a {minutes}-minute {}:", kind_name(&kind, true).to_lowercase())),
                list: Some(slots.iter().map(|x| x.item(en, minutes)).collect()), choices: None,
            }))
        }
        "book" => book(s, a, it, &ths, &today, en).await,
        "move" => {
            let picked = pick_appts(seen, it);
            let x = match picked.as_slice() {
                [] => return Ok(said(say(en, "Que marcação quer mudar? Diga o nome do utente e o dia.", "Which appointment should I change? Say the patient’s name and the day."))),
                [x] => (*x).clone(),
                many => return Ok(which(many, en)),
            };
            if x.status != "booked" || x.kind == "block" {
                return Ok(said(say(en, "Só mudo marcações por realizar.", "I only move appointments that are still booked.")));
            }
            let day = date_of(&str_of(it, "date")).unwrap_or(x.day.clone());
            let time = hm(&str_of(it, "time")).unwrap_or(x.start.clone());
            let th = match str_of(it, "therapist") { t if t.is_empty() => None, t => match find_therapist(&ths, &t) { Ok(p) => Some(p), Err(_) => return Ok(said(say(en, "Não encontrei esse terapeuta.", "I couldn’t find that therapist."))) } };
            let tid = th.as_ref().map(|p| p.id).or(x.therapist_id);
            let minutes = x.minutes() as i32;
            let (st, et, dow): (String, String, i32) = sqlx::query_as(&format!("SELECT {}, {}, extract(isodow FROM $1::date)::int", utc("($1::date + $2::time) AT TIME ZONE $3"), utc("(($1::date + $2::time) AT TIME ZONE $3) + make_interval(mins => $4)")))
                .bind(&day).bind(&time).bind(tz()).bind(minutes).fetch_one(&s.pool).await?;
            if st == x.starts_at && tid == x.therapist_id {
                return Ok(said(say(en, "Essa marcação já está a essa hora.", "That appointment is already at that time.")));
            }
            if day < today {
                return Ok(said(say(en, "Escolha um dia a partir de hoje.", "Choose a day from today on.")));
            }
            let clashes = {
                let mut tx = s.pool.begin().await?;
                sqlx::query("UPDATE appointments SET starts_at = $2::timestamptz, ends_at = $3::timestamptz, therapist_id = $4 WHERE id = $1").bind(x.id).bind(&st).bind(&et).bind(tid).execute(&mut *tx).await?;
                let o = schedule::overlaps(&mut tx, &[x.id]).await?;
                tx.rollback().await?;
                o
            };
            let tname = ths.iter().find(|p| Some(p.id) == tid).map(|p| p.name.clone()).unwrap_or(x.therapist.clone());
            let mut warnings = clash_warnings(&clashes, en);
            if x.series { warnings.push(say(en, "Muda só esta da série.", "Only this one of the series changes.")); }
            warnings.extend(outside_hours(s, &day, &time, minutes, en).await?);
            let before = json!({ "starts_at": x.starts_at, "ends_at": x.ends_at, "therapist_id": x.therapist_id, "status": "booked" });
            let after = json!({ "starts_at": st, "ends_at": et, "therapist_id": tid, "status": "booked" });
            Ok(Built::Change(Proposal { skill: "move", target: Some(x.id), params: json!({ "patient_id": x.patient_id, "overlaps": ids_of(&clashes) }), before, after,
                rows: vec![[x.who(en), format!("{} · {}", x.when(en), x.therapist), format!("{}–{} · {tname}", when(&day, dow, &time, en), end_of(&time, minutes))]], warnings }))
        }
        "cancel" | "mark" => {
            let status = if skill == "cancel" { "cancelled".to_string() } else { match str_of(it, "status").as_str() { "no_show" | "missed" => "no_show".into(), _ => "done".into() } };
            let picked: Vec<&Appt> = pick_appts(seen, it).into_iter().filter(|x| x.kind != "block" && (skill == "cancel" || x.kind != "class")).collect();
            if picked.is_empty() {
                return Ok(said(say(en, "Que marcação? Diga o nome do utente e o dia.", "Which appointment? Say the patient’s name and the day.")));
            }
            if picked.len() > 1 && it["refs"].as_array().is_none_or(|r| r.len() < 2) {
                return Ok(which(&picked, en));
            }
            let picked: Vec<&Appt> = picked.into_iter().filter(|x| x.status == "booked").collect();
            if picked.is_empty() {
                return Ok(said(say(en, "Essa marcação já não está por realizar.", "That appointment isn’t booked any more.")));
            }
            let mut warnings = Vec::new();
            if status == "done" && picked.iter().any(|x| x.day > today) { warnings.push(say(en, "Há marcações no futuro: confirme que já foram realizadas.", "Some are in the future: check they really happened.")); }
            if status == "done" { warnings.push(say(en, "Usa a sessão de um pacote ou fica em dívida, como ao marcar à mão.", "Uses a package session or adds a charge, as when marked by hand.")); }
            if status == "cancelled" && picked.iter().any(|x| x.series) { warnings.push(say(en, "Cancela só as indicadas, não a série toda.", "Cancels only these, not the whole series.")); }
            let mut before = serde_json::Map::new();
            let mut after = serde_json::Map::new();
            for x in &picked { before.insert(x.id.to_string(), json!("booked")); after.insert(x.id.to_string(), json!(status)); }
            let ids: Vec<Uuid> = picked.iter().map(|x| x.id).collect();
            let skill: &'static str = if skill == "cancel" { "cancel" } else { "mark" };
            Ok(Built::Change(Proposal { skill, target: if ids.len() == 1 { Some(ids[0]) } else { None },
                params: json!({ "ids": ids, "status": status, "patient_id": picked[0].patient_id }), before: Value::Object(before), after: Value::Object(after),
                rows: picked.iter().map(|x| [format!("{} · {}", x.who(en), x.when(en)), status_name("booked", en), status_name(&status, en)]).collect(), warnings }))
        }
        "accept_request" | "decline_request" => {
            let r = match seen.req(&str_of(it, "request")) {
                Some(r) => r.clone(),
                None => {
                    let who = fold(&str_of(it, "patient"));
                    let m: Vec<&Req> = seen.reqs.iter().filter(|r| !who.is_empty() && { let n = fold(&r.name); let w: Vec<&str> = n.split(' ').collect(); who.split(' ').all(|x| w.contains(&x)) }).collect();
                    match m.as_slice() {
                        [r] => (*r).clone(),
                        [] => return Ok(said(say(en, "Que pedido? Não há nenhum pedido à espera com esse nome.", "Which request? There’s no waiting request with that name."))),
                        many => return Ok(Built::Said(Said { text: say(en, "Qual destes pedidos?", "Which of these requests?"), list: None,
                            choices: Some(many.iter().map(|r| choice(format!("{} · {}", r.name, r.asked(en)), format!("{} {} ({})", say(en, "O pedido de", "The request from"), r.name, r.asked(en)))).collect()) })),
                    }
                }
            };
            let asked = format!("{} · {}", kind_name(service_kind(&r.service), en), r.asked(en));
            if skill == "decline_request" {
                return Ok(Built::Change(Proposal { skill: "decline_request", target: Some(r.id), params: json!({ "request_id": r.id }), before: json!({ "status": "pending" }), after: json!({ "status": "declined" }),
                    rows: vec![[r.name.clone(), asked, say(en, "Recusado", "Declined")]], warnings: vec![say(en, "A pessoa não é avisada automaticamente: ligue ou escreva-lhe.", "The person isn’t told automatically: call or write to them.")] }));
            }
            let linked: Option<(String, i32, Option<Uuid>)> = match r.patient_id { Some(p) => sqlx::query_as("SELECT name, number, therapist_id FROM patients WHERE id = $1").bind(p).fetch_optional(&s.pool).await?, None => None };
            let kind = service_kind(&r.service);
            let th = match str_of(it, "therapist") { t if t.is_empty() => None, t => find_therapist(&ths, &t).ok() };
            let slot = match (date_of(&str_of(it, "date")), hm(&str_of(it, "time"))) {
                (d, Some(t)) => {
                    let day = d.unwrap_or(r.day.clone());
                    let list = ordered(&ths, &[th.as_ref().map(|p| p.id), linked.as_ref().and_then(|l| l.2)]);
                    let list = if th.is_some() { list.into_iter().take(1).collect() } else { list };
                    let w = Want { day: &day, part: "", around: Some(&t), minutes: minutes_for(kind), travel: if kind == "home" { 15 } else { 0 }, therapists: list, days: 1, max: 1, one_per_time: true };
                    free_slots(s, &w).await?.into_iter().find(|x| x.time == t)
                }
                _ => request_slots(s, &r, th.as_ref().map(|p| p.id).or(linked.as_ref().and_then(|l| l.2)), 1).await?.into_iter().next(),
            };
            let Some(slot) = slot else {
                return Ok(said(say(en, "Não encontrei uma hora livre que sirva. Diga o dia e a hora que quer.", "I couldn’t find a free time that fits. Tell me the day and time you want.")));
            };
            let mut warnings = vec![say(en, "A pessoa não é avisada automaticamente: ligue ou escreva-lhe a confirmar.", "The person isn’t told automatically: call or write to confirm.")];
            let who = match &linked {
                Some((n, num, _)) => format!("{n} (nº {num})"),
                None => {
                    let looks = lookalikes(s, &r).await?;
                    if let Some(l) = looks.first() {
                        warnings.insert(0, if en { format!("Looks like {} (no. {}) — if it’s the same person, book from Schedule › Booking Requests.", l["name"].as_str().unwrap_or(""), l["number"]) }
                                           else { format!("Parece ser {} (nº {}) — se for a mesma pessoa, marque em Agenda › Pedidos de marcação.", l["name"].as_str().unwrap_or(""), l["number"]) });
                    }
                    warnings.insert(0, say(en, "Cria uma ficha nova com o nome e os contactos do pedido.", "Makes a new patient file from the request’s name and contacts."));
                    r.name.clone()
                }
            };
            Ok(Built::Change(Proposal { skill: "accept_request", target: Some(r.id),
                params: json!({ "request_id": r.id, "starts_at": slot.starts_at, "ends_at": slot.ends_at, "therapist_id": slot.therapist_id }),
                before: json!({ "status": "pending" }), after: json!({ "status": "accepted" }),
                rows: vec![[who, asked, format!("{}–{} · {}", when(&slot.day, slot.dow, &slot.time, en), end_of(&slot.time, minutes_for(kind)), slot.therapist)]], warnings }))
        }
        "waitlist_add" => {
            let p = match find_patient(s, &str_of(it, "patient")).await? {
                Found::One(p) => p,
                Found::Many(m) => return Ok(many_patients(&m, en)),
                Found::None => return Ok(said(say(en, "Não encontrei esse utente. A lista de espera é para quem já tem ficha; adicione-o em Agenda › Lista de espera.", "I couldn’t find that patient. The waitlist here is for people with a file; add others in Schedule › Waitlist."))),
            };
            let days: Vec<i32> = it["repeat_weekdays"].as_array().map(|a| a.iter().filter_map(|d| d.as_i64()).filter(|d| (1..=7).contains(d)).map(|d| d as i32).collect()).unwrap_or_default();
            let (from, to) = match str_of(it, "part").as_str() { "morning" => (Some("08:00"), Some("13:00")), "afternoon" => (Some("13:00"), Some("20:00")), _ => (None, None) };
            let th = match str_of(it, "therapist") { t if t.is_empty() => None, t => find_therapist(&ths, &t).ok() };
            let wd = if en { WD_EN } else { WD_PT };
            let pref = format!("{}{}{}", if days.is_empty() { say(en, "Qualquer dia", "Any day") } else { days.iter().map(|d| wd[(*d - 1) as usize]).collect::<Vec<_>>().join(", ") },
                match (from, to) { (Some(f), Some(t)) => format!(", {f}–{t}"), _ => String::new() }, th.as_ref().map(|t| format!(" · {}", t.name)).unwrap_or_default());
            Ok(Built::Change(Proposal { skill: "waitlist_add", target: Some(p.id),
                params: json!({ "patient_id": p.id, "weekdays": days, "from_time": from, "to_time": to, "therapist_id": th.map(|t| t.id) }), before: Value::Null, after: Value::Null,
                rows: vec![[p.label(), "—".into(), format!("{} · {pref}", say(en, "Lista de espera", "Waitlist"))]], warnings: vec![] }))
        }
        _ => Ok(said(String::new())),
    }
}

pub(crate) fn many_patients(m: &[Patient], en: bool) -> Built {
    Built::Said(Said {
        text: say(en, "Há mais do que um utente com esse nome. Qual?", "More than one patient has that name. Which one?"),
        list: None,
        choices: Some(m.iter().map(|p| {
            let tail: String = p.phone.chars().filter(|c| c.is_ascii_digit()).collect::<String>().chars().rev().take(3).collect::<Vec<_>>().into_iter().rev().collect();
            choice(if tail.is_empty() { p.label() } else { format!("{} · …{tail}", p.label()) }, p.label())
        }).collect()),
    })
}

fn end_of(time: &str, minutes: i32) -> String {
    let t = to_min(time) + minutes;
    format!("{:02}:{:02}", (t / 60) % 24, t % 60)
}

fn ids_of(clashes: &Value) -> Vec<String> {
    clashes.as_array().map(|a| a.iter().filter_map(|x| x["id"].as_str().map(String::from)).collect()).unwrap_or_default()
}

fn clash_warnings(clashes: &Value, en: bool) -> Vec<String> {
    let n = clashes.as_array().map(|a| a.len()).unwrap_or(0);
    if n == 0 {
        return vec![];
    }
    let names: Vec<String> = clashes.as_array().unwrap().iter().take(3).map(|c| c["patient_name"].as_str().or(c["title"].as_str()).unwrap_or("").to_string()).filter(|x| !x.is_empty()).collect();
    vec![if en { format!("Overlaps {n} other appointment{} of the same therapist{}. Apply books it anyway.", if n == 1 { "" } else { "s" }, if names.is_empty() { String::new() } else { format!(" ({})", names.join(", ")) }) }
         else { format!("Sobrepõe-se a {n} marcaç{} do mesmo terapeuta{}. Aplicar marca na mesma.", if n == 1 { "ão" } else { "ões" }, if names.is_empty() { String::new() } else { format!(" ({})", names.join(", ")) }) }]
}

/// A warning when the time is outside opening hours or on a closure (booking stays possible, as by hand).
async fn outside_hours(s: &AppState, day: &str, time: &str, minutes: i32, en: bool) -> ApiResult<Vec<String>> {
    let (hours,): (Value,) = sqlx::query_as("SELECT opening_hours FROM clinic WHERE id = 1").fetch_one(&s.pool).await?;
    let (dow, closed): (i32, bool) = sqlx::query_as("SELECT extract(isodow FROM $1::date)::int, EXISTS (SELECT 1 FROM closures WHERE $1::date BETWEEN starts_on AND ends_on)")
        .bind(day).fetch_one(&s.pool).await?;
    let h = &hours[DAYS[((dow - 1).clamp(0, 6)) as usize]];
    let mut out = vec![];
    if closed {
        out.push(say(en, "Nesse dia a clínica está fechada (encerramento).", "The clinic is closed that day (closure)."));
    } else {
        match (h[0].as_str(), h[1].as_str()) {
            (Some(o), Some(c)) if to_min(time) >= to_min(o) && to_min(time) + minutes <= to_min(c) => {}
            (Some(o), Some(c)) => out.push(if en { format!("Outside opening hours ({o}–{c}).") } else { format!("Fora do horário de funcionamento ({o}–{c}).") }),
            _ => out.push(say(en, "A clínica está fechada nesse dia da semana.", "The clinic is closed on that weekday.")),
        }
    }
    Ok(out)
}

/// The new appointment(s) as `create_in` takes them, from a stored change's params.
fn appointment_of(p: &Value, allow: bool) -> NewAppointment {
    let st = |k: &str| p[k].as_str().unwrap_or("").to_string();
    NewAppointment {
        kind: st("kind"), starts_at: st("starts_at"), ends_at: st("ends_at"), therapist_id: p["therapist_id"].as_str().and_then(|x| x.parse().ok()),
        patient_id: p["patient_id"].as_str().and_then(|x| x.parse().ok()), prescription_id: None, title: String::new(), color: String::new(),
        address: st("address"), travel_minutes: p["travel_minutes"].as_i64().unwrap_or(0) as i32, capacity: None, admin_note: String::new(),
        repeat: p["repeat"]["weekdays"].as_array().map(|w| Repeat { weekdays: w.iter().filter_map(|d| d.as_i64()).map(|d| d as i32).collect(), until: None, count: p["repeat"]["count"].as_i64() }),
        allow_overlap: allow,
    }
}

async fn book(s: &AppState, a: &Auth, it: &Value, ths: &[Person], today: &str, en: bool) -> ApiResult<Built> {
    let typed = str_of(it, "patient");
    if typed.is_empty() {
        return Ok(said(say(en, "Para quem é a marcação?", "Who is the appointment for?")));
    }
    let found = find_patient(s, &typed).await?;
    let patient = match found {
        Found::One(p) => Some(p),
        Found::Many(m) => return Ok(many_patients(&m, en)),
        Found::None => None,
    };
    let kind = match str_of(it, "kind").as_str() { k @ ("assessment" | "home") => k.to_string(), _ => if patient.is_none() { "assessment".into() } else { "session".into() } };
    let address = str_of(it, "address");
    if kind == "home" && address.is_empty() {
        return Ok(said(say(en, "Qual é a morada da visita ao domicílio?", "What’s the address for the home visit?")));
    }
    let minutes = it["minutes"].as_i64().filter(|m| (15..=240).contains(m)).map(|m| m as i32).unwrap_or(minutes_for(&kind));
    let th_typed = str_of(it, "therapist");
    let th = if th_typed.is_empty() { None } else {
        match find_therapist(ths, &th_typed) { Ok(p) => Some(p), Err(n) => return Ok(said(if n > 1 { say(en, "Há mais do que um terapeuta com esse nome. Diga o nome completo.", "More than one therapist has that name. Use the full name.") } else { say(en, "Não encontrei esse terapeuta.", "I couldn’t find that therapist.") })) }
    };
    let Some(day) = date_of(&str_of(it, "date")) else {
        return Ok(said(say(en, "Para que dia?", "For which day?")));
    };
    if day.as_str() < today {
        return Ok(said(say(en, "Esse dia já passou. Para que dia é?", "That day has passed. Which day is it for?")));
    }
    // Who: the one named, else the patient's own therapist, else the person asking (if they treat), else the first free.
    let me_treats = a.staff.role != "reception";
    let prefer = ordered(ths, &[th.as_ref().map(|p| p.id), patient.as_ref().and_then(|p| p.therapist_id), if me_treats { Some(a.staff.id) } else { None }]);
    let travel = if kind == "home" { 15 } else { 0 };
    let Some(time) = hm(&str_of(it, "time")) else {
        // No time: offer the free ones that day as choices.
        let list = if th.is_some() { prefer.into_iter().take(1).collect() } else { prefer };
        let w = Want { day: &day, part: &str_of(it, "part"), around: None, minutes, travel, therapists: list, days: 1, max: 6, one_per_time: true };
        let slots = free_slots(s, &w).await?;
        if slots.is_empty() {
            return Ok(said(say(en, "A que horas? Não vejo horas livres nesse dia.", "What time? I can’t see free times that day.")));
        }
        return Ok(Built::Said(Said { text: say(en, "A que horas? Estas estão livres:", "What time? These are free:"), list: None,
            choices: Some(slots.iter().map(|x| choice(x.label(en), format!("{} {} {}", say(en, "Às", "At"), x.time, if en { format!("with {}", x.therapist) } else { format!("com {}", x.therapist) }))).collect()) }));
    };
    let therapist = match &th {
        Some(p) => p.clone(),
        None => {
            let w = Want { day: &day, part: "", around: Some(&time), minutes, travel, therapists: prefer.clone(), days: 1, max: 50, one_per_time: false };
            let free = free_slots(s, &w).await?;
            prefer.iter().find(|p| free.iter().any(|x| x.time == time && x.therapist_id == p.id)).cloned().or_else(|| prefer.first().cloned())
                .ok_or(AppError::BadRequest("Choose the therapist."))?
        }
    };
    let (st, et, dow): (String, String, i32) = sqlx::query_as(&format!("SELECT {}, {}, extract(isodow FROM $1::date)::int", utc("($1::date + $2::time) AT TIME ZONE $3"), utc("(($1::date + $2::time) AT TIME ZONE $3) + make_interval(mins => $4)")))
        .bind(&day).bind(&time).bind(tz()).bind(minutes).fetch_one(&s.pool).await?;
    let weekdays: Vec<i64> = it["repeat_weekdays"].as_array().map(|a| a.iter().filter_map(|d| d.as_i64()).filter(|d| (1..=7).contains(d)).collect()).unwrap_or_default();
    let count = it["repeat_count"].as_i64().filter(|c| (2..=50).contains(c));
    let repeat = if !weekdays.is_empty() && count.is_some() { json!({ "weekdays": weekdays, "count": count }) } else { Value::Null };
    let mut params = json!({ "kind": kind, "starts_at": st, "ends_at": et, "therapist_id": therapist.id, "patient_id": patient.as_ref().map(|p| p.id),
        "address": address, "travel_minutes": travel, "repeat": repeat });
    let new_name: String = typed.chars().filter(|c| !c.is_control()).take(120).collect::<String>().trim().to_string();
    let phone = str_of(it, "phone");
    // What it would clash with: book it inside a transaction that is rolled back.
    let staff_id = a.staff.id;
    let mut tx = s.pool.begin().await?;
    let pid_for_dry = match &patient { Some(p) => p.id, None => {
        let p = Uuid::new_v4();
        sqlx::query("INSERT INTO patients (id, name, created_by) VALUES ($1, $2, $3)").bind(p).bind(&new_name).bind(staff_id).execute(&mut *tx).await?;
        p } };
    let mut dry = params.clone();
    dry["patient_id"] = json!(pid_for_dry);
    let (ids, _, clashes) = schedule::create_in(&mut tx, a, &appointment_of(&dry, true)).await?;
    let (last_day, last_dow): (String, i32) = sqlx::query_as("SELECT to_char(max(starts_at) AT TIME ZONE $2, 'YYYY-MM-DD'), extract(isodow FROM max(starts_at) AT TIME ZONE $2)::int FROM appointments WHERE id = ANY($1)")
        .bind(&ids).bind(tz()).fetch_one(&mut *tx).await?;
    tx.rollback().await?;
    params["overlaps"] = json!(ids_of(&clashes));
    let mut warnings = clash_warnings(&clashes, en);
    warnings.extend(outside_hours(s, &day, &time, minutes, en).await?);
    let who = match &patient {
        Some(p) => p.label(),
        None => {
            params["new_patient"] = json!({ "name": new_name, "phone": phone });
            warnings.insert(0, if en { format!("“{new_name}” has no file yet: Apply makes a new one (add the details later in Patients).") }
                               else { format!("«{new_name}» ainda não tem ficha: Aplicar cria uma nova (complete os dados depois em Utentes).") });
            new_name.clone()
        }
    };
    let mut rows = vec![[who, "—".to_string(), format!("{}–{} · {} · {}", when(&day, dow, &time, en), end_of(&time, minutes), kind_name(&kind, en), therapist.name)]];
    if ids.len() > 1 {
        let wd = if en { WD_EN } else { WD_PT };
        rows.push([say(en, "Repete", "Repeats"), "—".into(), if en { format!("{} appointments, {} — last {}", ids.len(), weekdays.iter().map(|d| wd[(*d - 1) as usize]).collect::<Vec<_>>().join(", "), when(&last_day, last_dow, "", true)) }
                                                          else { format!("{} marcações, {} — última {}", ids.len(), weekdays.iter().map(|d| wd[(*d - 1) as usize]).collect::<Vec<_>>().join(", "), when(&last_day, last_dow, "", false)) }]);
    }
    Ok(Built::Change(Proposal { skill: "book", target: patient.as_ref().map(|p| p.id), params, before: Value::Null, after: Value::Null, rows, warnings }))
}

// ───────── apply / undo for the schedule jobs ─────────

fn subset(now: &Value, preview: &Value) -> bool {
    let p: Vec<String> = preview.as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(String::from)).collect()).unwrap_or_default();
    ids_of(now).iter().all(|x| p.contains(x))
}

async fn statuses(tx: &mut Tx<'_>, ids: &[Uuid]) -> ApiResult<Value> {
    let rows: Vec<(Uuid, String)> = sqlx::query_as("SELECT id, status FROM appointments WHERE id = ANY($1)").bind(ids).fetch_all(&mut **tx).await?;
    Ok(Value::Object(rows.into_iter().map(|(i, s)| (i.to_string(), json!(s))).collect()))
}

async fn times(tx: &mut Tx<'_>, id: Option<Uuid>) -> ApiResult<Value> {
    let r: Option<(String, String, Option<Uuid>, String)> = sqlx::query_as(&format!("SELECT {}, {}, therapist_id, status FROM appointments WHERE id = $1 FOR UPDATE", utc("starts_at"), utc("ends_at")))
        .bind(id).fetch_optional(&mut **tx).await?;
    Ok(r.map(|(s, e, t, st)| json!({ "starts_at": s, "ends_at": e, "therapist_id": t, "status": st })).unwrap_or(Value::Null))
}

fn uuids(v: &Value) -> Vec<Uuid> {
    v.as_array().map(|a| a.iter().filter_map(|x| x.as_str().and_then(|s| s.parse().ok())).collect()).unwrap_or_default()
}

/// Apply a schedule change inside `tx`. Returns what to remember for Undo (stored as `after` for book/accept/waitlist)
/// and the patient it was about (access log subject).
pub async fn apply(tx: &mut Tx<'_>, a: &Auth, skill: &str, target: Option<Uuid>, params: &Value, before: &Value, after: &Value) -> ApiResult<(Option<Value>, Option<Uuid>)> {
    const CHANGED: &str = "This was changed since the assistant suggested it. Ask the assistant again.";
    match skill {
        "book" => {
            let mut p = params.clone();
            let mut created = false;
            if p["patient_id"].is_null() {
                let id = Uuid::new_v4();
                let blank = |x: &str| if x.trim().is_empty() { None } else { Some(x.trim().to_string()) };
                sqlx::query("INSERT INTO patients (id, name, phone, address, created_by) VALUES ($1, $2, $3, $4, $5)")
                    .bind(id).bind(p["new_patient"]["name"].as_str().unwrap_or("").trim()).bind(blank(p["new_patient"]["phone"].as_str().unwrap_or("")))
                    .bind(blank(p["address"].as_str().unwrap_or(""))).bind(a.staff.id).execute(&mut **tx).await?;
                p["patient_id"] = json!(id);
                created = true;
            }
            let (ids, _, clashes) = schedule::create_in(tx, a, &appointment_of(&p, true)).await?;
            if !subset(&clashes, &params["overlaps"]) {
                return Err(AppError::Conflict(CHANGED));
            }
            let pid: Option<Uuid> = p["patient_id"].as_str().and_then(|x| x.parse().ok());
            Ok((Some(json!({ "ids": ids, "patient_id": pid, "created_patient": created })), pid))
        }
        "move" => {
            if times(tx, target).await? != *before {
                return Err(AppError::Conflict(CHANGED));
            }
            sqlx::query("UPDATE appointments SET starts_at = $2::timestamptz, ends_at = $3::timestamptz, therapist_id = $4 WHERE id = $1")
                .bind(target).bind(after["starts_at"].as_str()).bind(after["ends_at"].as_str()).bind(after["therapist_id"].as_str().and_then(|x| x.parse::<Uuid>().ok())).execute(&mut **tx).await?;
            let o = schedule::overlaps(tx, &target.into_iter().collect::<Vec<_>>()).await?;
            if !subset(&o, &params["overlaps"]) {
                return Err(AppError::Conflict(CHANGED));
            }
            Ok((None, params["patient_id"].as_str().and_then(|x| x.parse().ok())))
        }
        "cancel" | "mark" => {
            let ids = uuids(&params["ids"]);
            sqlx::query("SELECT 1 FROM appointments WHERE id = ANY($1) FOR UPDATE").bind(&ids).fetch_all(&mut **tx).await?;
            if statuses(tx, &ids).await? != *before {
                return Err(AppError::Conflict(CHANGED));
            }
            let st = params["status"].as_str().unwrap_or("cancelled");
            sqlx::query("UPDATE appointments SET status = $2 WHERE id = ANY($1)").bind(&ids).bind(st).execute(&mut **tx).await?;
            super::money::on_status(tx, &ids, st).await?;
            Ok((None, params["patient_id"].as_str().and_then(|x| x.parse().ok())))
        }
        "accept_request" => {
            let rid: Uuid = params["request_id"].as_str().and_then(|x| x.parse().ok()).ok_or(AppError::NotFound)?;
            let bk = match book_request_in(tx, a, rid, params["starts_at"].as_str().unwrap_or(""), params["ends_at"].as_str(),
                    params["therapist_id"].as_str().and_then(|x| x.parse().ok()).ok_or(AppError::NotFound)?, None, false).await {
                Err(AppError::ConflictWith("overlap", _, _)) => return Err(AppError::Conflict(CHANGED)),
                x => x?,
            };
            Ok((Some(json!({ "status": "accepted", "appointment_id": bk.appointment_id, "patient_id": bk.patient_id, "created_patient": bk.created })), Some(bk.patient_id)))
        }
        "decline_request" => {
            let n = sqlx::query("UPDATE booking_requests SET status = 'declined', decided_by = $2, decided_at = now() WHERE id = $1 AND status = 'pending'")
                .bind(target).bind(a.staff.id).execute(&mut **tx).await?.rows_affected();
            if n == 0 {
                return Err(AppError::Conflict("This request was already answered."));
            }
            Ok((None, None))
        }
        "waitlist_add" => {
            let id = Uuid::new_v4();
            let pid: Option<Uuid> = params["patient_id"].as_str().and_then(|x| x.parse().ok());
            let days: Vec<i32> = params["weekdays"].as_array().map(|a| a.iter().filter_map(|d| d.as_i64()).map(|d| d as i32).collect()).unwrap_or_default();
            sqlx::query("INSERT INTO waitlist (id, patient_id, therapist_id, weekdays, from_time, to_time, note, created_by) VALUES ($1, $2, $3, $4, $5::time, $6::time, '', $7)")
                .bind(id).bind(pid).bind(params["therapist_id"].as_str().and_then(|x| x.parse::<Uuid>().ok())).bind(&days)
                .bind(params["from_time"].as_str()).bind(params["to_time"].as_str()).bind(a.staff.id).execute(&mut **tx).await?;
            Ok((Some(json!({ "id": id })), pid))
        }
        _ => Err(AppError::NotFound),
    }
}

/// Put back what an applied schedule change did, only if nothing else happened to it since.
pub async fn undo(tx: &mut Tx<'_>, skill: &str, target: Option<Uuid>, params: &Value, before: &Value, after: &Value) -> ApiResult<Option<Uuid>> {
    const AGAIN: &str = "This was changed again since, so it can’t be undone here. Change it by hand.";
    let pid: Option<Uuid> = params["patient_id"].as_str().and_then(|x| x.parse().ok());
    match skill {
        "book" => {
            let ids = uuids(&after["ids"]);
            let n = sqlx::query("DELETE FROM appointments ap WHERE ap.id = ANY($1) AND ap.status = 'booked'
                                   AND NOT EXISTS (SELECT 1 FROM notes WHERE appointment_id = ap.id) AND NOT EXISTS (SELECT 1 FROM payments WHERE appointment_id = ap.id)")
                .bind(&ids).execute(&mut **tx).await?.rows_affected();
            if n as usize != ids.len() {
                return Err(AppError::Conflict(AGAIN));
            }
            let p: Option<Uuid> = after["patient_id"].as_str().and_then(|x| x.parse().ok());
            if after["created_patient"].as_bool() == Some(true) {
                if let Some(p) = p { drop_new_patient(tx, p).await?; }
            }
            Ok(p)
        }
        "move" => {
            if times(tx, target).await? != *after {
                return Err(AppError::Conflict(AGAIN));
            }
            sqlx::query("UPDATE appointments SET starts_at = $2::timestamptz, ends_at = $3::timestamptz, therapist_id = $4 WHERE id = $1")
                .bind(target).bind(before["starts_at"].as_str()).bind(before["ends_at"].as_str()).bind(before["therapist_id"].as_str().and_then(|x| x.parse::<Uuid>().ok())).execute(&mut **tx).await?;
            Ok(pid)
        }
        "cancel" | "mark" => {
            let ids = uuids(&params["ids"]);
            sqlx::query("SELECT 1 FROM appointments WHERE id = ANY($1) FOR UPDATE").bind(&ids).fetch_all(&mut **tx).await?;
            if statuses(tx, &ids).await? != *after {
                return Err(AppError::Conflict(AGAIN));
            }
            sqlx::query("UPDATE appointments SET status = 'booked' WHERE id = ANY($1)").bind(&ids).execute(&mut **tx).await?;
            super::money::on_status(tx, &ids, "booked").await?;
            if params["status"] == "cancelled" {
                let o = schedule::overlaps(tx, &ids).await?;
                if o.as_array().is_some_and(|x| !x.is_empty()) {
                    return Err(AppError::Conflict("That time was booked for someone else meanwhile. Book it again by hand."));
                }
            }
            Ok(pid)
        }
        "accept_request" | "decline_request" => {
            let rid: Uuid = params["request_id"].as_str().and_then(|x| x.parse().ok()).ok_or(AppError::NotFound)?;
            let st: Option<String> = sqlx::query_scalar("SELECT status FROM booking_requests WHERE id = $1").bind(rid).fetch_optional(&mut **tx).await?;
            if st.as_deref() != after["status"].as_str() {
                return Err(AppError::Conflict(AGAIN));
            }
            reopen_in(tx, rid).await?;
            Ok(None)
        }
        "waitlist_add" => {
            let id: Option<Uuid> = after["id"].as_str().and_then(|x| x.parse().ok());
            let n = sqlx::query("UPDATE waitlist SET status = 'removed' WHERE id = $1 AND status = 'waiting'").bind(id).execute(&mut **tx).await?.rows_affected();
            if n == 0 {
                return Err(AppError::Conflict(AGAIN));
            }
            Ok(pid)
        }
        _ => Err(AppError::NotFound),
    }
}

/// Access-log detail for a schedule change: what and when, never names.
pub fn detail(skill: &str, params: &Value, after: &Value) -> String {
    let n = match skill {
        "book" => after["ids"].as_array().map(|a| a.len()).unwrap_or(1),
        "cancel" | "mark" => params["ids"].as_array().map(|a| a.len()).unwrap_or(1),
        _ => 1,
    };
    let at = params["starts_at"].as_str().or(after["starts_at"].as_str()).unwrap_or("");
    format!("{skill}{} x{n}{}", params["status"].as_str().map(|s| format!(" {s}")).unwrap_or_default(), if at.is_empty() { String::new() } else { format!(" {at}") })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refs_by_line_or_id() {
        assert_eq!(super::id_prefix("3f9c2a1b").as_deref(), Some("3f9c2a1b"));
        assert_eq!(super::id_prefix("3F9C2A"), Some("3f9c2a".into()));
        assert_eq!(super::id_prefix("12"), None);
        assert_eq!(super::id_prefix("zzzzzz"), None);
        let id = uuid::Uuid::parse_str("3f9c2a1b-0000-4000-8000-000000000000").unwrap();
        assert_eq!(super::short_ref(id), "3f9c2a1b");
    }
    #[test]
    fn numbers_people_type() {
        assert_eq!(number_in("Maria Silva (nº 12)"), Some(12));
        assert_eq!(number_in("utente n.º 7"), Some(7));
        assert_eq!(number_in("#45"), Some(45));
        assert_eq!(number_in("Maria Silva"), None);
    }
    #[test]
    fn labels() {
        assert_eq!(when("2026-11-13", 4, "10:00", false), "qui 13/11, 10:00");
        assert_eq!(when("2026-11-13", 4, "", true), "Thu 13/11");
        assert_eq!(end_of("10:00", 45), "10:45");
        assert_eq!(minutes_for("assessment"), 60);
    }
    #[test]
    fn therapists_by_first_name() {
        let l = vec![Person { id: Uuid::nil(), name: "Ana Rocha".into() }, Person { id: Uuid::new_v4(), name: "João Ana".into() }, Person { id: Uuid::new_v4(), name: "Rui Sá".into() }];
        assert_eq!(find_therapist(&l, "rui").unwrap().name, "Rui Sá");
        assert_eq!(find_therapist(&l, "ana").err(), Some(2));
        assert_eq!(find_therapist(&l, "Ana Rocha").unwrap().name, "Ana Rocha");
    }
}
