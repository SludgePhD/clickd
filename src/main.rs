mod config;
mod systray;

use std::{cmp, env, fs, ops::Mul, path::Path, process, sync::mpsc, thread, time::Duration};

use anyhow::Context;
use config::Config;
use cpal::{
    traits::{DeviceTrait, HostTrait, StreamTrait},
    StreamConfig,
};
use evdevil::{
    bits::BitSet,
    event::{EventKind, EventType, Key, KeyState},
};
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

    let (sender, recv) = mpsc::sync_channel(1);

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
            move |data, _| {
                if offset != 0 || recv.try_recv().is_ok() {
                    let len = cmp::min(data.len(), sound.samples.len() - offset);

                    data.copy_from_slice(&sound.samples[offset..len]);
                    data[len..].fill(0.0);

                    offset += len;
                    offset = offset.max(sound.samples.len());

                    if offset == sound.samples.len() {
                        offset = 0;
                    }
                } else {
                    data.fill(0.0);
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

    for res in evdevil::enumerate_hotplug()? {
        let device = match res {
            Ok(dev) => dev,
            Err(e) => {
                eprintln!("couldn't open device: {e}");
                continue;
            }
        };
        if !device.supported_events()?.contains(EventType::KEY) {
            continue;
        }

        let keys = device.supported_keys()?;
        if !buttons.iter().any(|key| keys.contains(*key)) {
            continue;
        }

        let devname = device.name()?;
        if let Some(mut devs) = config.devices() {
            if !devs.any(|d| d == &*devname) {
                continue;
            }
        }

        let path = device.path().to_path_buf();
        println!(
            "opening input device {}: {}",
            path.display(),
            device.name().unwrap(),
        );

        // We only care about key/button presses, don't wake us for every mouse movement.
        device.set_event_mask(&BitSet::from_iter([EventType::KEY]))?;

        let mut reader = device.into_reader()?;
        let buttons = buttons.clone();
        let sender = sender.clone();
        let systray = systray.clone();
        threads.push(thread::spawn(move || loop {
            for res in &mut reader {
                let ev = match res {
                    Ok(ev) => ev,
                    Err(e) => {
                        eprintln!("ERROR: {e}; closing {}", path.display());
                        return;
                    }
                };

                if let EventKind::Key(ev) = ev.kind() {
                    if ev.state() == KeyState::PRESSED && buttons.contains(&ev.key()) {
                        let should_play = match &systray {
                            None => true,
                            Some(systray) => systray.service_enabled(),
                        };

                        if should_play {
                            sender.try_send(()).ok();
                        }
                    }
                }
            }

            // rate-limit the polling loop a bit (although the persistent ALSA connection seems to
            // use a bunch of CPU, not this)
            thread::sleep(Duration::from_millis(50));
        }));
    }

    Ok(())
}
