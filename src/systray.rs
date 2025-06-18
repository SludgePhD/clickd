use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

use ksni::{Icon, Tray, TrayService};
use png::{BitDepth, ColorType};

#[derive(Clone)]
pub struct SystrayIcon {
    enabled: Arc<AtomicBool>,
}

impl SystrayIcon {
    pub fn new() -> anyhow::Result<Self> {
        let enabled = Arc::new(AtomicBool::new(true));

        let icon_enabled = decode_png(include_bytes!("../assets/icon_enabled.png"));
        let icon_disabled = decode_png(include_bytes!("../assets/icon_disabled.png"));

        let service = TrayService::new(TrayImpl {
            enabled: enabled.clone(),
            icon_enabled,
            icon_disabled,
        });
        service.spawn();

        Ok(Self { enabled })
    }

    pub fn service_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

fn decode_png(png: &[u8]) -> Icon {
    let decoder = png::Decoder::new(png);
    let mut reader = decoder.read_info().unwrap();
    assert_eq!(
        reader.output_color_type(),
        (ColorType::Rgba, BitDepth::Eight)
    );

    let mut buf = vec![0; reader.info().width as usize * reader.info().height as usize * 4];
    reader.next_frame(&mut buf).unwrap();

    for pix in buf.chunks_exact_mut(4) {
        let pix: &mut [u8; 4] = pix.try_into().unwrap();

        let [r, g, b, a] = *pix;
        *pix = [a, r, g, b];
    }

    Icon {
        width: reader.info().width as _,
        height: reader.info().height as _,
        data: buf,
    }
}

struct TrayImpl {
    enabled: Arc<AtomicBool>,
    icon_enabled: Icon,
    icon_disabled: Icon,
}

impl Tray for TrayImpl {
    fn id(&self) -> String {
        "clickd".into()
    }

    fn title(&self) -> String {
        if self.enabled.load(Ordering::Relaxed) {
            "clickd - enabled (click to disable)".into()
        } else {
            "clickd - disabled (click to enable)".into()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.enabled.fetch_xor(true, Ordering::Relaxed);
    }

    fn icon_pixmap(&self) -> Vec<Icon> {
        if self.enabled.load(Ordering::Relaxed) {
            vec![self.icon_enabled.clone()]
        } else {
            vec![self.icon_disabled.clone()]
        }
    }
}
