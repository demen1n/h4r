//! `.h4r` resource archives.
//!
//! Layout: `"H4R\x05"`, u32 directory offset, gzip blobs. The directory is a u32 count, then
//! per entry u32 offset, compressed size, size, time; three u16-prefixed strings (name, source
//! path, alias); u32 type. An entry with an alias has no data and points at another name.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use flate2::read::GzDecoder;

use crate::reader::{bad, Reader};
use crate::Result;

/// Archives in the order heroes4.exe opens them (observed with lsof); the first hit wins.
pub const LOAD_ORDER: &[&str] = &[
    "x2_override.h4r",
    "updates.h4r",
    "text.h4r",
    "music.h4r",
    "movies.h4r",
    "x2.h4r",
    "storm.h4r",
    "heroes4.h4r",
];

#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub offset: u32,
    pub compressed_size: u32,
    pub size: u32,
    pub alias: Option<String>,
    pub kind: u32,
}

/// One archive. Only the directory is kept in memory; blobs are read on demand.
pub struct Archive {
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    file: Mutex<File>,
}

impl Archive {
    pub fn open(path: impl AsRef<Path>) -> Result<Archive> {
        let path = path.as_ref().to_path_buf();
        let mut file = File::open(&path)?;
        let mut head = [0u8; 8];
        file.read_exact(&mut head)?;
        if &head[..4] != b"H4R\x05" {
            return bad(format!("{} is not an .h4r archive", path.display()));
        }
        let dir_offset = u32::from_le_bytes(head[4..8].try_into().unwrap()) as u64;
        let mut dir = Vec::new();
        file.seek(SeekFrom::Start(dir_offset))?;
        file.read_to_end(&mut dir)?;

        let mut r = Reader::new(&dir);
        let count = r.u32()?;
        let mut entries = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let offset = r.u32()?;
            let compressed_size = r.u32()?;
            let size = r.u32()?;
            let _time = r.u32()?;
            let name = r.str16()?;
            let _source_path = r.str16()?;
            let alias = r.str16()?;
            let kind = r.u32()?;
            entries.push(Entry {
                name,
                offset,
                compressed_size,
                size,
                alias: (!alias.is_empty()).then_some(alias),
                kind,
            });
        }
        Ok(Archive { path, entries, file: Mutex::new(file) })
    }

    /// Read and decompress one entry's data.
    pub fn read(&self, e: &Entry) -> Result<Vec<u8>> {
        let mut blob = vec![0u8; e.compressed_size as usize];
        {
            let mut f = self.file.lock().unwrap();
            f.seek(SeekFrom::Start(e.offset as u64))?;
            f.read_exact(&mut blob)?;
        }
        let mut out = Vec::with_capacity(e.size as usize);
        GzDecoder::new(&blob[..]).read_to_end(&mut out)?;
        Ok(out)
    }
}

/// All archives of a game install, looked up by name (case-insensitive) in load order.
pub struct Resources {
    pub archives: Vec<Archive>,
    index: HashMap<String, (usize, usize)>,
}

impl Resources {
    /// Open every archive of [`LOAD_ORDER`] that exists in `data_dir`.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Resources> {
        let dir = data_dir.as_ref();
        let mut archives = Vec::new();
        for name in LOAD_ORDER {
            let p = dir.join(name);
            if p.exists() {
                archives.push(Archive::open(p)?);
            }
        }
        if archives.is_empty() {
            return bad(format!("no .h4r archives in {}", dir.display()));
        }
        let mut index = HashMap::new();
        for (ai, a) in archives.iter().enumerate() {
            for (ei, e) in a.entries.iter().enumerate() {
                index.entry(e.name.to_lowercase()).or_insert((ai, ei));
            }
        }
        Ok(Resources { archives, index })
    }

    /// Every visible resource name (after priority), in no particular order.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.index.values().map(|&(a, e)| self.archives[a].entries[e].name.as_str())
    }

    pub fn entry(&self, name: &str) -> Option<&Entry> {
        self.index.get(&name.to_lowercase()).map(|&(a, e)| &self.archives[a].entries[e])
    }

    /// Read a resource, following aliases.
    pub fn read(&self, name: &str) -> Result<Vec<u8>> {
        let mut name = name.to_lowercase();
        for _ in 0..8 {
            let Some(&(a, e)) = self.index.get(&name) else {
                return bad(format!("no resource {name}"));
            };
            let entry = &self.archives[a].entries[e];
            match &entry.alias {
                Some(target) => name = target.to_lowercase(),
                None => return self.archives[a].read(entry),
            }
        }
        bad(format!("alias loop at {name}"))
    }
}
