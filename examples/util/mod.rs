// This module is used in a special way: it is (re)compiled into each and every top-level example.
// Every example uses only a subset of its functionality, hence we get a lot of artificial unused
// warnings.
// Unfortunate consequence is that we lose the genuine unused warnings.
#![allow(unused)]

use crate::util::meter::Metered;
use dialoguer::console::{Style, Term};
use indicatif::{ProgressBar, ProgressStyle};
use interflow_core::stream;
use std::fmt::Write;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::{iter, thread};

pub mod meter;
pub mod noop;
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

pub fn prepare_display<C: stream::Callback>(
    callback: C,
) -> (meter::Metered<C>, impl Fn() -> anyhow::Result<()>) {
    let meter = Metered::new(callback, 100e-3);
    let shared = meter.shared();
    let meter_display = make_meter_display(-60.0, 6.0, 10);
    let display = move || {
        let term = Term::stdout();
        loop {
            let elapsed = shared.timestamp.as_timestamp().as_seconds();

            let inp = shared.input.load(Ordering::Relaxed);
            let inp = 20.0 * inp.log10();
            let inp_display = meter_display(inp);

            let out = shared.output.load(Ordering::Relaxed);
            let out = 20.0 * out.log10();
            let out_display = meter_display(out);

            term.write_str(&format!(
                "Elapsed: {elapsed:3.2} s\tInput: {inp_display} {inp:2.1} dB\tOutput: {out_display} {out:2.1} dB"
            ))?;
            std::thread::sleep(std::time::Duration::from_millis(50));
            term.clear_line()?;
        }
    };
    (meter, display)
}

fn make_meter_display(min: f32, max: f32, initial_size: usize) -> impl Fn(f32) -> String {
    const WIDTH: usize = 2;
    let size = WIDTH * initial_size;
    let fsize = size as f32;
    let position =
        move |v: f32| f32::round(fsize * normalize(min, max, v).clamp(0.0, 1.0)) as usize;
    let fullscale_pos = position(0.0);
    let style_below = Style::new().green();
    let style_above = Style::new().red();
    let style_background = Style::new().black().bright();

    move |v: f32| {
        let pos = position(v);
        let mut s = String::new();
        for i in 0..initial_size {
            let start = WIDTH * i;
            let end = start + WIDTH;
            let style = if end < fullscale_pos {
                &style_below
            } else {
                &style_above
            };
            if end < pos {
                write!(s, "{}", style.apply_to('⣿'));
            } else if start + 1 == pos {
                write!(s, "{}", style.apply_to('⡇'));
            } else {
                write!(s, "{}", style_background.apply_to(' '));
            }
        }
        s
    }
}
