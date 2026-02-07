//! Minimal IPTC-IIM parser for JPEG and TIFF files.
//!
//! Extracts three fields from IPTC Record 2:
//! - ObjectName (2:05) — title
//! - Caption-Abstract (2:120) — description
//! - Keywords (2:25) — repeatable, collected into a Vec
//!
//! For JPEG: reads from APP13 marker (Photoshop 8BIM resource 0x0404).
//! For TIFF: reads from IFD tag 33723 (IPTC-NAA, raw IIM bytes).
//!
//! Zero external dependencies — pure Rust, ~150 lines.

use std::path::Path;

/// IPTC metadata extracted from an image file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IptcData {
    pub object_name: Option<String>,
    pub caption: Option<String>,
    pub keywords: Vec<String>,
}

/// Read IPTC metadata from a file, dispatching by extension.
/// Returns default (empty) metadata on any parse failure.
pub fn read_iptc(path: &Path) -> IptcData {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => return IptcData::default(),
    };

    match ext.as_str() {
        "jpg" | "jpeg" => read_iptc_from_jpeg(&bytes),
        "tif" | "tiff" => read_iptc_from_tiff(&bytes),
        _ => IptcData::default(),
    }
}

// ---------------------------------------------------------------------------
// IPTC-IIM record parsing
// ---------------------------------------------------------------------------

/// Parse raw IPTC-IIM bytes into structured metadata.
///
/// IIM record format (each dataset):
///   Byte 0:    0x1C (tag marker)
///   Byte 1:    Record number (we want 0x02)
///   Byte 2:    Dataset number (0x05=ObjectName, 0x19=Keywords, 0x78=Caption)
///   Bytes 3-4: Data length (big-endian u16)
///   Bytes 5+:  Data (UTF-8/ASCII string)
fn parse_iptc_iim(data: &[u8]) -> IptcData {
    let mut result = IptcData::default();
    let mut pos = 0;

    while pos + 5 <= data.len() {
        if data[pos] != 0x1C {
            pos += 1;
            continue;
        }

        let record = data[pos + 1];
        let dataset = data[pos + 2];
        let length = u16::from_be_bytes([data[pos + 3], data[pos + 4]]) as usize;
        pos += 5;

        if pos + length > data.len() {
            break;
        }

        // Only care about Record 2 (Application Record)
        if record == 2 {
            let value = String::from_utf8_lossy(&data[pos..pos + length])
                .trim()
                .to_string();

            if !value.is_empty() {
                match dataset {
                    5 => result.object_name = Some(value), // ObjectName
                    25 => result.keywords.push(value),     // Keywords (repeatable)
                    120 => result.caption = Some(value),   // Caption-Abstract
                    _ => {}
                }
            }
        }

        pos += length;
    }

    result
}

// ---------------------------------------------------------------------------
// JPEG: extract IPTC from APP13 / Photoshop 8BIM
// ---------------------------------------------------------------------------

/// Extract IPTC-IIM bytes from a JPEG file's APP13 marker.
///
/// Structure: APP13 contains "Photoshop 3.0\0" header, then 8BIM resource
/// blocks. Resource 0x0404 contains the raw IPTC-IIM data.
fn read_iptc_from_jpeg(data: &[u8]) -> IptcData {
    let Some(iptc_bytes) = find_jpeg_app13_iptc(data) else {
        return IptcData::default();
    };
    parse_iptc_iim(iptc_bytes)
}

const PHOTOSHOP_HEADER: &[u8] = b"Photoshop 3.0\0";
const BIM_MARKER: &[u8] = b"8BIM";
const IPTC_RESOURCE_ID: u16 = 0x0404;

/// Find the raw IPTC-IIM bytes inside a JPEG's APP13 segment.
fn find_jpeg_app13_iptc(data: &[u8]) -> Option<&[u8]> {
    // Find APP13 marker (0xFF 0xED)
    let mut pos = 0;
    while pos + 4 < data.len() {
        if data[pos] == 0xFF && data[pos + 1] == 0xED {
            let seg_len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
            let seg_start = pos + 4;
            let seg_end = (pos + 2 + seg_len).min(data.len());
            let segment = &data[seg_start..seg_end];

            if let Some(iptc) = extract_iptc_from_8bim(segment) {
                return Some(iptc);
            }
        }

        // Advance: if 0xFF, skip marker + length; otherwise byte-by-byte
        if data[pos] == 0xFF && pos + 3 < data.len() && data[pos + 1] != 0x00 {
            let marker = data[pos + 1];
            // SOS (0xDA) means image data starts — stop scanning
            if marker == 0xDA {
                break;
            }
            // Markers without length field
            if marker == 0xD8 || marker == 0xD9 || (0xD0..=0xD7).contains(&marker) {
                pos += 2;
            } else {
                let len = u16::from_be_bytes([data[pos + 2], data[pos + 3]]) as usize;
                pos += 2 + len;
            }
        } else {
            pos += 1;
        }
    }
    None
}

