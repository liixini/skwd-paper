use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ValidationError;

pub const MAX_WIDGETS: usize = 16;
pub const MAX_OUTLINES: usize = 16;
pub const WEEKDAYS: usize = 7;
pub const MONTHS: usize = 12;
pub const WIDGET_HEIGHTS: std::ops::RangeInclusive<f32> = 0.05..=1.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ClockStyle {
    Aperture,
    ApertureRing,
    Greeter,
    ControlCenter,
    DayDial,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "style", rename_all = "kebab-case")]
pub enum Face {
    Clock(ClockStyle),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Depth {
    #[default]
    Behind,
    Front,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Pin {
    pub anchor: [f32; 2],
    pub offset: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Widget {
    pub face: Face,
    #[serde(default)]
    pub depth: Depth,
    pub pin: Pin,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Palette {
    pub text: [u8; 3],
    pub accent: [u8; 3],
    pub shade: [u8; 3],
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Labels {
    pub days: [String; WEEKDAYS],
    pub day_names: [String; WEEKDAYS],
    pub months: [String; MONTHS],
    pub month_names: [String; MONTHS],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WidgetScene {
    pub widgets: Vec<Widget>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outlines: Vec<PathBuf>,
    pub palette: Palette,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font: Option<PathBuf>,
    pub labels: Labels,
}

impl Widget {
    pub fn shown_on(&self, output: &str) -> bool {
        self.output.as_ref().is_none_or(|name| name == output)
    }

    pub fn validate(&self) -> Result<(), ValidationError> {
        let finite = self.pin.offset.iter().all(|value| value.is_finite());
        let inside = self.pin.anchor.iter().all(|value| (0.0..=1.0).contains(value));
        if !inside || !finite {
            return Err(ValidationError::InvalidWidgetPin);
        }
        if self.height.is_some_and(|height| !WIDGET_HEIGHTS.contains(&height)) {
            return Err(ValidationError::InvalidWidgetHeight);
        }
        Ok(())
    }
}

impl WidgetScene {
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.widgets.len() > MAX_WIDGETS {
            return Err(ValidationError::TooManyWidgets(self.widgets.len()));
        }
        self.widgets.iter().try_for_each(Widget::validate)?;
        if self.outlines.len() > MAX_OUTLINES {
            return Err(ValidationError::TooManyOutlines(self.outlines.len()));
        }
        let relative = self.font.as_ref().is_some_and(|path| !path.is_absolute());
        if relative || self.outlines.iter().any(|path| !path.is_absolute()) {
            return Err(ValidationError::RelativeWidgetPath);
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "widget_tests.rs"]
mod tests;
