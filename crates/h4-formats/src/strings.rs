//! String tables, `strings.*.h4d`: interface and message texts looked up by key
//! (`strings.Text.h4d`, the campaign editor's texts, credits, intro voice-over).
//!
//! Layout: first the same rows as in `table.*.h4d` (u32 row count; each row is a u16 cell
//! count and cells of u16 length plus cp1251 text), then a sorted index: u32 count and that
//! many u32 row numbers. A row is `key, text`, sometimes followed by translators' notes in
//! extra cells. Rows with an empty key are section titles ("Combat strings") or blank lines.
//!
//! The index lists every row that has a key and a text, ordered by key ignoring case, so
//! that a key can be found by binary search. The game reads both parts as they are (reader
//! rva `479900`: rows, then the index) and merges the rows, in index order, into one sorted
//! list of all loaded texts (rva `313360`). We keep each table separate and search its own
//! index.
//!
//! Keys repeat in two files (`strings.Text.h4d` lists seven network menu keys twice), always
//! with the same text; the index keeps both rows, earlier row first, and [`StringTable::get`]
//! returns the first.

use std::cmp::Ordering;

use crate::reader::{bad, Reader, Result};
use crate::table::read_rows;

/// A whole string table: all rows as decoded text plus the game's sorted index.
#[derive(Debug, Clone)]
pub struct StringTable {
    /// Every row of the file, including section titles and blank lines.
    pub rows: Vec<Vec<String>>,
    /// Numbers of the rows that have a key and a text, sorted by key ignoring case
    /// (rows with equal keys stay in file order).
    pub index: Vec<usize>,
}

impl StringTable {
    /// Parse a `strings.*.h4d` file. `name` is only used in error messages.
    ///
    /// Besides reading to the last byte, this checks the index: every number must point at
    /// a row with at least two cells, and the keys must come in order. The game trusts the
    /// file here; we check so that [`get`](Self::get) can neither panic nor miss a key.
    pub fn read(data: &[u8], name: &str) -> Result<StringTable> {
        let mut r = Reader::new(data);
        let rows = read_rows(&mut r, name)?;

        let count = r.u32()? as usize;
        // Each entry takes 4 bytes; a bigger count is garbage (and would allocate a lot).
        if count > r.remaining() / 4 {
            return bad(format!("{name}: {count} index entries don't fit in {} bytes", r.remaining()));
        }
        let mut index = Vec::with_capacity(count);
        for _ in 0..count {
            let row = r.u32()? as usize;
            match rows.get(row) {
                Some(cells) if cells.len() >= 2 => index.push(row),
                Some(_) => return bad(format!("{name}: index points at row {row}, which has no text")),
                None => return bad(format!("{name}: index points at row {row} of {}", rows.len())),
            }
        }
        if r.remaining() != 0 {
            return bad(format!("{name}: {} bytes left after the index", r.remaining()));
        }

        let table = StringTable { rows, index };
        // `windows(2)` gives every pair of neighbours: [a, b], [b, c], ...
        for pair in table.index.windows(2) {
            let (a, b) = (table.key(pair[0]), table.key(pair[1]));
            if compare_keys(a, b) == Ordering::Greater {
                return bad(format!("{name}: index is not sorted: {a:?} before {b:?}"));
            }
        }
        Ok(table)
    }

    /// The text for `key` (case doesn't matter, as in the game), or `None` if there is none.
    pub fn get(&self, key: &str) -> Option<&str> {
        // The first position whose key is not less than `key`: if `key` is in the table,
        // its first row is there. Same idea as `lower_bound` in C++.
        let pos = self.index.partition_point(|&row| compare_keys(self.key(row), key) == Ordering::Less);
        let &row = self.index.get(pos)?;
        if compare_keys(self.key(row), key) == Ordering::Equal {
            Some(&self.rows[row][1])
        } else {
            None
        }
    }

    /// All `(key, text)` pairs in index order (sorted by key ignoring case).
    pub fn entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.index.iter().map(|&row| (self.rows[row][0].as_str(), self.rows[row][1].as_str()))
    }

    /// The key of an indexed row. `read` made sure such rows have at least two cells.
    fn key(&self, row: usize) -> &str {
        &self.rows[row][0]
    }
}

/// Compare keys ignoring the case of Latin letters, like the C library's `_stricmp`.
/// Russian letters are compared as they are; no key in the game's files relies on their case.
fn compare_keys(a: &str, b: &str) -> Ordering {
    let a = a.chars().map(|c| c.to_ascii_lowercase());
    let b = b.chars().map(|c| c.to_ascii_lowercase());
    a.cmp(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a string table file the way the game stores it: rows, then the index.
    fn encode(rows: &[&[&str]], index: &[u32]) -> Vec<u8> {
        let mut out = (rows.len() as u32).to_le_bytes().to_vec();
        for row in rows {
            out.extend((row.len() as u16).to_le_bytes());
            for cell in *row {
                out.extend((cell.len() as u16).to_le_bytes());
                out.extend(cell.as_bytes());
            }
        }
        out.extend((index.len() as u32).to_le_bytes());
        for i in index {
            out.extend(i.to_le_bytes());
        }
        out
    }

    /// A small table shaped like `strings.Text.h4d`: a title, blank line, keys, a repeat.
    fn sample() -> Vec<u8> {
        encode(
            &[
                &["", "General string table"],
                &[""],
                &["ok", "OK"],
                &["Cancel", "Cancel", "translator's note"],
                &["apply", "Apply"],
                &["ok", "OK again"],
            ],
            // apply, Cancel, ok, ok — case ignored, equal keys in file order
            &[4, 3, 2, 5],
        )
    }

    #[test]
    fn finds_texts_ignoring_case() {
        let t = StringTable::read(&sample(), "test").unwrap();
        assert_eq!(t.get("cancel"), Some("Cancel"));
        assert_eq!(t.get("APPLY"), Some("Apply"));
        assert_eq!(t.get("ok"), Some("OK")); // the first of the two rows
        assert_eq!(t.get("missing"), None);
        assert_eq!(t.get(""), None); // titles are not in the index
        let keys: Vec<&str> = t.entries().map(|(k, _)| k).collect();
        assert_eq!(keys, ["apply", "Cancel", "ok", "ok"]);
    }

    #[test]
    fn rejects_bad_index() {
        let rows: &[&[&str]] = &[&["b", "B"], &["a", "A"], &["note"]];
        assert!(StringTable::read(&encode(rows, &[1, 0]), "test").is_ok());
        assert!(StringTable::read(&encode(rows, &[0, 1]), "test").is_err(), "not sorted");
        assert!(StringTable::read(&encode(rows, &[1, 3]), "test").is_err(), "past the last row");
        assert!(StringTable::read(&encode(rows, &[2]), "test").is_err(), "row without text");
    }

    #[test]
    fn rejects_truncated_and_trailing_data() {
        let data = sample();
        for len in 0..data.len() {
            assert!(StringTable::read(&data[..len], "test").is_err(), "cut at {len}");
        }
        let mut longer = data.clone();
        longer.push(0);
        assert!(StringTable::read(&longer, "test").is_err());
    }
}
