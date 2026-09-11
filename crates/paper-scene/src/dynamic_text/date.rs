use super::{
    LocalTime, quoted,
    settings::{literal, setting},
};
use serde_json::Value;

pub struct Date {
    months: Vec<String>,
    days: Vec<String>,
    delimiter: String,
    show_day: bool,
    full_day: bool,
    newline: bool,
}

fn locale_data<'a>(script: &'a str, locale: &str) -> Option<&'a str> {
    for (index, _) in script.match_indices("lang.indexOf(") {
        let tail = &script[index + "lang.indexOf(".len()..];
        let language = literal(tail)?;
        if locale.to_lowercase().starts_with(language.as_str()?) {
            return tail.split_once("return")?.1.split_once('{')?.1.split('}').next();
        }
    }
    None
}

fn names(data: &str, field: &str, count: usize) -> Option<Vec<String>> {
    let tail = data.split_once(field)?.1.split_once('[')?.1.split(']').next()?;
    let names = quoted(tail);
    (names.len() == count).then_some(names)
}

impl Date {
    pub fn from_text(text: &Value) -> Option<Self> {
        let script = text.get("script")?.as_str()?;
        if ![
            "getLocaleData(currentLocale)",
            "date.getDate()",
            "date.getFullYear()",
            "formatSpaced(dayText)",
        ]
        .iter()
        .all(|part| script.contains(part))
        {
            return None;
        }
        let properties = text.get("scriptproperties");
        let locale = setting(script, None, "currentLocale")?;
        let data = locale_data(script, locale.as_str()?)?;
        let month_format = setting(script, properties, "monthFormat")?;
        let day_format = setting(script, properties, "dayFormat")?;
        let months = match month_format.as_str()? {
            "1" => (1..=12).map(|month| month.to_string()).collect(),
            "2" => names(data, "monthsAbbr", 12)?,
            "3" => names(data, "monthsFull", 12)?,
            _ => return None,
        };
        let full_day = day_format.as_str()? == "2";
        let days = names(data, if full_day { "daysFull" } else { "daysAbbr" }, 7)?;
        let delimiter = if setting(script, properties, "useDelimiter")?.as_bool()? {
            setting(script, properties, "addDelimiter")?.as_str()?.to_owned()
        } else {
            " ".to_owned()
        };
        Some(Self {
            months,
            days,
            delimiter,
            full_day,
            show_day: setting(script, properties, "showDay")?.as_bool()?,
            newline: setting(script, properties, "alignVertical")?.as_bool()?,
        })
    }

    pub fn value(&self, now: LocalTime) -> String {
        if self.show_day {
            let day = self.days[now.weekday.rem_euclid(7) as usize].to_uppercase();
            let spaced = day.chars().map(|ch| ch.to_string()).collect::<Vec<_>>().join(" ");
            let mut value = if self.full_day { format!("| {spaced} |") } else { spaced };
            if self.newline {
                value.push('\n');
            }
            return value;
        }
        format!(
            "{}{}{}{}{}",
            now.day,
            self.delimiter,
            self.months[(now.month - 1).clamp(0, 11) as usize],
            self.delimiter,
            now.year
        )
    }
}
