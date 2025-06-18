use std::{
    fmt,
    path::{Path, PathBuf},
};

use evdevil::event::Key;
use serde::{
    de::{Error, Unexpected, Visitor},
    Deserialize, Deserializer,
};

#[derive(Deserialize)]
pub struct Config {
    devices: Option<Vec<String>>,
    audio: Option<PathBuf>,
    #[serde(default = "default_volume")]
    volume: f32,
    buttons: Option<Vec<KeyName>>,
    #[serde(default = "default_tray")]
    tray: bool,
}

fn default_volume() -> f32 {
    1.0
}

fn default_tray() -> bool {
    true
}

impl Default for Config {
    fn default() -> Self {
        Self {
            devices: None,
            audio: None,
            volume: default_volume(),
            buttons: None,
            tray: default_tray(),
        }
    }
}

impl Config {
    pub fn devices(&self) -> Option<impl Iterator<Item = &str>> {
        self.devices.as_ref().map(|devs| devs.iter().map(|s| &**s))
    }

    pub fn audio_path(&self) -> Option<&Path> {
        self.audio.as_deref()
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn buttons(&self) -> Option<impl Iterator<Item = Key> + '_> {
        self.buttons.as_ref().map(|v| v.iter().map(|key| key.0))
    }

    pub fn tray(&self) -> bool {
        self.tray
    }
}

struct KeyName(Key);

impl<'de> Deserialize<'de> for KeyName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct KeyVisitor;
        impl<'de> Visitor<'de> for KeyVisitor {
            type Value = KeyName;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("evdev `Key` enum value")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                let key = v
                    .parse::<Key>()
                    .map_err(|_| E::invalid_value(Unexpected::Str(v), &self))?;
                Ok(KeyName(key))
            }
        }

        deserializer.deserialize_str(KeyVisitor)
    }
}
