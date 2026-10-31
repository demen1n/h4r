//! Combat scenery models. Layout: Buka 3.0 readers 206aa0 / 206690.
use crate::reader::{bad, Reader};
use crate::sprite::ActorSequence;
use crate::Result;

#[derive(Debug, Clone)]
pub struct CombatObject {
    pub version: u16,
    /// Two dimensions/properties; units have not been confirmed.
    pub properties: [u8; 2],
    pub parameter: u8,
    pub frames_per_second: u8,
    pub flag: u8,
    /// Five bytes introduced in version 1; meanings still unverified.
    pub attributes: [u8; 5],
    /// Original numeric type and name, before the engine resolves their enum.
    pub type_id: u16,
    pub type_name: String,
    pub image: ActorSequence,
}

impl CombatObject {
    pub fn read(data: &[u8]) -> Result<Self> {
        let mut r = Reader::new(data);
        let version = r.u16()?;
        if version > 2 {
            return bad(format!("unsupported combat_object version {version}"));
        }
        let first = r.u8()?;
        let properties = [first, if version == 2 { r.u8()? } else { first }];
        let parameter = r.u8()?;
        let frames_per_second = r.u8()?;
        let flag = r.u8()?;
        let mut attributes = [0; 5];
        let (type_id, type_name) = if version >= 1 {
            attributes.copy_from_slice(r.take(5)?);
            (r.u16()?, r.str16()?)
        } else {
            (0x25, String::new())
        };
        let image = ActorSequence::read_image_from(&mut r)?;
        if r.remaining() != 0 {
            return bad(format!(
                "{} trailing combat_object bytes at {:#x}",
                r.remaining(),
                r.pos
            ));
        }
        Ok(Self {
            version,
            properties,
            parameter,
            frames_per_second,
            flag,
            attributes,
            type_id,
            type_name,
            image,
        })
    }
}
