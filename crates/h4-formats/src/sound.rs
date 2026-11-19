//! Sounds, `sound.*.h4d`: creature and spell effects, voice-overs, music.
//!
//! Layout, as the game reads it (rva `45d920` reads the format byte, `45b980` the header):
//!
//! | bytes | meaning |
//! | --- | --- |
//! | u8 | format: 0 — PCM, 1 — MP3 |
//! | u16 | header version, must be 0 (the game refuses anything else) |
//! | u8 | bits per sample (8 or 16) |
//! | u8 | channels (1 or 2) |
//! | u32 | samples per second |
//! | u32 | size of the PCM data: for PCM what follows, for MP3 the size after decoding |
//! | u16 | unknown: 1 in all files but two (3 and 20); the game keeps it in the header |
//! | u32 | MP3 only (rva `45de30`): size of the MP3 data that follows |
//!
//! Then the data: PCM samples (8-bit unsigned, 16-bit signed little-endian, channels
//! interleaved, as in a WAV file) or MP3 frames. The game keeps PCM shorter than 0x19000
//! bytes in memory and streams longer sounds from the archive (rva `45dad0`).

use crate::reader::{bad, Reader, Result};

/// How the sound data is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundFormat {
    Pcm,
    Mp3,
}

/// A sound resource: header fields plus the stored data (PCM samples or MP3 frames).
#[derive(Debug, Clone)]
pub struct Sound {
    pub format: SoundFormat,
    pub bits_per_sample: u8,
    pub channels: u8,
    pub sample_rate: u32,
    /// Bytes of PCM: the data itself for PCM, what decoding gives for MP3.
    pub pcm_size: u32,
    /// The u16 after the sizes; meaning unknown (1 in almost every file).
    pub unknown: u16,
    /// The stored data: PCM samples, or MP3 frames still to be decoded.
    pub data: Vec<u8>,
}

impl Sound {
    /// Parse a `sound.*.h4d` file. `name` is only used in error messages.
    pub fn read(data: &[u8], name: &str) -> Result<Sound> {
        let mut r = Reader::new(data);
        let format = match r.u8()? {
            0 => SoundFormat::Pcm,
            1 => SoundFormat::Mp3,
            other => return bad(format!("{name}: unknown sound format {other}")),
        };
        let version = r.u16()?;
        if version != 0 {
            return bad(format!("{name}: sound header version {version}, the game reads only 0"));
        }
        let bits_per_sample = r.u8()?;
        let channels = r.u8()?;
        let sample_rate = r.u32()?;
        let pcm_size = r.u32()?;
        let unknown = r.u16()?;
        // The game doesn't check these, but every file has them, and a player can't play
        // anything else (or would divide by zero working out the length).
        if !matches!(bits_per_sample, 8 | 16) || !matches!(channels, 1 | 2) || sample_rate == 0 {
            return bad(format!("{name}: {bits_per_sample}-bit, {channels} channels, {sample_rate} Hz"));
        }

        // How many bytes of data the header promises.
        let stored = match format {
            SoundFormat::Pcm => pcm_size,
            SoundFormat::Mp3 => r.u32()?,
        } as usize;
        if r.remaining() != stored {
            return bad(format!("{name}: header promises {stored} bytes of data, the file has {}", r.remaining()));
        }
        // One sample for every channel: 1 to 4 bytes.
        let frame_bytes = u32::from(bits_per_sample / 8) * u32::from(channels);
        if format == SoundFormat::Pcm && !pcm_size.is_multiple_of(frame_bytes) {
            return bad(format!("{name}: {pcm_size} bytes is not a whole number of samples"));
        }
        let data = r.take(stored)?.to_vec();
        Ok(Sound { format, bits_per_sample, channels, sample_rate, pcm_size, unknown, data })
    }

    /// How long the sound plays, in seconds (from the PCM size, so for MP3 too).
    pub fn duration(&self) -> f64 {
        let bytes_per_second = f64::from(self.sample_rate) * f64::from(self.channels) * f64::from(self.bits_per_sample / 8);
        f64::from(self.pcm_size) / bytes_per_second
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a sound file the way the game stores it.
    fn encode(format: u8, bits: u8, channels: u8, pcm_size: u32, data: &[u8]) -> Vec<u8> {
        let mut out = vec![format, 0, 0, bits, channels];
        out.extend(22050u32.to_le_bytes());
        out.extend(pcm_size.to_le_bytes());
        out.extend(1u16.to_le_bytes());
        if format == 1 {
            out.extend((data.len() as u32).to_le_bytes());
        }
        out.extend(data);
        out
    }

    #[test]
    fn reads_pcm_and_mp3() {
        let pcm = Sound::read(&encode(0, 16, 1, 4, &[1, 0, 2, 0]), "test").unwrap();
        assert_eq!(pcm.format, SoundFormat::Pcm);
        assert_eq!((pcm.bits_per_sample, pcm.channels, pcm.sample_rate, pcm.unknown), (16, 1, 22050, 1));
        assert_eq!(pcm.data, [1, 0, 2, 0]);

        // MP3: the PCM size is what decoding gives, the data is shorter
        let mp3 = Sound::read(&encode(1, 16, 2, 88200, &[0xff, 0xfb, 0x90]), "test").unwrap();
        assert_eq!(mp3.format, SoundFormat::Mp3);
        assert_eq!(mp3.data, [0xff, 0xfb, 0x90]);
        assert_eq!(mp3.duration(), 1.0); // 88200 bytes = 22050 stereo 16-bit samples
    }

    #[test]
    fn rejects_bad_headers() {
        assert!(Sound::read(&encode(2, 16, 1, 0, &[]), "test").is_err(), "format");
        let mut version = encode(0, 16, 1, 0, &[]);
        version[1] = 1;
        assert!(Sound::read(&version, "test").is_err(), "version");
        assert!(Sound::read(&encode(0, 12, 1, 0, &[]), "test").is_err(), "bits");
        assert!(Sound::read(&encode(0, 16, 0, 0, &[]), "test").is_err(), "channels");
        assert!(Sound::read(&encode(0, 16, 1, 3, &[0; 3]), "test").is_err(), "half a sample");
        assert!(Sound::read(&encode(0, 16, 1, 4, &[0; 2]), "test").is_err(), "short data");
        assert!(Sound::read(&encode(0, 16, 1, 2, &[0; 4]), "test").is_err(), "trailing data");
    }

    #[test]
    fn rejects_truncated_files() {
        for data in [encode(0, 8, 1, 2, &[128, 128]), encode(1, 16, 2, 4, &[0xff, 0xfb])] {
            for len in 0..data.len() {
                assert!(Sound::read(&data[..len], "test").is_err(), "cut at {len}");
            }
        }
    }
}
