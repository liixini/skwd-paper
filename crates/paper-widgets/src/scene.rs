use paper_control::{Face, Labels};

use crate::canvas::Canvas;
use crate::lettering::Lettering;
use crate::place::Place;
use crate::tones::Tones;

const MINUTES_PER_DAY: f32 = 1440.0;
const MINUTES_PER_HOUR: u16 = 60;
const WEEK: i32 = 7;
const DAYS_BEFORE_MONDAY: i32 = 6;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moment {
    pub hour: u8,
    pub minute: u8,
    pub weekday: usize,
    pub day: u8,
    pub month: usize,
}

impl Moment {
    pub fn share(self) -> f32 {
        f32::from(u16::from(self.hour) * MINUTES_PER_HOUR + u16::from(self.minute))
            / MINUTES_PER_DAY
    }

    pub(crate) fn hours(self) -> String {
        format!("{:02}", self.hour)
    }

    pub(crate) fn minutes(self) -> String {
        format!("{:02}", self.minute)
    }
}

pub struct Scene<'a> {
    pub lettering: &'a Lettering,
    pub tones: Tones,
    pub moment: Moment,
    pub labels: &'a Labels,
}

impl Scene<'_> {
    pub fn paint(&self, face: Face, place: Place, cut: Option<&[u8]>) -> Canvas {
        let mut canvas = Canvas::window(place.size, place.ink, place.shift);
        crate::draw(face, &mut canvas, self);
        if let Some(cut) = cut {
            canvas.occlude(cut);
        }
        canvas
    }

    pub(crate) fn short_date(&self) -> String {
        let moment = self.moment;
        format!(
            "{} {} {}",
            self.labels.days[moment.weekday], moment.day, self.labels.months[moment.month]
        )
    }

    pub(crate) fn long_date(&self) -> String {
        let moment = self.moment;
        format!(
            "{} {} {}",
            self.labels.day_names[moment.weekday],
            moment.day,
            self.labels.month_names[moment.month]
        )
    }
}

impl Moment {
    pub fn civil(hour: i32, minute: i32, weekday_from_sunday: i32, day: i32, month: i32) -> Self {
        Self {
            hour: hour as u8,
            minute: minute as u8,
            weekday: ((weekday_from_sunday + DAYS_BEFORE_MONDAY) % WEEK) as usize,
            day: day as u8,
            month: (month - 1) as usize,
        }
    }
}

#[cfg(test)]
#[path = "scene_tests.rs"]
mod tests;
