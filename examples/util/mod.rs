use indicatif::{ProgressBar, ProgressStyle};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;

pub mod enumerate;
pub mod meter;
pub mod sine;

#[derive(Debug)]
#[repr(transparent)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    pub fn new(value: f32) -> Self {
        Self(AtomicU32::new(value.to_bits()))
    }

    pub fn load(&self, ordering: Ordering) -> f32 {
        f32::from_bits(self.0.load(ordering))
    }

    pub fn store(&self, value: f32, ordering: Ordering) {
        self.0.store(value.to_bits(), ordering);
    }
}

pub fn display_peakmeter(value: Arc<AtomicF32>) -> anyhow::Result<()> {
    println!("Press Enter to stop");
    let quit = Arc::new(AtomicBool::new(false));
    let handle = thread::spawn({
        let quit = quit.clone();
        move || {
            let progress = ProgressBar::new(100).with_style(
                ProgressStyle::default_bar()
                    .template("{bar:40.green} {msg}")
                    .unwrap(),
            );
            while !quit.load(Ordering::Relaxed) {
                let peak_db = 20. * value.load(Ordering::Relaxed).log10();
                let pc = normalize(-60., 6., peak_db);
                let pos = if let Some(len) = progress.length() {
                    pc * len as f32
                } else {
                    progress.set_length(100);
                    100. * pc
                };
                progress.set_position(pos as _);
                progress.set_message(format!("Peak: {peak_db:2.1} dB"));
                thread::sleep(std::time::Duration::from_millis(100));
            }
        }
    });
    thread::spawn(move || {
        std::io::stdin().read_line(&mut String::new()).unwrap();
        quit.store(true, std::sync::atomic::Ordering::Relaxed);
    });
    handle.join().unwrap();
    Ok(())
}

pub fn normalize(min: f32, max: f32, value: f32) -> f32 {
    let range = max - min;
    (value - min) / range
}
