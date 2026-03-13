use crate::syntax::span::{FileId, FileRegistry};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PositionUtf16 {
    pub line: u32,
    pub character: u32,
}

impl PositionUtf16 {
    pub const fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

pub fn byte_offset_to_position_utf16(source: &str, byte_offset: usize) -> Option<PositionUtf16> {
    if byte_offset > source.len() || !source.is_char_boundary(byte_offset) {
        return None;
    }

    let mut line = 0u32;
    let mut character = 0u32;

    for ch in source[..byte_offset].chars() {
        if ch == '\n' {
            line += 1;
            character = 0;
        } else {
            character += ch.len_utf16() as u32;
        }
    }

    Some(PositionUtf16 { line, character })
}

pub fn position_utf16_to_byte_offset(source: &str, position: PositionUtf16) -> Option<usize> {
    let target_line = position.line;
    let target_character = position.character;
    let mut line = 0u32;
    let mut character = 0u32;

    for (idx, ch) in source.char_indices() {
        if line == target_line && character == target_character {
            return Some(idx);
        }

        if ch == '\n' {
            if line == target_line {
                return None;
            }
            line += 1;
            character = 0;
            continue;
        }

        if line == target_line {
            let next_character = character + ch.len_utf16() as u32;
            if target_character > character && target_character < next_character {
                // Position points into the middle of a surrogate pair.
                return None;
            }
            character = next_character;
        }
    }

    if line == target_line && character == target_character {
        Some(source.len())
    } else {
        None
    }
}

pub fn file_position_utf16_to_byte_offset(
    registry: &FileRegistry,
    file_id: FileId,
    position: PositionUtf16,
) -> Option<u32> {
    let source = registry.source(file_id)?;
    let offset = position_utf16_to_byte_offset(source, position)?;
    u32::try_from(offset).ok()
}

pub fn file_byte_offset_to_position_utf16(
    registry: &FileRegistry,
    file_id: FileId,
    byte_offset: u32,
) -> Option<PositionUtf16> {
    let source = registry.source(file_id)?;
    byte_offset_to_position_utf16(source, byte_offset as usize)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_positions_round_trip() {
        let source = "abc\nxy";
        let pos = PositionUtf16::new(1, 1);
        let offset = position_utf16_to_byte_offset(source, pos).expect("position should convert");
        assert_eq!(offset, 5);

        let back =
            byte_offset_to_position_utf16(source, offset).expect("offset should convert back");
        assert_eq!(back, pos);
    }

    #[test]
    fn multibyte_utf16_positions_round_trip() {
        let source = "a😀b\n好z";
        let pos = PositionUtf16::new(0, 3);
        let offset = position_utf16_to_byte_offset(source, pos).expect("position should convert");
        assert_eq!(offset, 5);

        let back =
            byte_offset_to_position_utf16(source, offset).expect("offset should convert back");
        assert_eq!(back, pos);
    }

    #[test]
    fn rejects_position_inside_surrogate_pair() {
        let source = "a😀b";
        assert_eq!(
            position_utf16_to_byte_offset(source, PositionUtf16::new(0, 2)),
            None
        );
    }
}
