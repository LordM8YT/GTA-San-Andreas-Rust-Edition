//! Bounded, read-only classic PC San Andreas SFX banks (mono signed PCM16).
use anyhow::{ensure, Context, Result};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const HEADER: usize = 4804;
const LIMIT: usize = 16 * 1024 * 1024;

#[derive(Debug)]
pub struct PcmSound {
    pub sample_rate: u32,
    pub samples: Vec<i16>,
    /// PCM sample index (the original loop field also counts samples).
    pub loop_start: Option<usize>,
    pub headroom_hundredths_db: i16,
}
#[derive(Clone, Copy)]
struct Bank {
    package: usize,
    offset: u64,
    bytes: usize,
}
pub struct SfxArchive {
    root: PathBuf,
    packages: Vec<String>,
    banks: Vec<Bank>,
}

fn bounded_file(path: &Path, max: usize) -> Result<Vec<u8>> {
    let file = File::open(path)?;
    ensure!(
        file.metadata()?.len() <= max as u64,
        "audio metadata exceeds size limit"
    );
    let mut bytes = Vec::new();
    file.take(max as u64 + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= max, "audio metadata exceeds size limit");
    Ok(bytes)
}
impl SfxArchive {
    pub fn open(game: &Path) -> Result<Self> {
        let root = game.join("audio").canonicalize()?;
        let pak = bounded_file(&root.join("CONFIG/PakFiles.dat"), 52 * 64)?;
        ensure!(
            !pak.is_empty() && pak.len().is_multiple_of(52),
            "invalid SFX package directory"
        );
        let mut packages = Vec::new();
        for row in pak.as_chunks::<52>().0 {
            let raw = row[..12].split(|byte| *byte == 0).next().unwrap();
            ensure!(
                !raw.is_empty()
                    && raw
                        .iter()
                        .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'_'),
                "invalid SFX package name"
            );
            packages.push(std::str::from_utf8(raw)?.to_owned());
        }
        let lookup = bounded_file(&root.join("CONFIG/BankLkup.dat"), 12 * 4096)?;
        ensure!(
            !lookup.is_empty() && lookup.len().is_multiple_of(12),
            "invalid SFX bank directory"
        );
        let mut banks = Vec::new();
        for row in lookup.as_chunks::<12>().0 {
            let bank = Bank {
                package: usize::from(row[0]),
                offset: u64::from(u32::from_le_bytes(row[4..8].try_into()?)),
                bytes: u32::from_le_bytes(row[8..12].try_into()?) as usize,
            };
            ensure!(bank.package < packages.len(), "invalid SFX bank bounds");
            banks.push(bank);
        }
        Ok(Self {
            root,
            packages,
            banks,
        })
    }
    pub fn bank_count(&self) -> usize {
        self.banks.len()
    }
    pub fn read_bank(&self, index: usize) -> Result<Vec<PcmSound>> {
        let bank = self
            .banks
            .get(index)
            .context("SFX bank index out of range")?;
        ensure!(
            bank.bytes <= LIMIT,
            "requested SFX bank exceeds 16 MiB limit"
        );
        let path = self
            .root
            .join("SFX")
            .join(&self.packages[bank.package])
            .canonicalize()?;
        ensure!(
            path.starts_with(&self.root),
            "SFX package escapes audio directory"
        );
        let mut file = File::open(path)?;
        let length = HEADER + bank.bytes;
        ensure!(
            bank.offset
                .checked_add(length as u64)
                .is_some_and(|end| end <= file.metadata().map(|m| m.len()).unwrap_or(0)),
            "SFX bank exceeds package bounds"
        );
        file.seek(SeekFrom::Start(bank.offset))?;
        let mut bytes = vec![0; length];
        file.read_exact(&mut bytes)?;
        decode_bank(&bytes, bank.bytes)
    }
}
fn decode_bank(bytes: &[u8], pcm_bytes: usize) -> Result<Vec<PcmSound>> {
    ensure!(
        pcm_bytes <= LIMIT && bytes.len() == HEADER + pcm_bytes,
        "invalid SFX bank length"
    );
    let count = usize::from(u16::from_le_bytes(bytes[..2].try_into()?));
    ensure!((1..=400).contains(&count), "invalid SFX sound count");
    let offset = |index: usize| {
        u32::from_le_bytes(bytes[4 + index * 12..8 + index * 12].try_into().unwrap()) as usize
    };
    ensure!(offset(0) == 0, "SFX PCM starts outside bank origin");
    let mut sounds = Vec::with_capacity(count);
    for index in 0..count {
        let start = offset(index);
        let end = if index + 1 == count {
            pcm_bytes
        } else {
            offset(index + 1)
        };
        ensure!(
            start <= end && end <= pcm_bytes && start.is_multiple_of(2) && end.is_multiple_of(2),
            "invalid SFX PCM range"
        );
        let row = 4 + index * 12;
        let sample_rate = u32::from(u16::from_le_bytes(bytes[row + 8..row + 10].try_into()?));
        ensure!(sample_rate > 0, "unsupported SFX sample rate");
        let loop_offset = u32::from_le_bytes(bytes[row + 4..row + 8].try_into()?);
        let loop_start = if loop_offset == u32::MAX {
            None
        } else {
            ensure!(
                (loop_offset as usize) < (end - start) / 2,
                "invalid SFX loop offset"
            );
            Some(loop_offset as usize)
        };
        let samples = bytes[HEADER + start..HEADER + end]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|sample| i16::from_le_bytes(*sample))
            .collect();
        sounds.push(PcmSound {
            sample_rate,
            samples,
            loop_start,
            headroom_hundredths_db: i16::from_le_bytes(bytes[row + 10..row + 12].try_into()?),
        });
    }
    Ok(sounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0; HEADER + 8];
        bytes[..2].copy_from_slice(&2u16.to_le_bytes());
        for index in 0..2 {
            let row = 4 + index * 12;
            bytes[row..row + 4].copy_from_slice(&(index as u32 * 4).to_le_bytes());
            bytes[row + 4..row + 8].copy_from_slice(&u32::MAX.to_le_bytes());
            bytes[row + 8..row + 10].copy_from_slice(&16000u16.to_le_bytes());
        }
        for (index, sample) in [i16::MIN, i16::MAX, 0, -16384].iter().enumerate() {
            bytes[HEADER + index * 2..HEADER + index * 2 + 2]
                .copy_from_slice(&sample.to_le_bytes());
        }
        bytes
    }
    #[test]
    fn pcm_ranges_rates_and_last_sound_use_declared_bank_size() {
        let sounds = decode_bank(&fixture(), 8).unwrap();
        assert_eq!(sounds.len(), 2);
        assert_eq!(sounds[0].samples, [i16::MIN, i16::MAX]);
        assert_eq!(sounds[1].samples, [0, -16384]);
        assert_eq!(sounds[1].sample_rate, 16000);
        assert!(sounds[1].loop_start.is_none());
        let mut looping = fixture();
        looping[8..12].copy_from_slice(&1u32.to_le_bytes());
        assert_eq!(decode_bank(&looping, 8).unwrap()[0].loop_start, Some(1));
    }
    #[test]
    fn truncated_unaligned_and_out_of_range_audio_is_rejected() {
        assert!(decode_bank(&fixture()[..HEADER], 8).is_err());
        let mut bytes = fixture();
        bytes[16..20].copy_from_slice(&9u32.to_le_bytes());
        assert!(decode_bank(&bytes, 8).is_err());
        let mut bytes = fixture();
        bytes[12..14].fill(0);
        assert!(decode_bank(&bytes, 8).is_err());
        let mut bytes = fixture();
        bytes[8..12].copy_from_slice(&3u32.to_le_bytes());
        assert!(decode_bank(&bytes, 8).is_err());
    }
}
