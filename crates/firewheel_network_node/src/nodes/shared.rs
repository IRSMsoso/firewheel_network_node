use std::fmt::{Display, Formatter};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpusApplicationType {
    Voip,
    Audio,
    RestrictedLowDelay,
}

#[derive(Debug, Copy, Clone, Error)]
pub struct OpusError(pub &'static str);

impl Display for OpusError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "Opus Error: {}", self.0)
    }
}
