mod config;
mod systray;

use std::{
    cmp, env, fs,
    ops::Mul,
    path::Path,
    process,
    sync::{Arc, Condvar, Mutex},
    thread,
    time::Duration,
};

use anyhow::Context;
use config::Config;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    StreamConfig,
};
use evdev::{EventType, InputEventKind, Key};
use hound::WavReader;

use crate::systray::SystrayIcon;

static DEFAULT_WAV: &[u8] = include_bytes!("../assets/Windows Navigation Start.wav");

#[derive(Clone)]
struct Sound {
    channels: u16,
    sample_rate: u32,
    samples: Vec<f32>,
}

impl Sound {
    fn new(wav: &[u8]) -> anyhow::Result<Self> {
        let mut decoder = WavReader::new(wav)?;
        let spec = decoder.spec();
        let channels = spec.channels;
        let sample_rate = spec.sample_rate;
        let samples = match spec.sample_format {
            hound::SampleFormat::Float => {
                decoder.samples::<f32>().collect::<Result<Vec<_>, _>>()?
            }
            hound::SampleFormat::Int => {
                let max = (1 << spec.bits_per_sample) as f32 * 0.5;
                decoder
                    .samples::<i32>()
                    .map(|res| res.map(|i| i as f32 / max))
                    .collect::<Result<Vec<_>, _>>()?
            }
        };

        Ok(Sound {
            channels,
            sample_rate,
            samples,
        })
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
    Sound::new(&wav)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Initial state.
    Idle,
    /// Set by the input listener threads when a sound should be played.
    Trigger,
    Playing,
    /// Set by the audio callback when the sound has finished.
    Done,
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
        None => Sound::new(DEFAULT_WAV)?,
    };
    let sound = sound * config.volume();

    let signal = Arc::new((Mutex::new(State::Idle), Condvar::new()));

    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        eprintln!("no default audio device found");
        process::exit(1);
    };
    println!("using audio device: {}", device.name()?);
    let mut offset = 0;
    let output = device.build_output_stream::<f32, _, _>(
        &StreamConfig {
            channels: sound.channels,
            buffer_size: cpal::BufferSize::Default,
            sample_rate: cpal::SampleRate(sound.sample_rate),
        },
        {
            let signal = signal.clone();
            move |data, _| {
                let mut guard = signal.0.lock().unwrap();
                match *guard {
                    State::Playing => {
                        let len = cmp::min(data.len(), sound.samples.len() - offset);

                        data.copy_from_slice(&sound.samples[offset..len]);
                        data[len..].fill(0.0);

                        offset += len;
                        offset = offset.max(sound.samples.len());

                        if offset == sound.samples.len() {
                            offset = 0;
                            *guard = State::Done;
                            signal.1.notify_one();
                        }
                    }
                    _ => data.fill(0.0),
                }
            }
        },
        |error| {
            eprintln!("playback error: {}; exiting.", error);
            process::exit(1);
        },
        None,
    )?;
    output.play()?;

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

        let signal = signal.clone();
        let buttons = buttons.clone();
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

                if let InputEventKind::Key(key) = event.kind() {
                    if buttons.contains(&key) {
                        let mut guard = signal.0.lock().unwrap();
                        if *guard == State::Idle {
                            *guard = State::Trigger;
                        }
                        signal.1.notify_one();
                        drop(guard);
                    }
                }
            }

            // rate-limit the polling loop a bit (although the persistent ALSA connection seems to
            // use a bunch of CPU, not this)
            thread::sleep(Duration::from_millis(50));
        }));
    }

    if threads.is_empty() {
        eprintln!("no matching input device found!");
        process::exit(1);
    }

    loop {
        let mut guard = signal.1.wait(signal.0.lock().unwrap()).unwrap();
        match *guard {
            State::Idle | State::Playing => {}
            State::Trigger => {
                if let Some(tray) = &systray {
                    if !tray.service_enabled() {
                        *guard = State::Idle;
                        continue;
                    }
                }

                *guard = State::Playing;
            }
            State::Done => {
                *guard = State::Idle;
            }
        }
    }
}
