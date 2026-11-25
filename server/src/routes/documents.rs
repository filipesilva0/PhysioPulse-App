//! Patient documents (photos, reports, imaging, scanned prescriptions, letters) and the patient archive.
//!
//! Files live in the readable archive on the server's disk, one folder per patient:
//! `Utentes/<n.º>-<nome>/Documentos/` for uploads, `Utentes/<n.º>-<nome>/Notas/` for signed notes (PDF) and
//! `Utentes/<n.º>-<nome>/Consentimentos/` for signed consents and withdrawals (PDF).
//! The folder is found by the number, so renaming a patient keeps using the folder they already have.
//!
//! Roles (owner decision 4 Nov 2026: reception works with prescriptions):
//! - Owner and therapists: every document of a patient.
//! - Reception: scanned prescriptions (upload, open) and the consent PDFs (they collect the signatures). Nothing clinical.
//! - Removing a document: the owner, or whoever uploaded it on the same day (a mistake). Signed-note PDFs
//!   can't be removed (database trigger).
//!
//! Every open of a document is written to the access log (`document.opened`).

use super::money::{pct_encode, sniff, MAX_FILE};
use super::schedule::tz;
use super::util::clinical;
use crate::auth::{self, AppState, Auth};
use crate::error::{ApiResult, AppError};
use crate::export::{self, Line};
use axum::{
    extract::{Path, State},
    http::{header, HeaderValue},
    response::{IntoResponse, Response},
    Json,
};
use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const KINDS: &[&str] = &["photo", "report", "imaging", "prescription", "letter", "other"];

fn subject(id: Uuid) -> Option<(&'static str, String)> {
    Some(("patient", id.to_string()))
}

/// Reception may only handle scanned prescriptions and consent PDFs.
fn may_handle(a: &Auth, kind: &str) -> bool {
    clinical(a) || kind == "prescription" || kind == "consent"
}

/// PDFs the server makes (signed notes, consents) are part of the record: never renamed, re-filed or removed.
fn server_made(kind: &str) -> bool {
    kind == "note" || kind == "consent"
}

/// `Utentes/0007-Maria Silva`: the patient's folder, relative to the archive root.
/// Reuses an existing folder with the same number, so a renamed patient keeps one folder.
pub async fn patient_dir(s: &AppState, pid: Uuid) -> ApiResult<String> {
    let row: Option<(i32, String)> = sqlx::query_as("SELECT number, name FROM patients WHERE id = $1 AND anonymised_at IS NULL")
        .bind(pid).fetch_optional(&s.pool).await?;
    let (number, name) = row.ok_or(AppError::NotFound)?;
    let prefix = format!("{number:04}-");
    if let Ok(mut rd) = tokio::fs::read_dir(s.archive.join("Utentes")).await {
        while let Ok(Some(e)) = rd.next_entry().await {
            let is_dir = e.file_type().await.map(|t| t.is_dir()).unwrap_or(false);
            if let Some(n) = e.file_name().to_str() {
                if is_dir && n.starts_with(&prefix) {
                    return Ok(format!("Utentes/{n}"));
                }
            }
        }
    }
    Ok(format!("Utentes/{prefix}{}", export::safe_name(&name)))
}

/// Writes the bytes under the patient's folder and returns the path relative to the archive root.
pub(crate) async fn write(s: &AppState, pid: Uuid, sub: &str, name: &str, bytes: &[u8]) -> ApiResult<String> {
    let rel = format!("{}/{sub}/{name}", patient_dir(s, pid).await?);
    let full = s.archive.join(&rel);
    if let Some(dir) = full.parent() {
        tokio::fs::create_dir_all(dir).await.map_err(|e| AppError::Internal(format!("archive folder: {}", e.kind())))?;
    }
    tokio::fs::write(&full, bytes).await.map_err(|e| AppError::Internal(format!("archive write: {}", e.kind())))?;
    Ok(rel)
}

