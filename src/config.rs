use std::path::{Path, PathBuf};

use evdevil::event::Key;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct Config {
    devices: Option<Vec<String>>,
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
        self.buttons.as_ref().map(|v| v.iter().copied())
    }

    pub fn tray(&self) -> bool {
        self.tray
    }
}
