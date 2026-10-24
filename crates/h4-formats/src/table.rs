//! Spreadsheet-like game tables, `table.*.h4d` (creatures, spells, artifacts, ...).
//!
//! Layout: u32 row count; each row is a u16 cell count followed by that many cells,
//! and each cell is a u16 length plus cp1251 text. Rows can differ in length.
//! The first two rows are headers with merged cells (many of them empty); the data
//! starts on the third row. The numbers in the cells are stored as text, like in the
//! spreadsheet the designers exported them from.

use crate::cp1251;
use crate::reader::{bad, Reader, Result};

/// How many header rows every table starts with.
pub const HEADER_ROWS: usize = 2;

/// A whole table: header rows first, then data rows, all as decoded text.
#[derive(Debug, Clone)]
pub struct Table {
    pub rows: Vec<Vec<String>>,
}

impl Table {
    /// Parse a `table.*.h4d` file. `name` is only used in error messages.
    pub fn read(data: &[u8], name: &str) -> Result<Table> {
        let mut r = Reader::new(data);
        let row_count = r.u32()? as usize;
        // Every row takes at least 2 bytes, so a bigger count can only be garbage;
        // checking it first avoids a huge allocation on a corrupt file.
        if row_count > r.remaining() / 2 {
            return bad(format!("{name}: {row_count} rows don't fit in {} bytes", data.len()));
        }
        let mut rows = Vec::with_capacity(row_count);
        for _ in 0..row_count {
            let cells = r.u16()? as usize;
            let mut row = Vec::with_capacity(cells);
            for _ in 0..cells {
                let len = r.u16()? as usize;
                row.push(cp1251::decode(r.take(len)?));
            }
            rows.push(row);
        }
        if r.remaining() != 0 {
            return bad(format!("{name}: {} bytes left after the last row", r.remaining()));
        }
        Ok(Table { rows })
    }

    /// The rows after the headers.
    pub fn data_rows(&self) -> &[Vec<String>] {
        &self.rows[HEADER_ROWS.min(self.rows.len())..]
    }

    /// Index of the column whose second header row says `title` (e.g. "Hit Points").
    /// The second row holds the column names; the first only groups some of them
    /// ("Dmg" over Low/High, "Cost" over the resources).
    pub fn column(&self, title: &str) -> Option<usize> {
        self.rows.get(1)?.iter().position(|c| c.trim() == title)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a table file the way the game stores it.
    fn encode(rows: &[&[&str]]) -> Vec<u8> {
        let mut out = (rows.len() as u32).to_le_bytes().to_vec();
        for row in rows {
            out.extend((row.len() as u16).to_le_bytes());
            for cell in *row {
                out.extend((cell.len() as u16).to_le_bytes());
                out.extend(cell.as_bytes());
            }
        }
        out
    }

    #[test]
    fn reads_rows_and_finds_columns() {
        let data = encode(&[&["", "Dmg", ""], &["Keyword", "Low", "High"], &["orc", "1", "3"]]);
        let t = Table::read(&data, "test").unwrap();
        assert_eq!(t.data_rows(), &[vec!["orc".to_string(), "1".into(), "3".into()]]);
        assert_eq!(t.column("High"), Some(2));
        assert_eq!(t.column("Dmg"), None); // only the second header row names columns
    }

    #[test]
    fn rejects_truncated_and_trailing_data() {
        let data = encode(&[&["a", "b"], &["c"]]);
        for len in 0..data.len() {
            assert!(Table::read(&data[..len], "test").is_err(), "cut at {len}");
        }
        let mut longer = data.clone();
        longer.push(0);
        assert!(Table::read(&longer, "test").is_err());
        // a row count far bigger than the file
        assert!(Table::read(&[0xff, 0xff, 0xff, 0x7f, 0, 0], "test").is_err());
    }
}