#[derive(Deserialize)]
pub struct Upload { kind: String, filename: String, data: String }

/// Add a document to a patient. Body is JSON with the file as base64 (`data`), at most 10 MB;
/// PDF, JPEG, PNG, HEIC or WebP only (checked by the file's first bytes, not its name).
pub async fn upload(State(s): State<AppState>, a: Auth, Path(pid): Path<Uuid>, Json(u): Json<Upload>) -> ApiResult<Json<Value>> {
    if !KINDS.contains(&u.kind.as_str()) {
        return Err(AppError::BadRequest("Choose what this file is."));
    }
    if !may_handle(&a, &u.kind) {
        return Err(AppError::Forbidden);
    }
    let data = u.data.split_once("base64,").map(|x| x.1).unwrap_or(&u.data);
    let bytes = base64::engine::general_purpose::STANDARD.decode(data.trim()).map_err(|_| AppError::BadRequest("That file couldn’t be read. Try again."))?;
    if bytes.is_empty() || bytes.len() > MAX_FILE {
        return Err(AppError::BadRequest("Files must be under 10 MB."));
    }
    let (mime, ext) = sniff(&bytes).ok_or(AppError::BadRequest("Use a PDF or a photo (JPEG, PNG, HEIC)."))?;
    // Keep the person's name for the file, but make the extension match what the file really is.
    let base = export::safe_name(&u.filename);
    let stem = match base.rfind('.') { Some(i) if i > 0 => &base[..i], _ => base.as_str() };
    let filename = format!("{}.{ext}", stem.chars().take(70).collect::<String>());
    let id = Uuid::new_v4();
    let (day,): (String,) = sqlx::query_as(&format!("SELECT to_char(now() AT TIME ZONE '{}', 'YYYY-MM-DD')", tz())).fetch_one(&s.pool).await?;
    let rel = write(&s, pid, "Documentos", &format!("{day}-{}-{filename}", &id.simple().to_string()[..8]), &bytes).await?;
    let sha = hex::encode(Sha256::digest(&bytes));
    let res = sqlx::query("INSERT INTO documents (id, patient_id, kind, filename, mime, bytes, path, sha256, uploaded_by) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(id).bind(pid).bind(&u.kind).bind(&filename).bind(mime).bind(bytes.len() as i64).bind(&rel).bind(sha).bind(a.staff.id)
        .execute(&s.pool).await;
    if let Err(e) = res {
        let _ = tokio::fs::remove_file(s.archive.join(&rel)).await;
        return Err(e.into());
    }
    auth::log(&s.pool, Some(a.staff.id), "document.uploaded", subject(pid), &a.device, &u.kind).await;
    Ok(Json(json!({ "id": id, "kind": u.kind, "filename": filename, "mime": mime, "bytes": bytes.len() })))
}

struct Doc { patient_id: Uuid, kind: String, filename: String, mime: String, path: String, uploaded_by: Option<Uuid>, today: bool, locked: bool }

async fn find(s: &AppState, a: &Auth, id: Uuid) -> ApiResult<Doc> {
    let row: Option<(Uuid, String, String, String, String, Option<Uuid>, bool, bool)> = sqlx::query_as(&format!(
        "SELECT patient_id, kind, filename, mime, path, uploaded_by,
                (uploaded_at AT TIME ZONE '{0}')::date = (now() AT TIME ZONE '{0}')::date, letter_id IS NOT NULL
         FROM documents WHERE id = $1", tz()))
        .bind(id).fetch_optional(&s.pool).await?;
    let (patient_id, kind, filename, mime, path, uploaded_by, today, letter) = row.ok_or(AppError::NotFound)?;
    if !may_handle(a, &kind) {
        // Reception can't tell a clinical document exists.
        return Err(AppError::NotFound);
    }
    Ok(Doc { locked: letter || server_made(&kind), patient_id, kind, filename, mime, path, uploaded_by, today })
}

async fn read(s: &AppState, d: &Doc) -> ApiResult<Vec<u8>> {
    if d.path.contains("..") {
        return Err(AppError::NotFound);
    }
    tokio::fs::read(s.archive.join(&d.path)).await.map_err(|_| AppError::Conflict("The file is missing from the clinic archive. Check the server’s archive folder or restore it from a backup."))
}

/// Open a document inside the app: the file as base64 (shown as a picture or PDF, never saved to the computer).
pub async fn get(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let d = find(&s, &a, id).await?;
    let bytes = read(&s, &d).await?;
    auth::log(&s.pool, Some(a.staff.id), "document.opened", subject(d.patient_id), &a.device, &d.kind).await;
    Ok(Json(json!({
        "id": id, "kind": d.kind, "filename": d.filename, "mime": d.mime, "bytes": bytes.len(),
        "data": base64::engine::general_purpose::STANDARD.encode(&bytes),
    })))
}

/// The file itself, for "Save a Copy…" (the desktop app saves it to Downloads).
pub async fn raw(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Response> {
    let d = find(&s, &a, id).await?;
    let bytes = read(&s, &d).await?;
    auth::log(&s.pool, Some(a.staff.id), "document.saved", subject(d.patient_id), &a.device, &d.kind).await;
    let mut res = bytes.into_response();
    let h = res.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&d.mime).unwrap_or(HeaderValue::from_static("application/octet-stream")));
    if let Ok(v) = HeaderValue::from_str(&format!("attachment; filename*=UTF-8''{}", pct_encode(&d.filename))) {
        h.insert(header::CONTENT_DISPOSITION, v);
    }
    Ok(res)
}

#[derive(Deserialize)]
pub struct Patch { kind: Option<String>, filename: Option<String> }

/// Change what a document is or its name (the file on disk keeps its name).
pub async fn update(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>, Json(p): Json<Patch>) -> ApiResult<Json<Value>> {
    let d = find(&s, &a, id).await?;
    if d.locked {
        return Err(AppError::Conflict("This PDF is part of the clinical record and can’t be changed."));
    }
    let kind = p.kind.unwrap_or(d.kind.clone());
    if !KINDS.contains(&kind.as_str()) {
        return Err(AppError::BadRequest("Choose what this file is."));
    }
    if !may_handle(&a, &kind) {
        return Err(AppError::Forbidden);
    }
    let filename = match p.filename.map(|f| export::safe_name(&f)) { Some(f) => f, None => d.filename.clone() };
    sqlx::query("UPDATE documents SET kind = $2, filename = $3 WHERE id = $1").bind(id).bind(&kind).bind(&filename).execute(&s.pool).await?;
    auth::log(&s.pool, Some(a.staff.id), "document.updated", subject(d.patient_id), &a.device, &kind).await;
    Ok(Json(json!({ "ok": true })))
}

/// Remove a document added by mistake: the owner, or whoever uploaded it, the same day.
pub async fn delete(State(s): State<AppState>, a: Auth, Path(id): Path<Uuid>) -> ApiResult<Json<Value>> {
    let d = find(&s, &a, id).await?;
    if d.locked {
        return Err(AppError::Conflict("This PDF is part of the clinical record and is kept."));
    }
    if a.staff.role != "owner" && !(d.uploaded_by == Some(a.staff.id) && d.today) {
        return Err(AppError::Conflict("Only the owner can remove this document. You can remove documents you added today."));
    }
    sqlx::query("UPDATE prescriptions SET document_id = NULL WHERE document_id = $1").bind(id).execute(&s.pool).await?;
    sqlx::query("DELETE FROM documents WHERE id = $1").bind(id).execute(&s.pool).await?;
    if !d.path.contains("..") {
        let _ = tokio::fs::remove_file(s.archive.join(&d.path)).await;
    }
    auth::log(&s.pool, Some(a.staff.id), "document.removed", subject(d.patient_id), &a.device, &d.kind).await;
    Ok(Json(json!({ "ok": true })))
}

// ───────── signed notes as PDF ─────────

fn template_pt(t: &str) -> &str {
    match t {
        "neck" => "Cervicalgia",
        "lowback" => "Lombalgia",
        "acl" => "LCA pós-operatório",
        "blank" | "" => "",
        other => other,
    }
}

fn block_line(b: &Value) -> Option<(String, String)> {
    let label = b.get("label").and_then(Value::as_str).unwrap_or("").to_string();
    let v = b.get("value").filter(|v| !v.is_null())?;
    let value = match b.get("type").and_then(Value::as_str) {
        Some("slider") => format!("{}{}", v.as_f64().map(|n| if n.fract() == 0.0 { format!("{n:.0}") } else { n.to_string().replace('.', ",") }).unwrap_or_default(),
                                  b.get("unit").and_then(Value::as_str).unwrap_or("")),
        Some("checklist") => v.as_array().map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        _ => v.as_str().unwrap_or("").to_string(),
    };
    if value.trim().is_empty() { None } else { Some((label, value)) }
}

/// The PDF of one signed note (Portuguese, the clinic's language).
async fn note_pdf(s: &AppState, note: Uuid) -> ApiResult<(Uuid, String, Vec<u8>)> {
    let (v,): (Value,) = sqlx::query_as(&format!(
        "SELECT json_build_object(
            'patient_id', p.id, 'number', p.number, 'name', p.name, 'birth', to_char(p.birth_date, 'DD/MM/YYYY'),
            'clinic', (SELECT name FROM clinic LIMIT 1), 'clinic_info', (SELECT json_build_object('name', c.name, 'nif', c.nif, 'ers', c.ers_number, 'address', c.address, 'phone', c.phone, 'email', c.email, 'logo', encode(c.logo, 'base64')) FROM clinic c WHERE c.id = 1),
            'template', n.template, 'blocks', n.blocks, 'text', n.text,
            'written', to_char(n.created_at AT TIME ZONE '{0}', 'DD/MM/YYYY HH24:MI'),
            'signed', to_char(n.signed_at AT TIME ZONE '{0}', 'DD/MM/YYYY \"às\" HH24:MI'),
            'day', to_char(n.signed_at AT TIME ZONE '{0}', 'YYYY-MM-DD'),
            'hhmm', to_char(n.signed_at AT TIME ZONE '{0}', 'HH24\"h\"MI'),
            'appt', to_char(ap.starts_at AT TIME ZONE '{0}', 'DD/MM/YYYY HH24:MI'),
            'author', cb.name, 'signer', sb.name)
         FROM notes n JOIN patients p ON p.id = n.patient_id
         LEFT JOIN appointments ap ON ap.id = n.appointment_id
         LEFT JOIN staff cb ON cb.id = n.created_by LEFT JOIN staff sb ON sb.id = n.signed_by
         WHERE n.id = $1 AND n.signed_at IS NOT NULL", tz()))
        .bind(note).fetch_optional(&s.pool).await?.ok_or(AppError::NotFound)?;
    let st = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let pid: Uuid = st("patient_id").parse().map_err(|_| AppError::NotFound)?;
    let mut lines = vec![Line::Title("Nota de evolução".into())];
    lines.extend(crate::export::clinic_header(&v["clinic_info"], &st("clinic")));
    lines.push(Line::Gap);
    lines.push(Line::Row("Utente".into(), format!("{} (n.º {})", st("name"), v["number"])));
    if !st("birth").is_empty() {
        lines.push(Line::Row("Data de nascimento".into(), st("birth")));
    }
    if !st("appt").is_empty() {
        lines.push(Line::Row("Sessão".into(), st("appt")));
    }
    lines.push(Line::Row("Escrita".into(), format!("{} por {}", st("written"), st("author"))));
    let tpl = template_pt(&st("template")).to_string();
    if !tpl.is_empty() {
        lines.push(Line::Row("Modelo".into(), tpl));
    }
    let blocks: Vec<(String, String)> = v["blocks"].as_array().map(|a| a.iter().filter_map(block_line).collect()).unwrap_or_default();
    if !blocks.is_empty() {
        lines.push(Line::Heading("Registo".into()));
        for (label, value) in blocks {
            for (i, l) in export::wrap(&format!("{label}: {value}"), 10.0, 483.0).into_iter().enumerate() {
                lines.push(Line::Text(if i == 0 { l } else { format!("   {l}") }));
            }
        }
    }
    let text = st("text");
    if !text.trim().is_empty() {
        lines.push(Line::Heading("Notas".into()));
        for para in text.lines() {
            if para.trim().is_empty() {
                lines.push(Line::Gap);
                continue;
            }
            lines.extend(export::wrap(para, 10.0, 483.0).into_iter().map(Line::Text));
        }
    }
    lines.push(Line::Gap);
    lines.push(Line::Gap);
    lines.push(Line::Heading(format!("Assinada por {} a {}", st("signer"), st("signed"))));
    lines.push(Line::Text("Uma nota assinada não pode ser alterada. Correções ficam numa nova nota.".into()));
    // "2026-11-07 Nota de evolução 14h46.pdf": sorts by date in Finder/Explorer and reads well.
    let name = format!("{} Nota de evolução {}.pdf", st("day"), st("hhmm"));
    Ok((pid, name, export::pdf(&lines)))
}

/// After a note is signed: save it as PDF in the patient's `Notas` folder and index it as a document.
/// Safe to call again (one PDF per note).
pub async fn save_signed_note(s: &AppState, a: &Auth, note: Uuid) -> ApiResult<Uuid> {
    if let Some((id,)) = sqlx::query_as::<_, (Uuid,)>("SELECT id FROM documents WHERE note_id = $1").bind(note).fetch_optional(&s.pool).await? {
        return Ok(id);
    }
    let (pid, mut name, bytes) = note_pdf(s, note).await?;
    // Two notes signed in the same minute: keep both files.
    if tokio::fs::try_exists(s.archive.join(format!("{}/Notas/{name}", patient_dir(s, pid).await?))).await.unwrap_or(false) {
        name = format!("{} ({}).pdf", name.trim_end_matches(".pdf"), &note.simple().to_string()[..8]);
    }
    let rel = write(s, pid, "Notas", &name, &bytes).await?;
    let id = Uuid::new_v4();
    let sha = hex::encode(Sha256::digest(&bytes));
    sqlx::query("INSERT INTO documents (id, patient_id, kind, filename, mime, bytes, path, sha256, uploaded_by, note_id)
                 VALUES ($1,$2,'note',$3,'application/pdf',$4,$5,$6,$7,$8)")
        .bind(id).bind(pid).bind(&name).bind(bytes.len() as i64).bind(&rel).bind(sha).bind(a.staff.id).bind(note)
        .execute(&s.pool).await?;
    Ok(id)
}

// ───────── signed consents as PDF ─────────

/// Short Portuguese names for file names ("2026-11-07 Consentimento tratamento 14h46.pdf").
fn consent_file_pt(k: &str) -> &str {
    match k {
        "treatment" => "tratamento", "data" => "dados pessoais", "media" => "fotografias e vídeos",
        "marketing" => "comunicações", "guardian" => "representante legal", "invasive" => "técnicas invasivas", o => o,
    }
}

/// The PDF of a signed consent (`event` = "signed") or of its withdrawal ("revoked"), in the language it was signed in.
async fn consent_pdf(s: &AppState, consent: Uuid, event: &str, by: &str) -> ApiResult<(Uuid, String, Vec<u8>)> {
    let (v,): (Value,) = sqlx::query_as(&format!(
        "SELECT json_build_object(
            'patient_id', p.id, 'number', p.number, 'name', p.name, 'birth', to_char(p.birth_date, 'DD/MM/YYYY'),
            'nif', p.nif, 'clinic', (SELECT name FROM clinic LIMIT 1), 'clinic_info', (SELECT json_build_object('name', c.name, 'nif', c.nif, 'ers', c.ers_number, 'address', c.address, 'phone', c.phone, 'email', c.email, 'logo', encode(c.logo, 'base64')) FROM clinic c WHERE c.id = 1),
            'kind', c.kind, 'granted', c.granted, 'signer', c.signed_by_name, 'signature', c.signature_png,
            'title', t.title, 'body', t.body, 'version', t.version, 'lang', t.lang,
            'signed', to_char(c.signed_at AT TIME ZONE '{0}', 'DD/MM/YYYY HH24:MI'),
            'expires', to_char(c.expires_on, 'DD/MM/YYYY'),
            'revoked', to_char(c.revoked_at AT TIME ZONE '{0}', 'DD/MM/YYYY HH24:MI'),
            'day', to_char(CASE WHEN $2 = 'revoked' THEN c.revoked_at ELSE c.signed_at END AT TIME ZONE '{0}', 'YYYY-MM-DD'),
            'hhmm', to_char(CASE WHEN $2 = 'revoked' THEN c.revoked_at ELSE c.signed_at END AT TIME ZONE '{0}', 'HH24\"h\"MI'),
            'recorder', st.name)
         FROM consents c JOIN patients p ON p.id = c.patient_id JOIN consent_texts t ON t.id = c.text_id
         LEFT JOIN staff st ON st.id = c.recorded_by
         WHERE c.id = $1 AND p.anonymised_at IS NULL AND ($2 = 'signed' OR c.revoked_at IS NOT NULL)", tz()))
        .bind(consent).bind(event).fetch_optional(&s.pool).await?.ok_or(AppError::NotFound)?;
    let st = |k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let pid: Uuid = st("patient_id").parse().map_err(|_| AppError::NotFound)?;
    let en = st("lang") == "en";
    let t = |pt: &str, en_: &str| if en { en_.to_string() } else { pt.to_string() };
    let refusable = matches!(st("kind").as_str(), "data" | "media" | "marketing");
    let revoked = event == "revoked";

    let mut lines = vec![Line::Title(if revoked { t("Retirada de consentimento", "Withdrawal of consent") } else { st("title") })];
    lines.extend(crate::export::clinic_header(&v["clinic_info"], &st("clinic")));
    lines.push(Line::Gap);
    lines.push(Line::Row(t("Utente", "Patient"), format!("{} (n.º {})", st("name"), v["number"])));
    if !st("birth").is_empty() {
        lines.push(Line::Row(t("Data de nascimento", "Date of birth"), st("birth")));
    }
    if !st("nif").is_empty() {
        lines.push(Line::Row("NIF".into(), st("nif")));
    }
    if revoked {
        lines.push(Line::Row(t("Consentimento", "Consent"), format!("{} (v{})", st("title"), v["version"])));
        lines.push(Line::Row(t("Assinado a", "Signed on"), format!("{} {} {}", st("signed"), t("por", "by"), st("signer"))));
        lines.push(Line::Row(t("Retirado a", "Withdrawn on"), st("revoked")));
        lines.push(Line::Gap);
        for p in [
            t("O(a) utente retirou este consentimento. A partir desta data deixa de produzir efeitos; o que foi feito antes mantém-se válido.",
              "The patient withdrew this consent. From this date it no longer applies; what was done before remains valid."),
            t("A retirada não exige justificação nem afeta o direito a receber cuidados (Norma DGS 015/2013).",
              "Withdrawal needs no reason and doesn’t affect the right to care (DGS standard 015/2013)."),
        ] {
            lines.extend(export::wrap(&p, 10.0, 483.0).into_iter().map(Line::Text));
            lines.push(Line::Gap);
        }
        lines.push(Line::Row(t("Registado por", "Recorded by"), by.to_string()));
    } else {
        lines.push(Line::Row(t("Versão do texto", "Text version"), format!("v{}", v["version"])));
        lines.push(Line::Gap);
        for para in st("body").lines() {
            if para.trim().is_empty() {
                lines.push(Line::Gap);
                continue;
            }
            // "• item": hanging indent for list items.
            let (first, rest) = match para.strip_prefix("• ") { Some(r) => ("•  ", r), None => ("", para) };
            for (i, l) in export::wrap(rest, 10.0, if first.is_empty() { 483.0 } else { 470.0 }).into_iter().enumerate() {
                lines.push(Line::Text(if first.is_empty() { l } else if i == 0 { format!("{first}{l}") } else { format!("    {l}") }));
            }
        }
        lines.push(Line::Gap);
        if refusable {
            lines.push(Line::Heading(if v["granted"] == true { t("Decisão: autorizo", "Decision: I agree") } else { t("Decisão: não autorizo", "Decision: I do not agree") }));
        }
        lines.push(Line::Gap);
        lines.push(Line::Row(t("Assinado por", "Signed by"), st("signer")));
        lines.push(Line::Row(t("Data", "Date"), st("signed")));
        if !st("expires").is_empty() {
            lines.push(Line::Row(t("Rever até", "Review by"), st("expires")));
        }
        let png = st("signature").strip_prefix("data:image/png;base64,").and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok());
        match png.as_deref().and_then(export::picture_from_png) {
            Some(pic) => {
                lines.push(Line::Text(t("Assinatura:", "Signature:")));
                lines.push(Line::Image(pic, 200.0));
            }
            None => lines.push(Line::Text(t("Assinatura: (imagem não disponível)", "Signature: (image not available)"))),
        }
        if !st("recorder").is_empty() {
            lines.push(Line::Row(t("Recolhido por", "Collected by"), st("recorder")));
        }
    }
    let name = format!("{} {} {} {}.pdf", st("day"), if revoked { "Retirada consentimento" } else { "Consentimento" }, consent_file_pt(&st("kind")), st("hhmm"));
    Ok((pid, name, export::pdf(&lines)))
}

/// After a consent is signed (or withdrawn): save it as PDF in the patient's `Consentimentos` folder and index it.
/// Safe to call again (one PDF per consent and event).
pub async fn save_consent(s: &AppState, a: &Auth, consent: Uuid, event: &str) -> ApiResult<Uuid> {
    if let Some((id,)) = sqlx::query_as::<_, (Uuid,)>("SELECT id FROM documents WHERE consent_id = $1 AND consent_event = $2")
        .bind(consent).bind(event).fetch_optional(&s.pool).await? {
        return Ok(id);
    }
    let (pid, mut name, bytes) = consent_pdf(s, consent, event, &a.staff.name).await?;
    if tokio::fs::try_exists(s.archive.join(format!("{}/Consentimentos/{name}", patient_dir(s, pid).await?))).await.unwrap_or(false) {
        name = format!("{} ({}).pdf", name.trim_end_matches(".pdf"), &consent.simple().to_string()[..8]);
    }
    let rel = write(s, pid, "Consentimentos", &name, &bytes).await?;
    let id = Uuid::new_v4();
    let sha = hex::encode(Sha256::digest(&bytes));
    sqlx::query("INSERT INTO documents (id, patient_id, kind, filename, mime, bytes, path, sha256, uploaded_by, consent_id, consent_event)
                 VALUES ($1,$2,'consent',$3,'application/pdf',$4,$5,$6,$7,$8,$9)")
        .bind(id).bind(pid).bind(&name).bind(bytes.len() as i64).bind(&rel).bind(sha).bind(a.staff.id).bind(consent).bind(event)
        .execute(&s.pool).await?;
    Ok(id)
}