/// Extract IPTC-IIM bytes from a Photoshop 8BIM resource block.
///
/// Input: segment data after the JPEG marker header, starting with
/// "Photoshop 3.0\0" or directly with "8BIM" entries.
fn extract_iptc_from_8bim(segment: &[u8]) -> Option<&[u8]> {
    let data = if segment.starts_with(PHOTOSHOP_HEADER) {
        &segment[PHOTOSHOP_HEADER.len()..]
    } else {
        segment
    };

    let mut pos = 0;
    while pos + 12 <= data.len() {
        // Each resource: "8BIM" (4) + resource_id (2) + pascal_string + data_len (4) + data
        if &data[pos..pos + 4] != BIM_MARKER {
            pos += 1;
            continue;
        }
        pos += 4;

        if pos + 2 > data.len() {
            break;
        }
        let resource_id = u16::from_be_bytes([data[pos], data[pos + 1]]);
        pos += 2;

        // Pascal string: 1 byte length + string, padded to even total
        if pos >= data.len() {
            break;
        }
        let pascal_len = data[pos] as usize;
        let pascal_total = 1 + pascal_len + ((1 + pascal_len) % 2); // pad to even
        pos += pascal_total;

        if pos + 4 > data.len() {
            break;
        }
        let res_len =
            u32::from_be_bytes([data[pos], data[pos + 1], data[pos + 2], data[pos + 3]]) as usize;
        pos += 4;

        if pos + res_len > data.len() {
            break;
        }

        if resource_id == IPTC_RESOURCE_ID {
            return Some(&data[pos..pos + res_len]);
        }

        // Advance past data, padded to even
        pos += res_len + (res_len % 2);
    }

    None
}

// ---------------------------------------------------------------------------
// TIFF: extract IPTC from IFD tags
// ---------------------------------------------------------------------------

