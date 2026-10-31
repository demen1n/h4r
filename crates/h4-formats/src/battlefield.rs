//! Preset battlefield backdrops and their packed isometric map.
//! Reader RVAs: 184610, 1840e0, 18dfd0 (Buka 3.0).
use crate::reader::{bad, Reader};
use crate::sprite::Layer;
use crate::Result;

#[derive(Debug, Clone)]
pub struct BattlefieldRow {
    pub start: usize,
    /// Raw map values; their meaning is not yet established.
    pub cells: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct BattlefieldPreset {
    pub rows: Vec<BattlefieldRow>,
    pub layers: Vec<Layer>,
}

impl BattlefieldPreset {
    pub fn read(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let header = [r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?, r.u32()?];
        // Only the geometry present in all 24 installed resources is supported.
        // Reject other shapes until their arithmetic and bounds are verified.
        if header != [1, 1, 102, 16, 1180, 1024] {
            return bad(format!("unsupported battlefield header {header:?}"));
        }
        let mut rows = Vec::with_capacity(102);
        // From base_initialize (34db60): centered 1180x1024 rectangle,
        // 102 isometric rows, step 16. Bounds are half-open.
        for y in 0_i32..102 {
            let start = (37 - y).max(y - 37).max(0) as usize;
            let end = (38 + y).min(166 - y).min(102).max(start as i32) as usize;
            rows.push(BattlefieldRow {
                start,
                cells: vec![0; end - start],
            });
        }
        // The executable stores rows in descending order.
        for row in rows.iter_mut().rev() {
            let bytes = r.take(row.cells.len())?;
            row.cells.copy_from_slice(bytes);
        }
        let count = r.u16()?;
        let mut layers = Vec::new();
        for _ in 0..count {
            layers.push(Layer::read(&mut r)?);
        }
        if r.remaining() != 0 {
            return bad(format!(
                "{} trailing battlefield bytes at {:#x}",
                r.remaining(),
                r.pos
            ));
        }
        Ok(Self { rows, layers })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut data: Vec<u8> = [1_u32, 1, 102, 16, 1180, 1024]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect();
        data.extend((0..4838).map(|i| (i % 251) as u8));
        data.extend_from_slice(&0_u16.to_le_bytes());
        data
    }

    #[test]
    fn restores_descending_rows_and_rejects_damaged_input() {
        let data = fixture();
        let field = BattlefieldPreset::read(&data).unwrap();
        assert_eq!(field.rows[0].start, 37);
        assert_eq!(field.rows[0].cells, vec![(4837 % 251) as u8]);
        assert_eq!(field.rows[101].start, 64);
        assert_eq!(field.rows[101].cells, vec![0]);
        assert_eq!(
            field.rows.iter().map(|r| r.cells.len()).sum::<usize>(),
            4838
        );
        for end in 0..data.len() {
            assert!(
                BattlefieldPreset::read(&data[..end]).is_err(),
                "prefix {end}"
            );
        }
        let mut damaged = data.clone();
        damaged[8] = 0;
        assert!(BattlefieldPreset::read(&damaged).is_err());
        let mut trailing = data;
        trailing.push(0);
        assert!(BattlefieldPreset::read(&trailing).is_err());
    }
}
