#![allow(dead_code, unused_variables, clippy::empty_loop)]

use device_query::{DeviceQuery, DeviceState, Keycode};
use std::{sync::mpsc, collections::HashMap};
use std::thread;
use std::time::Duration;
use rodio::{OutputStream, source::Source};
use std::f32::consts::PI;

const SAMPLE_RATE: u32 = 44_100;

enum Waveform {
    Sine,
}

enum SynthCommand {
    NoteOn(Keycode),
    NoteOff(Keycode),
}

struct Synthesizer {
    oscillators: HashMap<Keycode, Oscillator>,
    sample_rate: u32,
    command_receiver: mpsc::Receiver<SynthCommand>,
}

impl Synthesizer {
    pub fn new(sample_rate: u32, command_receiver: mpsc::Receiver<SynthCommand>) -> Self {
        Self {
            oscillators: HashMap::new(),
            sample_rate,
            command_receiver
        }
    }

    pub fn note_on(&mut self, key: Keycode, waveform: Waveform) {
        if self.oscillators.contains_key(&key) {
            return;
        }
        if let Some(freq) = frequency_from_key(key) {
            let osc = Oscillator::new(freq, waveform, self.sample_rate);
            self.oscillators.insert(key, osc);
        }
    }

    pub fn note_off(&mut self, key: &Keycode) {
        self.oscillators.remove(key);
    }

    fn process_commands(&mut self) {
        while let Ok(command) = self.command_receiver.try_recv() {
            match command {
                SynthCommand::NoteOn(key) => {
                    self.note_on(key, Waveform::Sine);
                }
                SynthCommand::NoteOff(key) => {
                    self.note_off(&key);
                }
            }
        }
    }
}

struct Oscillator {
    phase: f32,
    phase_increment: f32,
    waveform: Waveform,
    sample_rate: u32,
}

impl Oscillator {
    pub fn new(frequency: f32, waveform: Waveform, sample_rate: u32) -> Self {
        Self {
            phase: 0.0,
            phase_increment: 2.0 * PI * frequency / sample_rate as f32,
            waveform,
            sample_rate,
        }
    }

    pub fn set_frequency(&mut self, frequency: f32) {
        self.phase_increment = 2.0 * PI * frequency / self.sample_rate as f32;
    }
}

// Iterator implementation for synthesizer
impl Iterator for Synthesizer {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        // Process incoming commands before generating the next sample
        self.process_commands();
    
        // Mix the oscillators with some headroom to avoid clipping
        let headroom = 0.2; // A constant headroom value can be used as a starting point
        let mut sample_sum = 0.0;
        let mut num_oscillators = 0;
    
        for osc in self.oscillators.values_mut() {
            let osc_sample = match osc.waveform {
                Waveform::Sine => osc.phase.sin(),
                // Other waveforms can be added here
            };
    
            sample_sum += osc_sample;
            num_oscillators += 1;
    
            // Increment the phase of the oscillator
            osc.phase += osc.phase_increment;
            if osc.phase > 2.0 * PI {
                osc.phase -= 2.0 * PI;
            }
        }
    
        if num_oscillators > 0 {
            // Divide by num_oscillators to prevent clipping, apply headroom
            let sample = (sample_sum / num_oscillators as f32) * headroom;
    
            // Soft clipping
            Some(sample.min(1.0).max(-1.0))
        } else {
            Some(0.0)
        }
    }
}

impl Source for Synthesizer {
    fn current_frame_len(&self) -> Option<usize> { None }
    fn channels(&self) -> u16 { 2 }
    fn sample_rate(&self) -> u32 { self.sample_rate }
    fn total_duration(&self) -> Option<Duration> { None }
}

fn frequency_from_key(key: Keycode) -> Option<f32> {
    match key {
        Keycode::A => Some(261.63), // C4
        Keycode::W => Some(277.18), // C#4/Db4
        Keycode::S => Some(293.66), // D4
        Keycode::E => Some(311.13), // D#4/Eb4
        Keycode::D => Some(329.63), // E4
        Keycode::F => Some(349.23), // F4
        Keycode::T => Some(369.99), // F#4/Gb4
        Keycode::G => Some(392.00), // G4
        Keycode::Y => Some(415.30), // G#4/Ab4
        Keycode::H => Some(440.00), // A4
        Keycode::U => Some(466.16), // A#4/Bb4
        Keycode::J => Some(493.88), // B4
        Keycode::K => Some(523.25), // C5
        _ => None
    }
}

fn main() {
    let (tx, rx) = mpsc::channel::<SynthCommand>();
    let (_stream, stream_handle) = OutputStream::try_default().unwrap();
    let synth = Synthesizer::new(SAMPLE_RATE, rx);

    // Input handling thread
    thread::spawn({
        let tx = tx.clone();
        move || {
            let device_state = DeviceState::new();
            let mut last_pressed_keys = Vec::new();
            loop {
                let currently_pressed_keys = device_state.get_keys();
                let pressed_keys = currently_pressed_keys.iter()
                                                         .filter(|&&key| !last_pressed_keys.contains(&key)) // Notice the double dereference here
                                                         .collect::<Vec<_>>();
                let released_keys = last_pressed_keys.iter()
                                                     .filter(|&&key| !currently_pressed_keys.contains(&key)) // Same double dereference here
                                                     .collect::<Vec<_>>();
            
                // Send NoteOn commands for new keys
                for &key in pressed_keys.iter() { // Correctly getting a reference to the keycode
                    tx.send(SynthCommand::NoteOn(*key)).expect("Failed to send NoteOn");
                }
                // Send NoteOff commands for released keys
                for &key in released_keys.iter() { // Same here
                    tx.send(SynthCommand::NoteOff(*key)).expect("Failed to send NoteOff");
                }
            
                // Update the last_pressed_keys list
                last_pressed_keys = currently_pressed_keys.to_vec();

                // Polling delay
                thread::sleep(Duration::from_millis(1)); 
            }
        }
    });

    // Audio playback thread
    thread::spawn(move || {
        // The synthesizer is now directly used as the audio source
        stream_handle.play_raw(synth.convert_samples()).expect("Failed to play_raw");
    });

    // Keep the main thread alive as long as the audio needs to play.
    loop {
        thread::sleep(Duration::from_secs(1));
    }
}