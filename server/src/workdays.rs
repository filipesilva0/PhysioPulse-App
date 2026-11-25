//! Working days in Portugal: Monday to Friday, without the national public holidays
//! (Código do Trabalho, art. 234.º). Carnival and the municipal holiday are optional, so they count as
//! working days here; that errs on the early side for a deadline.

use time::{Date, Duration, Month, Weekday};

/// Easter Sunday (anonymous Gregorian algorithm).
fn easter(year: i32) -> Date {
    let a = year % 19;
    let b = year / 100;
    let c = year % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    Date::from_calendar_date(year, Month::try_from(month as u8).unwrap_or(Month::April), day as u8).unwrap_or(Date::MIN)
}

pub fn is_holiday(d: Date) -> bool {
    let fixed = [(1, 1), (4, 25), (5, 1), (6, 10), (8, 15), (10, 5), (11, 1), (12, 1), (12, 8), (12, 25)];
    if fixed.contains(&(d.month() as u8, d.day())) {
        return true;
    }
    let e = easter(d.year());
    // Good Friday, Easter Sunday, Corpus Christi.
    d == e - Duration::days(2) || d == e || d == e + Duration::days(60)
}

pub fn is_working_day(d: Date) -> bool {
    !matches!(d.weekday(), Weekday::Saturday | Weekday::Sunday) && !is_holiday(d)
}

/// The day `n` working days after `from` (the day of receipt itself doesn't count).
pub fn add_working_days(from: Date, n: u32) -> Date {
    let mut d = from;
    let mut left = n;
    while left > 0 {
        d = d.next_day().unwrap_or(d);
        if is_working_day(d) {
            left -= 1;
        }
    }
    d
}

/// Working days from `from` (exclusive) to `to` (inclusive); negative when `to` is before `from`.
pub fn working_days_between(from: Date, to: Date) -> i64 {
    let (a, b, sign) = if to >= from { (from, to, 1) } else { (to, from, -1) };
    let mut d = a;
    let mut n = 0;
    while d < b {
        d = d.next_day().unwrap_or(b);
        if is_working_day(d) {
            n += 1;
        }
    }
    n * sign
}

pub fn parse(s: &str) -> Option<Date> {
    let s = s.trim();
    if s.len() != 10 || &s[4..5] != "-" || &s[7..8] != "-" {
        return None;
    }
    let y: i32 = s[..4].parse().ok()?;
    let m: u8 = s[5..7].parse().ok()?;
    let d: u8 = s[8..10].parse().ok()?;
    if !(2000..=2100).contains(&y) {
        return None;
    }
    Date::from_calendar_date(y, Month::try_from(m).ok()?, d).ok()
}

pub fn fmt(d: Date) -> String {
    format!("{:04}-{:02}-{:02}", d.year(), d.month() as u8, d.day())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn d(s: &str) -> Date { parse(s).unwrap() }

    #[test]
    fn easter_dates() {
        assert_eq!(fmt(easter(2026)), "2026-04-05");
        assert_eq!(fmt(easter(2027)), "2027-03-28");
        assert_eq!(fmt(easter(2025)), "2025-04-20");
    }

    #[test]
    fn holidays() {
        assert!(is_holiday(d("2026-04-03"))); // Good Friday
        assert!(is_holiday(d("2026-06-04"))); // Corpus Christi
        assert!(is_holiday(d("2026-12-08")));
        assert!(!is_holiday(d("2026-02-17"))); // Carnival is optional
    }

    #[test]
    fn ten_working_days() {
        // Received Friday 6 Nov 2026: 9–13 and 16–20 Nov.
        assert_eq!(fmt(add_working_days(d("2026-11-06"), 10)), "2026-11-20");
        // Received Monday 23 Nov 2026: 1 and 8 Dec are holidays.
        assert_eq!(fmt(add_working_days(d("2026-11-23"), 10)), "2026-12-09");
        // Received on a Saturday: the count starts on Monday.
        assert_eq!(fmt(add_working_days(d("2026-11-07"), 1)), "2026-11-09");
        assert_eq!(working_days_between(d("2026-11-06"), d("2026-11-20")), 10);
        assert_eq!(working_days_between(d("2026-11-20"), d("2026-11-18")), -2);
        assert!(parse("2026-02-30").is_none());
    }
}
