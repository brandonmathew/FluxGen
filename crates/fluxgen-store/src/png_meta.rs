//! Generation-settings metadata embedded in PNG output files.
//!
//! The settings document (JSON) is stored in a `tEXt` chunk with
//! the keyword `fluxgen`. Embedding rewrites the PNG with the chunk
//! inserted before the first `IDAT`; reading scans chunks for the
//! keyword. The image data itself is never re-encoded.

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

use fluxgen_core::AppError;

/// Keyword of the `tEXt` chunk carrying the settings document.
pub const SETTINGS_CHUNK_TYPE: &str = "fluxgen";

const PNG_SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

fn storage_err(reason: impl Into<String>) -> AppError {
    AppError::Storage(reason.into())
}

/// Read the embedded settings document from a PNG file, when
/// present.
pub fn read_settings_chunk(path: &Path) -> Result<Option<String>, AppError> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut signature = [0u8; 8];
    reader.read_exact(&mut signature)?;
    if signature != PNG_SIGNATURE {
        return Err(storage_err(format!("{} is not a PNG file", path.display())));
    }
    loop {
        let mut len_buf = [0u8; 4];
        if reader.read_exact(&mut len_buf).is_err() {
            return Ok(None);
        }
        let length = u32::from_be_bytes(len_buf) as usize;
        let mut type_buf = [0u8; 4];
        reader.read_exact(&mut type_buf)?;
        match &type_buf {
            b"tEXt" => {
                let mut data = vec![0u8; length];
                reader.read_exact(&mut data)?;
                reader.seek(SeekFrom::Current(4))?; // CRC
                if let Some(sep) = data.iter().position(|&b| b == 0) {
                    let keyword = String::from_utf8_lossy(&data[..sep]).into_owned();
                    if keyword == SETTINGS_CHUNK_TYPE {
                        let text = String::from_utf8_lossy(&data[sep + 1..]).into_owned();
                        return Ok(Some(text));
                    }
                }
            }
            b"IEND" => return Ok(None),
            _ => {
                reader.seek(SeekFrom::Current(length as i64 + 4))?;
            }
        }
    }
}

/// Write (or replace) the embedded settings document in a PNG
/// file. The file is rewritten through a temporary sibling file so
/// a failure cannot corrupt the original image.
pub fn write_settings_chunk(path: &Path, settings_json: &str) -> Result<(), AppError> {
    let bytes = std::fs::read(path)?;
    if bytes.len() < 8 || bytes[..8] != PNG_SIGNATURE {
        return Err(storage_err(format!("{} is not a PNG file", path.display())));
    }

    let chunk = make_text_chunk(SETTINGS_CHUNK_TYPE, settings_json);
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len() + chunk.len());
    out.extend_from_slice(&PNG_SIGNATURE);

    let mut pos = 8usize;
    let mut inserted = false;
    while pos + 8 <= bytes.len() {
        let length = u32::from_be_bytes([
            bytes[pos],
            bytes[pos + 1],
            bytes[pos + 2],
            bytes[pos + 3],
        ]) as usize;
        let chunk_type = &bytes[pos + 4..pos + 8];
        let chunk_end = pos + 8 + length + 4;
        if chunk_end > bytes.len() {
            return Err(storage_err(format!("{} has a truncated chunk", path.display())));
        }
        let is_ours = chunk_type == b"tEXt" && is_settings_chunk(&bytes[pos + 8..pos + 8 + length]);
        if !inserted && (chunk_type == b"IDAT" || chunk_type == b"IEND") {
            out.extend_from_slice(&chunk);
            inserted = true;
        }
        if !is_ours {
            out.extend_from_slice(&bytes[pos..chunk_end]);
        }
        pos = chunk_end;
        if chunk_type == b"IEND" {
            break;
        }
    }
    if !inserted {
        return Err(storage_err(format!(
            "{} has no IDAT/IEND chunk to attach metadata to",
            path.display()
        )));
    }

    let tmp_path = path.with_extension("png.tmp");
    {
        let file = File::create(&tmp_path)?;
        let mut writer = BufWriter::new(file);
        writer.write_all(&out)?;
        writer.flush()?;
    }
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Whether a `tEXt` chunk payload carries the settings keyword.
fn is_settings_chunk(payload: &[u8]) -> bool {
    match payload.iter().position(|&b| b == 0) {
        Some(sep) => &payload[..sep] == SETTINGS_CHUNK_TYPE.as_bytes(),
        None => false,
    }
}

/// Build a complete `tEXt` chunk (length, type, payload, CRC).
fn make_text_chunk(keyword: &str, text: &str) -> Vec<u8> {
    let mut payload = Vec::with_capacity(keyword.len() + 1 + text.len());
    payload.extend_from_slice(keyword.as_bytes());
    payload.push(0);
    payload.extend_from_slice(text.as_bytes());

    let mut chunk = Vec::with_capacity(payload.len() + 12);
    chunk.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    chunk.extend_from_slice(b"tEXt");
    chunk.extend_from_slice(&payload);
    let mut crc_data = Vec::with_capacity(4 + payload.len());
    crc_data.extend_from_slice(b"tEXt");
    crc_data.extend_from_slice(&payload);
    chunk.extend_from_slice(&crc32(&crc_data).to_be_bytes());
    chunk
}

/// CRC-32 (IEEE) as used by PNG chunks.
fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a minimal valid 1x1 PNG using the `png` crate.
    fn make_png(path: &Path) {
        let file = File::create(path).expect("create png");
        let writer = BufWriter::new(file);
        let mut encoder = png::Encoder::new(writer, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut image = encoder.write_header().expect("header");
        image
            .write_image_data(&[255, 0, 0, 255])
            .expect("image data");
    }

    fn test_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("fluxgen-png-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir.join(name)
    }

    #[test]
    fn settings_chunk_round_trip() {
        let path = test_path("round.png");
        make_png(&path);
        assert_eq!(read_settings_chunk(&path).expect("read"), None);
        let json = "{\"prompt\":\"harbor at dawn\",\"seed\":42}";
        write_settings_chunk(&path, json).expect("write");
        assert_eq!(
            read_settings_chunk(&path).expect("read"),
            Some(json.to_string())
        );
        // The PNG still decodes after the rewrite.
        let decoder = png::Decoder::new(File::open(&path).expect("open"));
        let mut reader = decoder.read_info().expect("decode info");
        let mut buf = vec![0u8; 4];
        reader.next_frame(&mut buf).expect("decode frame");
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn writing_twice_replaces_the_chunk() {
        let path = test_path("twice.png");
        make_png(&path);
        write_settings_chunk(&path, "{\"v\":1}").expect("write 1");
        write_settings_chunk(&path, "{\"v\":2}").expect("write 2");
        assert_eq!(
            read_settings_chunk(&path).expect("read"),
            Some("{\"v\":2}".to_string())
        );
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn non_png_is_an_error() {
        let path = test_path("not.png");
        std::fs::write(&path, b"this is not a png").expect("write");
        assert!(read_settings_chunk(&path).is_err());
        assert!(write_settings_chunk(&path, "{}").is_err());
        std::fs::remove_file(&path).expect("cleanup");
    }

    #[test]
    fn crc32_matches_png_reference_value() {
        // CRC of "IEND" with no data is the well-known 0xAE426082.
        assert_eq!(crc32(b"IEND"), 0xAE42_6082);
    }
}
