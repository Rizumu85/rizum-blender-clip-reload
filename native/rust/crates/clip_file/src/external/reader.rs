use crate::ClipFileError;

pub(super) fn read_utf16_be(bytes: &[u8]) -> Result<String, ClipFileError> {
    if !bytes.len().is_multiple_of(2) {
        return Err(ClipFileError::InvalidExternalDataBlock(
            "odd-length utf16 block name",
        ));
    }
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
        .collect();
    String::from_utf16(&units).map_err(|_| ClipFileError::InvalidExternalDataBlock("bad utf16"))
}

pub(super) fn skip(bytes: &[u8], pos: &mut usize, len: usize) -> Result<(), ClipFileError> {
    let end = pos
        .checked_add(len)
        .ok_or(ClipFileError::InvalidExternalDataBlock("offset overflow"))?;
    if end > bytes.len() {
        return Err(ClipFileError::InvalidExternalDataBlock("truncated field"));
    }
    *pos = end;
    Ok(())
}

pub(super) fn read_be_u64(bytes: &[u8], pos: &mut usize) -> Result<u64, ClipFileError> {
    let end = pos
        .checked_add(8)
        .ok_or(ClipFileError::InvalidExternalDataBlock("offset overflow"))?;
    let value = u64::from_be_bytes(
        bytes
            .get(*pos..end)
            .ok_or(ClipFileError::InvalidExternalDataBlock("truncated u64"))?
            .try_into()
            .unwrap(),
    );
    *pos = end;
    Ok(value)
}

pub(super) fn read_be_u32(bytes: &[u8], pos: &mut usize) -> Result<u32, ClipFileError> {
    let end = pos
        .checked_add(4)
        .ok_or(ClipFileError::InvalidExternalDataBlock("offset overflow"))?;
    let value = u32::from_be_bytes(
        bytes
            .get(*pos..end)
            .ok_or(ClipFileError::InvalidExternalDataBlock("truncated u32"))?
            .try_into()
            .unwrap(),
    );
    *pos = end;
    Ok(value)
}

pub(super) fn read_le_u32(bytes: &[u8], pos: &mut usize) -> Result<u32, ClipFileError> {
    let end = pos
        .checked_add(4)
        .ok_or(ClipFileError::InvalidExternalDataBlock("offset overflow"))?;
    let value = u32::from_le_bytes(
        bytes
            .get(*pos..end)
            .ok_or(ClipFileError::InvalidExternalDataBlock("truncated u32"))?
            .try_into()
            .unwrap(),
    );
    *pos = end;
    Ok(value)
}
