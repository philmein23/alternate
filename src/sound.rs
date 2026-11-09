//! Two short tones: a higher one when you swap, a lower one when you come back to the original.

use std::fs;
use std::path::{Path, PathBuf};

use rune::Player;

const RATE: u32 = 44_100;
const LENGTH: f32 = 0.085;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    High,
    Low,
}

impl Tone {
    fn pitch(self) -> f32 {
        match self {
            Tone::High => 1900.0,
            Tone::Low => 1200.0,
        }
    }

    fn file(self) -> &'static str {
        match self {
            Tone::High => "tone-high.wav",
            Tone::Low => "tone-low.wav",
        }
    }
}

/// A complete 16-bit mono WAV of the tone: a quick rise, then a smooth fall to silence.
pub fn wav(tone: Tone) -> Vec<u8> {
    let frames = (RATE as f32 * LENGTH) as u32;
    let mut samples = Vec::with_capacity(frames as usize * 2);
    for i in 0..frames {
        let t = i as f32 / RATE as f32;
        let rise = (t / 0.003).min(1.0);
        let fall = (1.0 - t / LENGTH).max(0.0).powi(2);
        let wave = (t * tone.pitch() * std::f32::consts::TAU).sin() + 0.25 * (t * tone.pitch() * 2.0 * std::f32::consts::TAU).sin();
        samples.extend_from_slice(&((wave * rise * fall * 0.28 * i16::MAX as f32) as i16).to_le_bytes());
    }
    let size = samples.len() as u32;
    let mut out = Vec::with_capacity(44 + samples.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + size).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&samples);
    out
}

/// Plays tones, or in tests and headless renders, only notes which were asked for.
#[derive(Default)]
pub struct Sound {
    players: Option<[Player; 2]>,
    pub played: Vec<Tone>,
    pub off: bool,
    /// Where the choice to stay silent is remembered.
    dir: Option<PathBuf>,
}

const OFF_FILE: &str = "sound-off";

impl Sound {
    /// Writes the tones if needed and loads them.
    pub fn load(dir: &Path) -> Sound {
        let mut sound = Sound { off: dir.join(OFF_FILE).exists(), dir: Some(dir.to_path_buf()), ..Sound::default() };
        if fs::create_dir_all(dir).is_err() {
            return sound;
        }
        let open = |tone: Tone| {
            let path = dir.join(tone.file());
            if !path.exists() {
                fs::write(&path, wav(tone)).ok()?;
            }
            let player = Player::audio(path);
            player.preload();
            Some(player)
        };
        if let (Some(high), Some(low)) = (open(Tone::High), open(Tone::Low)) {
            sound.players = Some([high, low]);
        }
        sound
    }

    /// Flips sound on or off and remembers it for the next launch.
    pub fn toggle(&mut self) {
        self.off = !self.off;
        if let Some(path) = self.dir.as_ref().map(|d| d.join(OFF_FILE)) {
            let _ = if self.off { fs::write(path, b"") } else { fs::remove_file(path) };
        }
    }

    pub fn play(&mut self, tone: Tone) {
        if self.off {
            return;
        }
        self.played.push(tone);
        if let Some([high, low]) = &self.players {
            let player = if tone == Tone::High { high } else { low };
            player.seek(0.0);
            player.play();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tone_is_a_well_formed_wav_of_the_right_length() {
        let w = wav(Tone::High);
        assert_eq!(&w[..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes([w[24], w[25], w[26], w[27]]), RATE);
        let ms = (w.len() - 44) as f32 / 2.0 / RATE as f32 * 1000.0;
        assert!((80.0..100.0).contains(&ms), "{ms} ms");
    }

    #[test]
    fn a_tone_ends_in_silence_so_it_does_not_click() {
        let w = wav(Tone::Low);
        let last = i16::from_le_bytes([w[w.len() - 2], w[w.len() - 1]]);
        assert!(last.abs() < 40, "{last}");
    }

    #[test]
    fn the_two_tones_differ() {
        assert_ne!(wav(Tone::High), wav(Tone::Low));
    }

    #[test]
    fn the_choice_to_stay_silent_survives_a_relaunch() {
        let dir = std::env::temp_dir().join(format!("alternate-sound-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let mut s = Sound::load(&dir);
        assert!(!s.off);
        s.toggle();
        assert!(Sound::load(&dir).off);
        s.toggle();
        assert!(!Sound::load(&dir).off);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn turning_sound_off_silences_it() {
        let mut s = Sound { off: true, ..Sound::default() };
        s.play(Tone::High);
        assert!(s.played.is_empty());
    }
}
