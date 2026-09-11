use super::{Cadence, LocalTime, settings::setting};
use serde_json::Value;

pub struct Clock {
    delimiter: String,
    seconds: bool,
    hours24: bool,
    suffix: bool,
}

impl Clock {
    pub fn from_text(text: &Value) -> Option<Self> {
        let script = text.get("script")?.as_str()?;
        if !["getHours()", "getMinutes()", "hours", "minutes", "delimiter"]
            .iter()
            .all(|part| script.contains(part))
        {
            return None;
        }
        let properties = text.get("scriptproperties");
        let delimiter = setting(script, properties, "delimiter")?.as_str()?.to_owned();
        let seconds = setting(script, properties, "showSeconds")?.as_bool()?;
        let hours24 = setting(script, properties, "use24hFormat")?.as_bool()?;
        Some(Self { delimiter, seconds, hours24, suffix: script.contains("+ suffix") })
    }

    pub fn cadence(&self) -> Cadence {
        if self.seconds { Cadence::Second } else { Cadence::Minute }
    }

    pub fn value(&self, now: LocalTime) -> String {
        let hour = if self.hours24 { now.hour } else { (now.hour + 11) % 12 + 1 };
        let mut value = if self.suffix && !self.hours24 {
            format!("{hour}{}{:02}", self.delimiter, now.minute)
        } else {
            format!("{hour:02}{}{:02}", self.delimiter, now.minute)
        };
        if self.seconds {
            use std::fmt::Write;
            let _ = write!(value, "{}{:02}", self.delimiter, now.second);
        }
        if self.suffix && !self.hours24 {
            value.push_str(if now.hour < 12 { " AM" } else { " PM" });
        }
        value
    }
}
