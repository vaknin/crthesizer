use rodio::{Source, OutputStream};
use std::f32::consts::TAU;
use std::sync::{Arc, Mutex};
use std::thread;
use crossterm::event::{self, KeyCode, KeyEvent};

const SAMPLE_RATE: u32 = 44_100;

enum Waveform {
    Sine,
    Sawtooth,
    Square
}

// A struct representing our sine wave generator
struct Oscillator {
    phase: f32,
    current_freq: f32,
    target_freq: Arc<Mutex<f32>>,
    lerp_speed: f32,
    sample_rate: u32,
    waveform: Waveform
}

impl Oscillator {
    pub fn new(target_freq: Arc<Mutex<f32>>, waveform: Waveform) -> Self {
        let current_freq = *target_freq.lock().unwrap();

        Self {
            phase: 0.0,
            current_freq,
            target_freq,
            lerp_speed: 0.001,
            sample_rate: SAMPLE_RATE,
            waveform
        }
    }

    // Linearly interpolate to smoothly approach target frequency
    fn lerp(&mut self) {
        let target = *self.target_freq.lock().unwrap();
        self.current_freq += (target - self.current_freq) * self.lerp_speed;
    }
}

impl Iterator for Oscillator {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        self.lerp();
        let sample = match self.waveform {
            Waveform::Sine => (self.phase * TAU).sin(),
            Waveform::Sawtooth => 2.0 * self.phase - 1.0,
            Waveform::Square => {
                if self.phase < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            },
        };
        self.phase += self.current_freq / self.sample_rate as f32;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }
        Some(sample)
    }
}

// Implementing the rodio::Source trait to make our sine wave playable
impl Source for Oscillator {
    fn current_frame_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        1
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

fn main() {
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let freq = Arc::new(Mutex::new(440.0));
    let sine = Oscillator::new(freq.clone(), Waveform::Sine);
    let sawtooth = Oscillator::new(freq.clone(), Waveform::Sawtooth);
    let square = Oscillator::new(freq.clone(), Waveform::Square);

    let stream_handle = Arc::new(stream_handle);

    let sine = Oscillator::new(freq.clone(), Waveform::Sine);
    let sawtooth = Oscillator::new(freq.clone(), Waveform::Sawtooth);

    let stream_handle_clone1 = stream_handle.clone();
    thread::spawn(move || {
        stream_handle_clone1.play_raw(sine.convert_samples::<f32>()).unwrap();
    });

    // let stream_handle_clone2 = stream_handle.clone();
    // thread::spawn(move || {
    //     stream_handle_clone2.play_raw(sawtooth.convert_samples::<f32>()).unwrap();
    // });

    // Listen for key events to adjust frequency or quit
    loop {
        if let Ok(crossterm::event::Event::Key(KeyEvent { code, .. })) = event::read() {
            match code {
                KeyCode::Up => {
                    let mut locked_freq = freq.lock().unwrap();
                    *locked_freq += 5.0;
                    println!("Frequency: {} Hz", *locked_freq);
                }
                KeyCode::Down => {
                    let mut locked_freq = freq.lock().unwrap();
                    *locked_freq -= 5.0;
                    println!("Frequency: {} Hz", *locked_freq);
                }
                KeyCode::Char('q') | KeyCode::Char('Q') => {
                    break;
                }
                _ => {}
            }
        }
    }

    // Clean up the terminal before exiting
    crossterm::terminal::disable_raw_mode().unwrap();
}