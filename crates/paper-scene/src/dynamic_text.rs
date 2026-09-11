pub mod clock;
pub mod date;
mod settings;

use serde_json::Value;
use std::fmt::Write;

const WEEKDAYS: [&str; 7] =
    ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalTime {
    pub hour: i32,
    pub minute: i32,
    pub second: i32,
    pub day: i32,
    pub month: i32,
    pub year: i32,
    pub weekday: i32,
}

#[must_use]
pub fn local_now() -> Option<LocalTime> {
    let mut spec = libc::timespec { tv_sec: 0, tv_nsec: 0 };
    if unsafe { libc::clock_gettime(libc::CLOCK_REALTIME, &raw mut spec) } != 0 {
        return None;
    }
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&raw const spec.tv_sec, &raw mut tm) }.is_null() {
        return None;
    }
    Some(LocalTime {
        hour: tm.tm_hour,
        minute: tm.tm_min,
        second: tm.tm_sec,
        day: tm.tm_mday,
        month: tm.tm_mon + 1,
        year: tm.tm_year + 1900,
        weekday: tm.tm_wday,
    })
}

#[must_use]
pub fn weekday_of(year: i32, month: i32, day: i32) -> i32 {
    let (year, month) = if month < 3 { (year - 1, month + 12) } else { (year, month) };
    let century = year / 100;
    let decade = year % 100;
    let zeller = (day + (13 * (month + 1)) / 5 + decade + decade / 4 + century / 4 + 5 * century)
        .rem_euclid(7);
    (zeller + 6) % 7
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Token {
    Digits(usize),
    Word,
    Literal,
}

fn tokenize(text: &str) -> Vec<(Token, String)> {
    let mut out: Vec<(Token, String)> = Vec::new();
    for ch in text.chars() {
        let kind = if ch.is_ascii_digit() {
            Token::Digits(1)
        } else if ch.is_alphabetic() {
            Token::Word
        } else {
            Token::Literal
        };
        match out.last_mut() {
            Some((last, run))
                if matches!(
                    (*last, kind),
                    (Token::Digits(_), Token::Digits(_)) | (Token::Word, Token::Word)
                ) =>
            {
                run.push(ch);
                if let Token::Digits(count) = last {
                    *count += 1;
                }
            }
            _ => out.push((kind, ch.to_string())),
        }
    }
    out
}

fn match_name(word: &str, names: &[&str]) -> Option<usize> {
    let lower = word.to_ascii_lowercase();
    names.iter().position(|name| {
        let name = name.to_ascii_lowercase();
        name == lower || (lower.len() == 3 && name.starts_with(&lower))
    })
}

fn cased(sample: &str, replacement: &str, abbreviate: bool) -> String {
    let text = if abbreviate {
        replacement.chars().take(3).collect::<String>()
    } else {
        replacement.to_string()
    };
    if sample.chars().all(|ch| ch.is_uppercase() || !ch.is_alphabetic()) {
        return text.to_uppercase();
    }
    if sample.chars().all(|ch| ch.is_lowercase() || !ch.is_alphabetic()) {
        return text.to_lowercase();
    }
    text
}

struct Slots {
    hour: Option<usize>,
    minute: Option<usize>,
    second: Option<usize>,
    year: Option<usize>,
    pairs: Vec<usize>,
    meridiem: Option<usize>,
}

fn locate(tokens: &[(Token, String)]) -> Slots {
    let mut slots = Slots {
        hour: None,
        minute: None,
        second: None,
        year: None,
        pairs: Vec::new(),
        meridiem: None,
    };
    let mut index = 0usize;
    while index < tokens.len() {
        let (kind, text) = &tokens[index];
        match kind {
            Token::Digits(4) if slots.year.is_none() => slots.year = Some(index),
            Token::Digits(count @ (1 | 2)) => {
                let separator = tokens.get(index + 1);
                let follows = tokens.get(index + 2);
                let is_clock = slots.hour.is_none()
                    && matches!(separator, Some((Token::Literal, sep)) if sep == ":" || sep == ".")
                    && matches!(follows, Some((Token::Digits(2), _)))
                    && text.parse::<i32>().is_ok_and(|value| value <= 23);
                if is_clock && *count <= 2 {
                    slots.hour = Some(index);
                    slots.minute = Some(index + 2);
                    if matches!(tokens.get(index + 3), Some((Token::Literal, sep)) if sep == ":" || sep == ".")
                        && matches!(tokens.get(index + 4), Some((Token::Digits(2), _)))
                    {
                        slots.second = Some(index + 4);
                    }
                    index = slots.second.unwrap_or(index + 2);
                } else if *count == 2 {
                    slots.pairs.push(index);
                }
            }
            Token::Word if matches!(text.to_ascii_lowercase().as_str(), "am" | "pm") => {
                slots.meridiem = Some(index);
            }
            _ => {}
        }
        index += 1;
    }
    slots
}

fn date_slots(
    tokens: &[(Token, String)],
    slots: &Slots,
    weekday_slot: Option<usize>,
    month_slot: Option<usize>,
) -> (Option<usize>, Option<usize>) {
    if slots.pairs.len() < 2 {
        let lone =
            slots.pairs.first().copied().filter(|_| {
                month_slot.is_some() || (weekday_slot.is_some() && slots.year.is_some())
            });
        return (lone, None);
    }
    match resolve_pair_order(tokens, slots, weekday_slot) {
        Some((day, month)) => (Some(day), Some(month)),
        None => (None, None),
    }
}

fn resolve_pair_order(
    tokens: &[(Token, String)],
    slots: &Slots,
    weekday_slot: Option<usize>,
) -> Option<(usize, usize)> {
    let (&first, &second) = (slots.pairs.first()?, slots.pairs.get(1)?);
    let a: i32 = tokens[first].1.parse().ok()?;
    let b: i32 = tokens[second].1.parse().ok()?;
    let year: i32 = slots.year.and_then(|at| tokens[at].1.parse().ok()).unwrap_or(2000);
    let named = weekday_slot.and_then(|at| match_name(&tokens[at].1, &WEEKDAYS));
    let month_first = a <= 12 && b <= 31;
    let day_first = b <= 12 && a <= 31;
    if let Some(named) = named {
        if month_first && weekday_of(year, a, b) == named as i32 {
            return Some((second, first));
        }
        if day_first && weekday_of(year, b, a) == named as i32 {
            return Some((first, second));
        }
    }
    if slots.year.is_some_and(|at| at < first) && month_first {
        return Some((second, first));
    }
    if day_first {
        return Some((first, second));
    }
    month_first.then_some((second, first))
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Cadence {
    Second,
    Minute,
    Day,
}

#[must_use]
pub fn cadence(placeholder: &str) -> Cadence {
    match placeholder.trim().to_ascii_lowercase().as_str() {
        "<clock>" => return Cadence::Minute,
        "<date>" => return Cadence::Day,
        _ => {}
    }
    let slots = locate(&tokenize(placeholder));
    if slots.second.is_some() {
        Cadence::Second
    } else if slots.hour.is_some() {
        Cadence::Minute
    } else {
        Cadence::Day
    }
}

#[must_use]
pub fn seconds_until_next(cadence: Cadence, now: LocalTime) -> f32 {
    match cadence {
        Cadence::Second => 1.0,
        Cadence::Minute => (60 - now.second).max(1) as f32,
        Cadence::Day => (86_400 - (now.hour * 3600 + now.minute * 60 + now.second)).max(1) as f32,
    }
}

fn keyword(placeholder: &str, now: LocalTime) -> Option<String> {
    match placeholder.trim().to_ascii_lowercase().as_str() {
        "<clock>" => Some(format!("{:02}:{:02}", now.hour, now.minute)),
        "<date>" => Some(format!("{:04}-{:02}-{:02}", now.year, now.month, now.day)),
        _ => None,
    }
}

fn stacked(placeholder: &str, now: LocalTime) -> Option<String> {
    if !placeholder.contains('\n') || placeholder.chars().any(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let letters: String = placeholder.chars().filter(|ch| ch.is_alphabetic()).collect();
    if letters.is_empty() {
        return None;
    }
    let live = if match_name(&letters, &WEEKDAYS).is_some() {
        WEEKDAYS[now.weekday.rem_euclid(7) as usize]
    } else if match_name(&letters, &MONTHS).is_some() {
        MONTHS[(now.month - 1).clamp(0, 11) as usize]
    } else {
        return None;
    };
    let live = cased(&letters, live, letters.chars().count() <= 3);
    if live.chars().count() != letters.chars().count() {
        return None;
    }
    let mut replacement = live.chars();
    Some(
        placeholder
            .chars()
            .map(|ch| if ch.is_alphabetic() { replacement.next().unwrap_or(ch) } else { ch })
            .collect(),
    )
}

fn plain(text: &str) -> String {
    text.chars().filter(char::is_ascii_alphanumeric).flat_map(char::to_lowercase).collect()
}

fn quoted(block: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = block;
    while let Some(open) = rest.find(['\'', '"']) {
        let quote = rest.as_bytes()[open] as char;
        let after = &rest[open + 1..];
        let Some(close) = after.find(quote) else {
            break;
        };
        out.push(after[..close].to_string());
        rest = &after[close + 1..];
    }
    out
}

#[must_use]
pub fn weekday_table(script: &str) -> Option<Vec<String>> {
    let mut rest = script;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let close = after.find(']').unwrap_or(after.len());
        let items = quoted(&after[..close]);
        rest = &after[close.min(after.len())..];
        if items.len() != 7 {
            continue;
        }
        let shaped: Vec<String> = items.iter().map(|item| plain(item)).collect();
        let canonical: Vec<String> =
            WEEKDAYS.iter().map(|name| name.to_ascii_lowercase()).collect();
        if shaped == canonical {
            return Some(items);
        }
        if shaped == [&canonical[1..], &canonical[..1]].concat() {
            let mut rotated = items;
            rotated.rotate_right(1);
            return Some(rotated);
        }
    }
    None
}

#[must_use]
pub fn substitute(placeholder: &str, now: LocalTime) -> Option<String> {
    substitute_with(placeholder, now, None)
}

#[must_use]
pub fn substitute_with(
    placeholder: &str,
    now: LocalTime,
    weekday: Option<&[String]>,
) -> Option<String> {
    let out = substitute_plain(placeholder, now)?;
    let Some(table) = weekday else {
        return Some(out);
    };
    let today = WEEKDAYS[now.weekday.rem_euclid(7) as usize];
    if plain(&out) == plain(today) {
        return table.get(now.weekday.rem_euclid(7) as usize).cloned().or(Some(out));
    }
    Some(out)
}

fn substitute_plain(placeholder: &str, now: LocalTime) -> Option<String> {
    if let Some(text) = keyword(placeholder, now) {
        return Some(text);
    }
    if let Some(text) = stacked(placeholder, now) {
        return Some(text);
    }
    let tokens = tokenize(placeholder);
    let slots = locate(&tokens);
    let weekday_slot = tokens.iter().position(|(kind, text)| {
        *kind == Token::Word
            && (match_name(text, &WEEKDAYS).is_some() || text.eq_ignore_ascii_case("day"))
    });
    let month_slot = tokens
        .iter()
        .position(|(kind, text)| *kind == Token::Word && match_name(text, &MONTHS).is_some());
    let (day_slot, month_number_slot) = date_slots(&tokens, &slots, weekday_slot, month_slot);
    if slots.hour.is_none()
        && slots.year.is_none()
        && weekday_slot.is_none()
        && month_slot.is_none()
    {
        return None;
    }
    let hour12 = |hour: i32| {
        let wrapped = hour % 12;
        if wrapped == 0 { 12 } else { wrapped }
    };
    let mut out = String::with_capacity(placeholder.len() + 8);
    for (index, (_, text)) in tokens.iter().enumerate() {
        let width = text.chars().count();
        let number = |value: i32| {
            if width >= 2 { format!("{value:0width$}") } else { value.to_string() }
        };
        if Some(index) == slots.hour {
            let hour = if slots.meridiem.is_some() { hour12(now.hour) } else { now.hour };
            out.push_str(&number(hour));
        } else if Some(index) == slots.minute {
            out.push_str(&number(now.minute));
        } else if Some(index) == slots.second {
            out.push_str(&number(now.second));
        } else if Some(index) == slots.year {
            let _ = write!(out, "{:04}", now.year);
        } else if Some(index) == slots.meridiem {
            out.push_str(&cased(text, if now.hour < 12 { "am" } else { "pm" }, false));
        } else if Some(index) == weekday_slot {
            let name = WEEKDAYS[now.weekday.rem_euclid(7) as usize];
            let abbreviate = width <= 3 && !text.eq_ignore_ascii_case("day");
            out.push_str(&cased(text, name, abbreviate));
        } else if Some(index) == month_slot {
            let name = MONTHS[(now.month - 1).clamp(0, 11) as usize];
            out.push_str(&cased(text, name, width <= 3));
        } else if day_slot == Some(index) {
            out.push_str(&number(now.day));
        } else if month_number_slot == Some(index) {
            out.push_str(&number(now.month));
        } else {
            out.push_str(text);
        }
    }
    Some(out)
}

const MEDIA_HOOKS: [&str; 5] = [
    "mediaPropertiesChanged",
    "mediaPlaybackChanged",
    "mediaThumbnailChanged",
    "mediaTimelineChanged",
    "MediaStatus",
];

#[must_use]
pub fn media_bound(script: &str) -> bool {
    MEDIA_HOOKS.iter().any(|hook| script.contains(hook))
}

#[must_use]
pub fn media_scripted(value: Option<&Value>) -> bool {
    let Some(Value::Object(map)) = value else {
        return false;
    };
    map.get("script").and_then(Value::as_str).is_some_and(media_bound)
}

#[must_use]
pub fn resolve(text: Option<&Value>, now: LocalTime) -> Option<String> {
    let Value::Object(map) = text? else {
        return None;
    };
    let script = map.get("script")?.as_str().unwrap_or_default();
    if media_bound(script) {
        return Some(String::new());
    }
    if let Some(clock) = clock::Clock::from_text(text?) {
        return Some(clock.value(now));
    }
    if let Some(date) = date::Date::from_text(text?) {
        return Some(date.value(now));
    }
    let placeholder = crate::text::text_value(map.get("value"))?;
    let table = weekday_table(script);
    let live = substitute_with(&placeholder, now, table.as_deref())?;
    (live != placeholder).then_some(live)
}

#[cfg(test)]
#[path = "dynamic_text_tests.rs"]
mod tests;
