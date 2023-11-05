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
        if let Some(freq) = frequency_from_key(key) {
            // If the key is already playing, reset its phase and envelope
            if let Some(osc) = self.oscillators.get_mut(&key) {
                osc.restart(freq);
            } else {
                // Create a new oscillator for the new note if not already playing
                let osc = Oscillator::new(freq, waveform, self.sample_rate);
                self.oscillators.insert(key, osc);
            }
        }
    }
    
    pub fn note_off(&mut self, key: &Keycode) {
        if let Some(osc) = self.oscillators.get_mut(key) {
            osc.start_release();
        }
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
    is_releasing: bool,  // Add this field to indicate if the oscillator is in release phase
    release_phase: f32,  // A value from 0.0 to 1.0 indicating the progress of the release
    release_rate: f32,   // The rate at which the release phase progresses
    attack_phase: f32,    // A value from 0.0 to 1.0 indicating the progress of the attack
    attack_rate: f32,     // The rate at which the attack phase progresses
}

impl Oscillator {
    pub fn new(frequency: f32, waveform: Waveform, sample_rate: u32) -> Self {
        Self {
            phase: 0.0,
            phase_increment: 2.0 * PI * frequency / sample_rate as f32,
            waveform,
            sample_rate,
            is_releasing: false,
            release_phase: 1.0, // Start at full volume for active notes
            release_rate: 1.0 / (sample_rate as f32 * 0.5), // This sets a release time of 0.5 seconds
            attack_phase: 0.0, // Start attack phase at 0 for silence
            attack_rate: 1.0 / (sample_rate as f32 * 0.01), // This sets a quick attack time of 0.01 seconds

        }
    }

    // This function resets the oscillator phase to ensure smooth transition between notes
    pub fn reset_phase(&mut self) {
        self.phase = 0.0;
    }

    // Call this when a new note is played on the same key to ensure a smooth transition
    pub fn restart(&mut self, frequency: f32) {
        self.set_frequency(frequency);
        self.reset_phase(); // Reset phase to ensure there's no click
        self.is_releasing = false; // Stop releasing because a new note is starting
        self.attack_phase = 0.0; // Reset attack phase to start a new envelope
    }

    pub fn set_frequency(&mut self, frequency: f32) {
        self.phase_increment = 2.0 * PI * frequency / self.sample_rate as f32;
    }

    pub fn start_release(&mut self) {
        // Only start the release if the note was fully attacked, otherwise set it to the attack_phase
        if !self.is_releasing && self.attack_phase >= 1.0 {
            self.is_releasing = true;
            self.release_phase = 1.0;
        } else {
            self.is_releasing = true;
            self.release_phase = self.attack_phase;
        }
    }

    pub fn apply_envelope(&mut self, sample: f32) -> f32 {
        if self.attack_phase < 1.0 {
            self.attack_phase += self.attack_rate;
            if self.attack_phase > 1.0 {
                self.attack_phase = 1.0;
            }
            return sample * self.attack_phase
        }
    
        if self.is_releasing {
            self.release_phase -= self.release_rate;
            if self.release_phase <= 0.0 {
                self.release_phase = 0.0;
                return 0.0; // Oscillator is silent, should be removed.
            }
            return sample * self.release_phase;
        }
    
        sample // If not in attack or release phase, output the sample as is.
    }
    
}

// Iterator implementation for synthesizer
impl Iterator for Synthesizer {
    type Item = f32;

    fn next(&mut self) -> Option<Self::Item> {
        // Process any pending SynthCommands (e.g., NoteOn, NoteOff)
        self.process_commands();

        // Headroom is the amount by which the signal amplitude is reduced to prevent clipping
        let headroom = 0.8; // Avoids clipping by leaving 20% headroom
        let mut sample_sum = 0.0; // This will accumulate the samples from all oscillators
        let mut active_oscillators = 0; // Counts how many oscillators are contributing to the current sample

        // A list to keep track of oscillators that have finished playing
        let mut finished_oscillators = Vec::new();

        for (key, osc) in &mut self.oscillators {
            let osc_sample = match osc.waveform {
                Waveform::Sine => osc.phase.sin(),
                // Additional waveforms can be implemented here
            };

            // Envelop the oscillator's sample (handle attack and release)
            let enveloped_sample = osc.apply_envelope(osc_sample);

            // Check if the oscillator's release phase has completed
            if osc.is_releasing && osc.release_phase <= 0.0 {
                finished_oscillators.push(*key); // Mark oscillator for removal
            } else {
                // Otherwise, accumulate the sample
                sample_sum += enveloped_sample;
                active_oscillators += 1;
            }

            // Increment the oscillator's phase, wrapping around at 2π
            osc.phase += osc.phase_increment;
            if osc.phase > 2.0 * PI {
                osc.phase -= 2.0 * PI;
            }
        }

        // Remove oscillators that have completed their release phase
        for key in finished_oscillators {
            self.oscillators.remove(&key);
        }

        // Normalize the sample sum to prevent clipping and apply headroom
        if active_oscillators > 0 {
            let average_sample = sample_sum / active_oscillators as f32;
            let normalized_sample = average_sample * headroom;

            // Enforce soft clipping
            Some(normalized_sample.clamp(-1.0, 1.0)) // Clamping the value to the range [-1.0, 1.0]
        } else {
            // If there are no active oscillators, output silence
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