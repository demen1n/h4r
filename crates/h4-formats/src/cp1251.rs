//! Windows-1251 (Cyrillic) text, the encoding of the Russian release's strings.

/// Characters 0x80..=0xFF. 0x98 is undefined in cp1251; we show it as U+FFFD.
const HIGH: [char; 128] = [
    'Ђ', 'Ѓ', '‚', 'ѓ', '„', '…', '†', '‡', '€', '‰', 'Љ', '‹', 'Њ', 'Ќ', 'Ћ', 'Џ', //
    'ђ', '‘', '’', '“', '”', '•', '–', '—', '\u{FFFD}', '™', 'љ', '›', 'њ', 'ќ', 'ћ', 'џ', //
    '\u{A0}', 'Ў', 'ў', 'Ј', '¤', 'Ґ', '¦', '§', 'Ё', '©', 'Є', '«', '¬', '\u{AD}', '®', 'Ї', //
    '°', '±', 'І', 'і', 'ґ', 'µ', '¶', '·', 'ё', '№', 'є', '»', 'ј', 'Ѕ', 'ѕ', 'ї', //
    'А', 'Б', 'В', 'Г', 'Д', 'Е', 'Ж', 'З', 'И', 'Й', 'К', 'Л', 'М', 'Н', 'О', 'П', //
    'Р', 'С', 'Т', 'У', 'Ф', 'Х', 'Ц', 'Ч', 'Ш', 'Щ', 'Ъ', 'Ы', 'Ь', 'Э', 'Ю', 'Я', //
    'а', 'б', 'в', 'г', 'д', 'е', 'ж', 'з', 'и', 'й', 'к', 'л', 'м', 'н', 'о', 'п', //
    'р', 'с', 'т', 'у', 'ф', 'х', 'ц', 'ч', 'ш', 'щ', 'ъ', 'ы', 'ь', 'э', 'ю', 'я',
];

/// Decode cp1251 bytes. Never fails: every byte maps to some character.
pub fn decode(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| if b < 0x80 { b as char } else { HIGH[(b - 0x80) as usize] }).collect()
}

/// The cp1251 byte of a character, or `None` if cp1251 has no such character.
/// The reverse of [`decode`]; U+FFFD gives 0x98, the byte `decode` turns into it.
pub fn encode_char(c: char) -> Option<u8> {
    if c.is_ascii() {
        return Some(c as u8);
    }
    // 128 entries: a linear search is fast enough for the few lookups we do.
    HIGH.iter().position(|&h| h == c).map(|i| 0x80 + i as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_russian() {
        // "Даннте", the name of the smallest map in the Russian release
        assert_eq!(decode(&[0xc4, 0xe0, 0xed, 0xed, 0xf2, 0xe5]), "Даннте");
        assert_eq!(decode(b"v. 2.0"), "v. 2.0");
        assert_eq!(decode(&[0xa8, 0xb8, 0x98]), "Ёё\u{FFFD}");
    }

    #[test]
    fn encodes_back_every_byte() {
        for b in 0..=255u8 {
            let c = decode(&[b]).chars().next().unwrap();
            assert_eq!(encode_char(c), Some(b), "byte {b:#x}");
        }
        assert_eq!(encode_char('€'), Some(0x88));
        assert_eq!(encode_char('中'), None);
    }
}
