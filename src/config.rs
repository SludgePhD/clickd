use std::{
    fmt,
    path::{Path, PathBuf},
};

use serde::{
    de::{Error, Unexpected, Visitor},
    Deserialize, Deserializer,
};

#[derive(Deserialize)]
pub struct Config {
    audio: Option<PathBuf>,
    #[serde(default = "default_volume")]
    volume: f32,
    buttons: Option<Vec<Key>>,
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
            audio: None,
            volume: default_volume(),
            buttons: None,
            tray: default_tray(),
        }
    }
}

impl Config {
    pub fn audio_path(&self) -> Option<&Path> {
        self.audio.as_deref()
    }

    pub fn volume(&self) -> f32 {
        self.volume
    }

    pub fn buttons(&self) -> Option<impl Iterator<Item = evdev::Key> + '_> {
        self.buttons.as_ref().map(|v| v.iter().map(|key| key.0))
    }

    pub fn tray(&self) -> bool {
        self.tray
    }
}

struct Key(evdev::Key);

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct KeyVisitor;
        impl<'de> Visitor<'de> for KeyVisitor {
            type Value = Key;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("evdev `Key` enum value")
            }

            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                let key = v
                    .parse::<evdev::Key>()
                    .map_err(|_| E::invalid_value(Unexpected::Str(v), &self))?;
                Ok(Key(key))
            }
        }

        deserializer.deserialize_str(KeyVisitor)
    }
}
