use super::{Particle, SIZE_TO_NATIVE};
use serde::Deserialize;
use serde_json::Value;

pub(super) const INHERIT_INITIAL: &str = "inheritinitialvaluefromevent";
pub(super) const INHERIT_OPERATOR: &str = "inheritvaluefromevent";
const INPUT: &str = "input";

#[derive(Clone, Copy)]
pub(super) struct EventValues {
    color: [f32; 3],
    opacity: f32,
    size: f32,
    velocity: [f32; 3],
}

impl From<&Particle> for EventValues {
    fn from(particle: &Particle) -> Self {
        Self {
            color: particle.color,
            opacity: particle.alpha,
            size: particle.size,
            velocity: particle.vel,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EventInheritance {
    SetColor,
    MultiplyColor,
    SetOpacity,
    MultiplyOpacity,
    SetColorOpacity,
    MultiplyColorOpacity,
    SetVelocity,
    AddVelocity,
    SetSize,
    MultiplySize,
}

impl EventInheritance {
    pub(super) fn parse(entry: &Value) -> Option<Self> {
        entry
            .get(INPUT)
            .map_or(Some(Self::SetColor), |input| serde_json::from_value(input.clone()).ok())
    }

    pub(super) fn parse_operator(entry: &Value) -> Option<Self> {
        match Self::parse(entry)? {
            Self::SetSize => Some(Self::SetSize),
            Self::MultiplySize => Some(Self::MultiplySize),
            _ => None,
        }
    }

    pub(super) fn apply_operator(
        &self,
        particle: &mut Particle,
        event: EventValues,
        weight: Option<f32>,
    ) {
        match (self, weight) {
            (Self::SetSize, None) => particle.size = event.size,
            (Self::SetSize, Some(weight)) => {
                particle.size += (event.size - particle.size) * weight;
            }
            (Self::MultiplySize, weight) => {
                let factor = event.size * SIZE_TO_NATIVE;
                particle.size *= weight.map_or(factor, |weight| (factor - 1.0) * weight + 1.0);
            }
            _ => unreachable!("unsupported event operator"),
        }
    }

    pub(super) fn apply(&self, particle: &mut Particle, event: EventValues) {
        match self {
            Self::SetColor | Self::SetColorOpacity => particle.color = event.color,
            Self::MultiplyColor | Self::MultiplyColorOpacity => {
                for (channel, parent) in particle.color.iter_mut().zip(event.color) {
                    *channel *= parent;
                }
            }
            Self::SetVelocity => particle.vel = event.velocity,
            Self::AddVelocity => {
                for (speed, parent) in particle.vel.iter_mut().zip(event.velocity) {
                    *speed += parent;
                }
            }
            Self::SetSize => particle.base_size = event.size,
            Self::MultiplySize => particle.base_size *= event.size * SIZE_TO_NATIVE,
            Self::SetOpacity | Self::MultiplyOpacity => {}
        }
        match self {
            Self::SetOpacity | Self::SetColorOpacity => particle.base_alpha = event.opacity,
            Self::MultiplyOpacity | Self::MultiplyColorOpacity => {
                particle.base_alpha *= event.opacity;
            }
            _ => {}
        }
        particle.size = particle.base_size;
        particle.alpha = particle.base_alpha;
    }
}
