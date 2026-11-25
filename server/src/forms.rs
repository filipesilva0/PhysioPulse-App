//! Question-by-question forms for the free outcome questionnaires (audit phase C): NPRS, SPADI, LEFS, PSFS and KOOS.
//! The app sends each answer; the server checks every answer and works out the score itself, so the saved total
//! can never disagree with the answers. ODI, NDI and QuickDASH are licensed instruments: they keep the typed total.
//!
//! Stored answers: `{"form": 1, "items": {"p1": 7, …}, "sub": {"pain": 70.0, …}, "activities": ["…"]}`
//! (`activities` only for PSFS). The item ids match `src/lib/forms.ts` in the app.

use crate::error::{ApiResult, AppError};
use serde_json::{json, Map, Value};

/// A scored form: the total, its maximum, whether higher is worse, and the tidied answers to store.
pub struct Scored { pub score: f64, pub max: f64, pub higher_is_worse: bool, pub answers: Value }

/// Subscales: (id, item ids, how many may be left blank).
struct Sub { id: &'static str, items: &'static [&'static str], may_skip: usize }

const SPADI: &[Sub] = &[
    Sub { id: "pain", items: &["p1", "p2", "p3", "p4", "p5"], may_skip: 2 },
    Sub { id: "disability", items: &["d1", "d2", "d3", "d4", "d5", "d6", "d7", "d8"], may_skip: 2 },
];
const LEFS: &[&str] = &["i1", "i2", "i3", "i4", "i5", "i6", "i7", "i8", "i9", "i10", "i11", "i12", "i13", "i14", "i15", "i16", "i17", "i18", "i19", "i20"];
const KOOS: &[Sub] = &[
    Sub { id: "symptoms", items: &["s1", "s2", "s3", "s4", "s5", "s6", "s7"], may_skip: 2 },
    Sub { id: "pain", items: &["p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8", "p9"], may_skip: 2 },
    Sub { id: "adl", items: &["a1", "a2", "a3", "a4", "a5", "a6", "a7", "a8", "a9", "a10", "a11", "a12", "a13", "a14", "a15", "a16", "a17"], may_skip: 2 },
    Sub { id: "sport", items: &["sp1", "sp2", "sp3", "sp4", "sp5"], may_skip: 2 },
    Sub { id: "qol", items: &["q1", "q2", "q3", "q4"], may_skip: 2 },
];

/// The codes that have a question-by-question form.
pub fn has_form(code: &str) -> bool {
    matches!(code, "nprs" | "spadi" | "lefs" | "psfs" | "koos")
}

fn r1(x: f64) -> f64 { (x * 10.0).round() / 10.0 }

/// Reads `items` as whole numbers in 0..=max, only from `allowed`. Missing items are simply absent.
fn items(a: &Value, allowed: &[&str], max: i64) -> ApiResult<Map<String, Value>> {
    let obj = a.get("items").and_then(Value::as_object).ok_or(AppError::BadRequest("Answer the questions first."))?;
    let mut out = Map::new();
    for (k, v) in obj {
        if !allowed.contains(&k.as_str()) {
            return Err(AppError::BadRequest("This form has an answer it doesn’t know. Refresh and try again."));
        }
        if v.is_null() { continue; }
        match v.as_i64() {
            Some(n) if (0..=max).contains(&n) && v.as_f64() == Some(n as f64) => { out.insert(k.clone(), json!(n)); }
            _ => return Err(AppError::BadRequest("An answer is out of range. Check the form.")),
        }
    }
    Ok(out)
}

fn sum(it: &Map<String, Value>, ids: &[&str]) -> (i64, usize) {
    ids.iter().filter_map(|i| it.get(*i).and_then(Value::as_i64)).fold((0, 0), |(s, n), v| (s + v, n + 1))
}

/// Scores a form, or `None` if this code has no form or the request carries no item answers (typed total).
pub fn score(code: &str, answers: Option<&Value>) -> ApiResult<Option<Scored>> {
    let Some(a) = answers.filter(|a| a.get("items").is_some()) else { return Ok(None) };
    if !has_form(code) { return Ok(None); }
    let done = |score: f64, max: f64, hiw: bool, mut stored: Value| {
        stored["form"] = json!(1);
        Ok(Some(Scored { score: r1(score), max, higher_is_worse: hiw, answers: stored }))
    };
    match code {
        "nprs" => {
            let it = items(a, &["now"], 10)?;
            let v = it.get("now").and_then(Value::as_i64).ok_or(AppError::BadRequest("Answer every question."))?;
            done(v as f64, 10.0, true, json!({ "items": it }))
        }
        "lefs" => {
            let it = items(a, LEFS, 4)?;
            let (s, n) = sum(&it, LEFS);
            if n < LEFS.len() { return Err(AppError::BadRequest("Answer every question.")); }
            done(s as f64, 80.0, false, json!({ "items": it }))
        }
        "spadi" => {
            let all: Vec<&str> = SPADI.iter().flat_map(|s| s.items.iter().copied()).collect();
            let it = items(a, &all, 10)?;
            let (st, nt) = sum(&it, &all);
            if all.len() - nt > 2 { return Err(AppError::BadRequest("Too many questions left blank to work out a score.")); }
            let mut sub = Map::new();
            for s in SPADI {
                let (x, n) = sum(&it, s.items);
                if n == 0 || s.items.len() - n > s.may_skip { return Err(AppError::BadRequest("Too many questions left blank to work out a score.")); }
                sub.insert(s.id.into(), json!(r1(x as f64 / (n as f64 * 10.0) * 100.0)));
            }
            done(st as f64 / (nt as f64 * 10.0) * 100.0, 100.0, true, json!({ "items": it, "sub": sub }))
        }
        "koos" => {
            let all: Vec<&str> = KOOS.iter().flat_map(|s| s.items.iter().copied()).collect();
            let it = items(a, &all, 4)?;
            let mut sub = Map::new();
            let mut total = 0.0;
            for s in KOOS {
                let (x, n) = sum(&it, s.items);
                if n == 0 || s.items.len() - n > s.may_skip { return Err(AppError::BadRequest("Too many questions left blank to work out a score.")); }
                // 0 = extreme problems, 100 = no problems.
                let v = 100.0 - (x as f64 / n as f64) * 25.0;
                total += v;
                sub.insert(s.id.into(), json!(r1(v)));
            }
            // The card shows the mean of the five subscales; each subscale is kept and shown too.
            done(total / KOOS.len() as f64, 100.0, false, json!({ "items": it, "sub": sub }))
        }
        "psfs" => {
            let acts = a.get("activities").and_then(Value::as_array).ok_or(AppError::BadRequest("Name at least one activity."))?;
            let names: Vec<String> = acts.iter().map(|x| x.as_str().unwrap_or("").trim().chars().take(80).collect::<String>()).collect();
            if names.is_empty() || names.len() > 5 || names.iter().any(String::is_empty) {
                return Err(AppError::BadRequest("Name at least one activity."));
            }
            let ids = ["a1", "a2", "a3", "a4", "a5"];
            let it = items(a, &ids[..names.len()], 10)?;
            let (s, n) = sum(&it, &ids[..names.len()]);
            if n < names.len() { return Err(AppError::BadRequest("Answer every question.")); }
            done(s as f64 / n as f64, 10.0, false, json!({ "items": it, "activities": names }))
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn it(pairs: &[(&str, i64)]) -> Value {
        json!({ "items": pairs.iter().map(|(k, v)| (k.to_string(), json!(v))).collect::<Map<_, _>>() })
    }
    #[test]
    fn spadi_percent_and_subscales() {
        let mut p: Vec<(&str, i64)> = ["p1", "p2", "p3", "p4", "p5"].iter().map(|k| (*k, 6)).collect();
        p.extend(["d1", "d2", "d3", "d4", "d5", "d6", "d7", "d8"].iter().map(|k| (*k, 3)));
        let s = score("spadi", Some(&it(&p))).unwrap().unwrap();
        assert_eq!(s.score, 41.5); // (30 + 24) / 130 × 100
        assert_eq!(s.answers["sub"]["pain"], json!(60.0));
        assert_eq!(s.answers["sub"]["disability"], json!(30.0));
        assert!(s.higher_is_worse);
        // One left blank: the maximum shrinks.
        let s = score("spadi", Some(&it(&p[1..]))).unwrap().unwrap();
        assert_eq!(s.score, 40.0); // (24 + 24) / 120 × 100
        // Three blank: no score.
        assert!(score("spadi", Some(&it(&p[3..]))).is_err());
    }
    #[test]
    fn koos_subscales_and_mean() {
        let all: Vec<(&str, i64)> = KOOS.iter().flat_map(|s| s.items.iter().map(|k| (*k, 1))).collect();
        let s = score("koos", Some(&it(&all))).unwrap().unwrap();
        assert_eq!(s.score, 75.0);
        assert_eq!(s.answers["sub"]["qol"], json!(75.0));
        assert!(!s.higher_is_worse);
        assert!(score("koos", Some(&it(&[("s1", 5)]))).is_err()); // out of range
    }
    #[test]
    fn lefs_needs_every_answer() {
        let all: Vec<(&str, i64)> = LEFS.iter().map(|k| (*k, 3)).collect();
        assert_eq!(score("lefs", Some(&it(&all))).unwrap().unwrap().score, 60.0);
        assert!(score("lefs", Some(&it(&all[1..]))).is_err());
    }
    #[test]
    fn psfs_mean_with_names() {
        let a = json!({ "items": { "a1": 4, "a2": 7 }, "activities": ["Subir escadas", "Correr"] });
        let s = score("psfs", Some(&a)).unwrap().unwrap();
        assert_eq!(s.score, 5.5);
        assert!(score("psfs", Some(&json!({ "items": { "a1": 4 }, "activities": [""] }))).is_err());
    }
    #[test]
    fn typed_totals_and_licensed_tests_are_left_alone() {
        assert!(score("odi", Some(&it(&[("i1", 2)]))).unwrap().is_none());
        assert!(score("spadi", Some(&json!({}))).unwrap().is_none());
        assert!(score("nprs", None).unwrap().is_none());
        assert!(score("nprs", Some(&it(&[("x", 2)]))).is_err());
        assert!(score("nprs", Some(&json!({ "items": { "now": 2.5 } }))).is_err());
    }
}
