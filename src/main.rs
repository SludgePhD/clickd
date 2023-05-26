mod config;
mod systray;

use std::{
    env, fs, io::Cursor, ops::Mul, panic::resume_unwind, path::Path, process, thread,
    time::Duration,
};

use anyhow::Context;
use config::Config;
use evdev::{EventType, InputEventKind, Key};
use rodio::{buffer::SamplesBuffer, Decoder, OutputStream, Source};

use crate::systray::SystrayIcon;

static DEFAULT_WAV: &[u8] = include_bytes!("../assets/Windows Navigation Start.wav");

#[derive(Clone)]
struct Sound {
    channels: u16,
    sample_rate: u32,
    samples: Vec<f32>,
}

impl Sound {
    fn new(wav: Vec<u8>) -> anyhow::Result<Self> {
        let decoder = Decoder::new_wav(Cursor::new(wav))?;
        let channels = decoder.channels();
        let sample_rate = decoder.sample_rate();
        let samples = decoder.convert_samples().collect::<Vec<f32>>();

        Ok(Sound {
            channels,
            sample_rate,
            samples,
        })
    }

    fn to_source(&self) -> SamplesBuffer<f32> {
        SamplesBuffer::new(self.channels, self.sample_rate, self.samples.clone())
    }
}

/// :)
impl Mul<f32> for Sound {
    type Output = Self;

    fn mul(self, rhs: f32) -> Self::Output {
        Self {
            channels: self.channels,
            sample_rate: self.sample_rate,
            samples: self.samples.into_iter().map(|f| f * rhs).collect(),
        }
    }
}

fn load_config() -> anyhow::Result<Config> {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let config: Config = match &*args {
        [] => Config::default(),
        [config] => {
            let config =
                fs::read_to_string(config).with_context(|| config.to_string_lossy().to_string())?;
            toml::from_str(&config)?
        }
        _ => {
            // Incorrect number of args.
            eprintln!("usage: clickd [<config.toml>]");
            process::exit(1);
        }
    };

    Ok(config)
}

fn load_sound(path: &Path) -> anyhow::Result<Sound> {
    let wav = fs::read(path).with_context(|| path.display().to_string())?;
    Sound::new(wav)
}

fn main() -> anyhow::Result<()> {
    let config = load_config()?;
    let buttons = match config.buttons() {
        Some(iter) => iter.collect::<Vec<_>>(),
        None => vec![Key::BTN_LEFT],
    };

    let sound = match config.audio_path() {
        Some(path) => {
            println!("opening audio file '{}'", path.display());
            load_sound(path)?
        }
        None => Sound::new(DEFAULT_WAV.to_vec())?,
    };
    let sound = sound * config.volume();

    let (_output_stream, handle) = OutputStream::try_default()?;

    let systray = if config.tray() {
        Some(SystrayIcon::new()?)
    } else {
        None
    };

    let mut threads = Vec::new();

    for (path, mut device) in evdev::enumerate() {
        if !device.supported_events().contains(EventType::KEY) {
            continue;
        }

        let keys = device.supported_keys().unwrap();
        if !buttons.iter().any(|key| keys.contains(*key)) {
            continue;
        }

        if let Some(mut devs) = config.devices() {
            if !devs.any(|name| Some(name) == device.name()) {
                continue;
            }
        }

        println!(
            "opening input device {}: {}",
            path.display(),
            device.name().unwrap(),
        );

        let handle = handle.clone();
        let sound = sound.clone();
        let buttons = buttons.clone();
        let systray = systray.clone();
        threads.push(thread::spawn(move || loop {
            let events = match device.fetch_events() {
                Ok(events) => events,
                Err(e) => {
                    eprintln!("ERROR: {e}; closing {}", path.display());
                    return;
                }
            };

            for event in events {
                if event.value() != 1 {
                    // Only react to key down events.
                    continue;
                }

                if let Some(tray) = &systray {
                    if !tray.service_enabled() {
                        continue;
                    }
                }

                if let InputEventKind::Key(key) = event.kind() {
                    if buttons.contains(&key) {
                        let source = sound.to_source();
                        if let Err(e) = handle.play_raw(source) {
                            // We exit the whole process here since there is no easy way for the
                            // main thread to block until _any_ of the worker threads exit.
                            eprintln!("audio playback failed: {e}; exiting application.");
                            process::exit(1);
                        }
                    }
                }
            }

            // rate-limit the polling loop a bit (although the persistent ALSA connection seems to
            // use a bunch of CPU, not this)
            thread::sleep(Duration::from_millis(50));
        }));
    }

    if threads.is_empty() {
        eprintln!("no matching device found!");
        process::exit(1);
    }

    // Wait for all listener threads to exit and do a worst-effort attempt at propagating panics.
    for thread in threads {
        if let Err(e) = thread.join() {
            resume_unwind(e);
        }
    }

    Ok(())
}