/// Read IPTC-IIM from a TIFF file.
///
/// Looks for IFD tag 33723 (IPTC-NAA, raw IIM bytes) first,
/// then falls back to tag 34377 (Photoshop 8BIM resource block).
fn read_iptc_from_tiff(data: &[u8]) -> IptcData {
    if data.len() < 8 {
        return IptcData::default();
    }

    // Determine byte order
    let big_endian = match &data[0..2] {
        b"MM" => true,
        b"II" => false,
        _ => return IptcData::default(),
    };

    let read_u16 = |offset: usize| -> u16 {
        if big_endian {
            u16::from_be_bytes([data[offset], data[offset + 1]])
        } else {
            u16::from_le_bytes([data[offset], data[offset + 1]])
        }
    };

    let read_u32 = |offset: usize| -> u32 {
        if big_endian {
            u32::from_be_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ])
        } else {
            u32::from_le_bytes([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ])
        }
    };

    // Verify TIFF magic (42)
    if read_u16(2) != 42 {
        return IptcData::default();
    }

    let mut ifd_offset = read_u32(4) as usize;

    // TIFF type sizes: count is number of values, not bytes.
    // Total bytes = count * type_size.
    let type_size = |typ: u16| -> usize {
        match typ {
            1 | 2 | 6 | 7 => 1, // BYTE, ASCII, SBYTE, UNDEFINED
            3 | 8 => 2,         // SHORT, SSHORT
            4 | 9 | 11 => 4,    // LONG, SLONG, FLOAT
            5 | 10 | 12 => 8,   // RATIONAL, SRATIONAL, DOUBLE
            _ => 1,
        }
    };

    // Walk IFD chain (main IFD + linked IFDs)
    while ifd_offset > 0 && ifd_offset + 2 < data.len() {
        let entry_count = read_u16(ifd_offset) as usize;
        let entries_start = ifd_offset + 2;

        for i in 0..entry_count {
            let entry_offset = entries_start + i * 12;
            if entry_offset + 12 > data.len() {
                return IptcData::default();
            }

            let tag = read_u16(entry_offset);
            let typ = read_u16(entry_offset + 2);
            let count = read_u32(entry_offset + 4) as usize;
            let byte_len = count * type_size(typ);
            let value_offset = read_u32(entry_offset + 8) as usize;

            // Tag 33723: IPTC-NAA — raw IPTC-IIM bytes
            if tag == 33723 && value_offset + byte_len <= data.len() {
                let result = parse_iptc_iim(&data[value_offset..value_offset + byte_len]);
                if result.object_name.is_some()
                    || result.caption.is_some()
                    || !result.keywords.is_empty()
                {
                    return result;
                }
            }

            // Tag 34377: Photoshop Image Resources — contains 8BIM blocks
            if tag == 34377 && value_offset + byte_len <= data.len() {
                let photoshop_data = &data[value_offset..value_offset + byte_len];
                if let Some(iptc_bytes) = extract_iptc_from_8bim(photoshop_data) {
                    let result = parse_iptc_iim(iptc_bytes);
                    if result.object_name.is_some()
                        || result.caption.is_some()
                        || !result.keywords.is_empty()
                    {
                        return result;
                    }
                }
            }
        }

        // Next IFD offset
        let next_offset_pos = entries_start + entry_count * 12;
        if next_offset_pos + 4 <= data.len() {
            ifd_offset = read_u32(next_offset_pos) as usize;
        } else {
            break;
        }
    }

    IptcData::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_empty_returns_default() {
        assert_eq!(parse_iptc_iim(&[]), IptcData::default());
    }

    #[test]
    fn parse_single_object_name() {
        // Record 2, Dataset 5 (ObjectName), length 5, "Hello"
        let data = [0x1C, 0x02, 0x05, 0x00, 0x05, b'H', b'e', b'l', b'l', b'o'];
        let result = parse_iptc_iim(&data);
        assert_eq!(result.object_name, Some("Hello".to_string()));
        assert_eq!(result.caption, None);
        assert!(result.keywords.is_empty());
    }

    #[test]
    fn parse_caption() {
        // Record 2, Dataset 120 (Caption), length 4, "test"
        let data = [0x1C, 0x02, 0x78, 0x00, 0x04, b't', b'e', b's', b't'];
        let result = parse_iptc_iim(&data);
        assert_eq!(result.caption, Some("test".to_string()));
    }

    #[test]
    fn parse_multiple_keywords() {
        // Two keyword entries
        let mut data = Vec::new();
        // Keyword "snow"
        data.extend_from_slice(&[0x1C, 0x02, 0x19, 0x00, 0x04]);
        data.extend_from_slice(b"snow");
        // Keyword "winter"
        data.extend_from_slice(&[0x1C, 0x02, 0x19, 0x00, 0x06]);
        data.extend_from_slice(b"winter");

        let result = parse_iptc_iim(&data);
        assert_eq!(result.keywords, vec!["snow", "winter"]);
    }

    #[test]
    fn parse_all_fields_together() {
        let mut data = Vec::new();
        // ObjectName: "Title"
        data.extend_from_slice(&[0x1C, 0x02, 0x05, 0x00, 0x05]);
        data.extend_from_slice(b"Title");
        // Keyword: "art"
        data.extend_from_slice(&[0x1C, 0x02, 0x19, 0x00, 0x03]);
        data.extend_from_slice(b"art");
        // Caption: "A caption"
        data.extend_from_slice(&[0x1C, 0x02, 0x78, 0x00, 0x09]);
        data.extend_from_slice(b"A caption");
        // Keyword: "photo"
        data.extend_from_slice(&[0x1C, 0x02, 0x19, 0x00, 0x05]);
        data.extend_from_slice(b"photo");

        let result = parse_iptc_iim(&data);
        assert_eq!(result.object_name, Some("Title".to_string()));
        assert_eq!(result.caption, Some("A caption".to_string()));
        assert_eq!(result.keywords, vec!["art", "photo"]);
    }

    #[test]
    fn skips_non_record2() {
        // Record 1, Dataset 5 — should be ignored
        let data = [0x1C, 0x01, 0x05, 0x00, 0x03, b'f', b'o', b'o'];
        let result = parse_iptc_iim(&data);
        assert_eq!(result, IptcData::default());
    }

    #[test]
    fn read_iptc_nonexistent_file() {
        let result = read_iptc(Path::new("/nonexistent/image.jpg"));
        assert_eq!(result, IptcData::default());
    }

    #[test]
    fn read_iptc_unsupported_extension() {
        let result = read_iptc(Path::new("/some/file.bmp"));
        assert_eq!(result, IptcData::default());
    }

    // Integration tests against real files (skipped if not available)

    #[test]
    fn read_iptc_from_real_jpeg() {
        let path = Path::new("content/001-NY/Q1021613.jpg");
        if !path.exists() {
            return;
        }
        let result = read_iptc(path);
        // This JPEG has keywords but no title/caption (verified via ImageMagick)
        assert!(
            !result.keywords.is_empty(),
            "Expected keywords in JPEG, got: {:?}",
            result
        );
    }

    #[test]
    fn read_iptc_from_real_tiff() {
        let path = Path::new("/Users/adebert/Downloads/photo-exports/20260125-Q1021613.tif");
        if !path.exists() {
            return;
        }
        let result = read_iptc(path);
        assert_eq!(
            result.object_name,
            Some("This is the title".to_string()),
            "TIFF title mismatch: {:?}",
            result
        );
        assert_eq!(
            result.caption,
            Some("Tihs is the caption".to_string()),
            "TIFF caption mismatch: {:?}",
            result
        );
        assert!(
            result.keywords.contains(&"snow-storm".to_string()),
            "Expected 'snow-storm' in keywords: {:?}",
            result.keywords
        );
        assert!(
            result.keywords.contains(&"white".to_string()),
            "Expected 'white' in keywords: {:?}",
            result.keywords
        );
    }
}
