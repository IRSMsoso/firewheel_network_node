use opus2::{Application, Channels};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpusApplicationType {
    Voip,
    Audio,
    LowDelay,
}

impl From<OpusApplicationType> for Application {
    fn from(value: OpusApplicationType) -> Self {
        match value {
            OpusApplicationType::Voip => Application::Voip,
            OpusApplicationType::Audio => Application::Audio,
            OpusApplicationType::LowDelay => Application::LowDelay,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum OpusChannels {
    Mono,
    Stereo,
}

impl From<OpusChannels> for Channels {
    fn from(value: OpusChannels) -> Self {
        match value {
            OpusChannels::Mono => Channels::Mono,
            OpusChannels::Stereo => Channels::Stereo,
        }
    }
}
