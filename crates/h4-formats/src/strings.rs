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
//! with the same text. The index keeps both rows, not always in file order, and
//! [`StringTable::get`] returns the one listed first in the index.

use std::cmp::Ordering;

use crate::cp1251;
use crate::reader::{bad, Reader, Result};
use crate::table::read_rows;

/// A whole string table: all rows as decoded text plus the game's sorted index.
///
/// The fields are private: `read` checks that the index matches the rows, and `get` relies on
/// that, so nobody may change them afterwards. Use [`rows`](Self::rows) and
/// [`entries`](Self::entries) to look inside.
#[derive(Debug, Clone)]
pub struct StringTable {
    /// Every row of the file, including section titles and blank lines.
    rows: Vec<Vec<String>>,
    /// Numbers of the rows that have a key and a text, sorted by key ignoring case
    /// (rows with equal keys in whatever order the file lists them).
    index: Vec<usize>,
}

/// Does this row belong in the index: a non-empty key and a text?
fn is_entry(row: &[String]) -> bool {
    row.len() >= 2 && !row[0].is_empty()
}

impl StringTable {
    /// Parse a `strings.*.h4d` file. `name` is only used in error messages.
    ///
    /// Besides reading to the last byte, this checks the index: it must list every row with
    /// a key and a text exactly once and nothing else, and the keys must come in order. The
    /// game trusts the file here; we check so that [`get`](Self::get) can neither panic nor
    /// miss a key.
    pub fn read(data: &[u8], name: &str) -> Result<StringTable> {
        let mut r = Reader::new(data);
        let rows = read_rows(&mut r, name)?;

        let count = r.u32()? as usize;
        // Each entry takes 4 bytes; a bigger count is garbage (and would allocate a lot).
        if count > r.remaining() / 4 {
            return bad(format!("{name}: {count} index entries don't fit in {} bytes", r.remaining()));
        }
        let mut index = Vec::with_capacity(count);
        // Which rows the index has already named, to catch repeats.
        let mut listed = vec![false; rows.len()];
        for _ in 0..count {
            let row = r.u32()? as usize;
            match rows.get(row) {
                Some(cells) if !is_entry(cells) => {
                    return bad(format!("{name}: index points at row {row}, which has no key or no text"));
                }
                Some(_) if listed[row] => return bad(format!("{name}: index lists row {row} twice")),
                Some(_) => {
                    listed[row] = true;
                    index.push(row);
                }
                None => return bad(format!("{name}: index points at row {row} of {}", rows.len())),
            }
        }
        // No repeats and only entry rows, so equal counts mean every entry row is listed.
        let entries = rows.iter().filter(|row| is_entry(row)).count();
        if entries != index.len() {
            return bad(format!("{name}: {entries} rows with a key but {} in the index", index.len()));
        }
        if r.remaining() != 0 {
            return bad(format!("{name}: {} bytes left after the index", r.remaining()));
        }

        let table = StringTable { rows, index };
        // `windows(2)` gives every pair of neighbours: [a, b], [b, c], ...
        for pair in table.index.windows(2) {
            let (a, b) = (table.key(pair[0]), table.key(pair[1]));
            // Equal keys may come in any order: in the game's files they sometimes go against
            // file order (the later row of a repeated key listed first).
            if compare_keys(a, b) == Ordering::Greater {
                return bad(format!("{name}: index is not sorted: {a:?} before {b:?}"));
            }
        }
        Ok(table)
    }

    /// The text for `key` (case doesn't matter, as in the game), or `None` if there is none.
    pub fn get(&self, key: &str) -> Option<&str> {
        // The first position whose key is not less than `key`: if `key` is in the table,
        // its first row in index order is there. Same idea as `lower_bound` in C++.
        let pos = self.index.partition_point(|&row| compare_keys(self.key(row), key) == Ordering::Less);
        let &row = self.index.get(pos)?;
        if compare_keys(self.key(row), key) == Ordering::Equal {
            Some(&self.rows[row][1])
        } else {
            None
        }
    }

    /// Every row of the file, including section titles, blank lines and translators' notes.
    pub fn rows(&self) -> &[Vec<String>] {
        &self.rows
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

/// Compare keys like the C library's `_stricmp` does on the file's cp1251 bytes: ignoring
/// the case of Latin letters, everything else by byte value. Russian letters are compared as
/// they are; no key in the game's files relies on their case.
fn compare_keys(a: &str, b: &str) -> Ordering {
    a.chars().map(sort_value).cmp(b.chars().map(sort_value))
}

/// Where a character goes in key order: its lowercased cp1251 byte. The byte order differs
/// from Unicode order (cp1251 puts `Ё` before `«`, Unicode after), and the index follows the
/// bytes. A character cp1251 doesn't have can only come from a caller's key, never from the
/// file; it goes after all bytes, so it is simply not found.
fn sort_value(c: char) -> u32 {
    match cp1251::encode_char(c.to_ascii_lowercase()) {
        Some(byte) => u32::from(byte),
        None => 0x100 + u32::from(c),
    }
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
                // The game's files are cp1251, one byte per character.
                let bytes: Vec<u8> = cell.chars().map(|c| cp1251::encode_char(c).unwrap()).collect();
                out.extend((bytes.len() as u16).to_le_bytes());
                out.extend(bytes);
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
        assert_eq!(t.get("ok"), Some("OK")); // the first of the two rows in the index
        assert_eq!(t.get("missing"), None);
        assert_eq!(t.get(""), None); // titles are not in the index
        let keys: Vec<&str> = t.entries().map(|(k, _)| k).collect();
        assert_eq!(keys, ["apply", "Cancel", "ok", "ok"]);
        assert_eq!(t.rows().len(), 6);
    }

    #[test]
    fn orders_keys_by_cp1251_bytes() {
        // cp1251: Ё = 0xA8 before « = 0xAB; in Unicode it's the other way round.
        let rows: &[&[&str]] = &[&["«", "quote"], &["Ё", "yo"]];
        let t = StringTable::read(&encode(rows, &[1, 0]), "test").unwrap();
        assert_eq!(t.get("Ё"), Some("yo"));
        assert_eq!(t.get("«"), Some("quote"));
        assert_eq!(t.get("中"), None);
        assert!(StringTable::read(&encode(rows, &[0, 1]), "test").is_err());
    }

    #[test]
    fn rejects_bad_index() {
        let rows: &[&[&str]] = &[&["b", "B"], &["a", "A"], &["note"], &["", "Section"]];
        assert!(StringTable::read(&encode(rows, &[1, 0]), "test").is_ok());
        assert!(StringTable::read(&encode(rows, &[0, 1]), "test").is_err(), "not sorted");
        assert!(StringTable::read(&encode(rows, &[1, 4]), "test").is_err(), "past the last row");
        assert!(StringTable::read(&encode(rows, &[1, 0, 2]), "test").is_err(), "row without text");
        assert!(StringTable::read(&encode(rows, &[3, 1, 0]), "test").is_err(), "section title");
        assert!(StringTable::read(&encode(rows, &[1]), "test").is_err(), "row left out");
        assert!(StringTable::read(&encode(rows, &[1, 1]), "test").is_err(), "row repeated");
        // equal keys in either order are fine; `get` follows the index
        let repeated: &[&[&str]] = &[&["ok", "first"], &["OK", "second"]];
        let t = StringTable::read(&encode(repeated, &[1, 0]), "test").unwrap();
        assert_eq!(t.get("ok"), Some("second"));
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
