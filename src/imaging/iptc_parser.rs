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
            // Some IPTC writers null-terminate ASCII values; `str::trim()` won't
            // strip NULs, so we trim NUL alongside ASCII whitespace explicitly.
            let value = String::from_utf8_lossy(&data[pos..pos + length])
                .trim_matches(|c: char| c.is_whitespace() || c == '\0')
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

    #[test]
    fn trims_surrounding_whitespace_in_value() {
        // Value with leading/trailing whitespace (and null) should be trimmed.
        let mut data = vec![0x1C, 0x02, 0x05, 0x00, 0x09];
        data.extend_from_slice(b"  Hello \0");
        let result = parse_iptc_iim(&data);
        assert_eq!(result.object_name, Some("Hello".to_string()));
    }

    #[test]
    fn trims_trailing_nul_only() {
        // Writers that pad ASCII with a single NUL must not leak it into the value.
        let mut data = vec![0x1C, 0x02, 0x78, 0x00, 0x05];
        data.extend_from_slice(b"Done\0");
        let result = parse_iptc_iim(&data);
        assert_eq!(result.caption, Some("Done".to_string()));
    }

    #[test]
    fn empty_value_is_skipped() {
        // Length-0 ObjectName should not set the field.
        let data = [0x1C, 0x02, 0x05, 0x00, 0x00];
        let result = parse_iptc_iim(&data);
        assert_eq!(result, IptcData::default());
    }

    #[test]
    fn unknown_dataset_is_ignored() {
        // Record 2, Dataset 0xFF (not 5/25/120) — must be ignored, no panic.
        let data = [0x1C, 0x02, 0xFF, 0x00, 0x03, b'a', b'b', b'c'];
        let result = parse_iptc_iim(&data);
        assert_eq!(result, IptcData::default());
    }

    #[test]
    fn truncated_length_stops_parsing() {
        // Declared length (10) exceeds remaining bytes (3) — must stop, not panic.
        let data = [0x1C, 0x02, 0x05, 0x00, 0x0A, b'a', b'b', b'c'];
        let result = parse_iptc_iim(&data);
        assert_eq!(result, IptcData::default());
    }

    #[test]
    fn leading_noise_before_tag_marker() {
        // Garbage before the 0x1C tag marker is skipped byte-by-byte.
        let mut data = vec![0xAA, 0xBB, 0xCC];
        data.extend_from_slice(&[0x1C, 0x02, 0x05, 0x00, 0x03, b'h', b'i', b'!']);
        let result = parse_iptc_iim(&data);
        assert_eq!(result.object_name, Some("hi!".to_string()));
    }

    // =========================================================================
    // Test helpers for synthetic JPEG / TIFF / 8BIM construction.
    // =========================================================================

    /// Build a single IIM dataset record: 0x1C, record, dataset, BE u16 length, payload.
    fn iim_record(record: u8, dataset: u8, value: &[u8]) -> Vec<u8> {
        let mut v = vec![0x1C, record, dataset];
        let len = u16::try_from(value.len()).expect("test payload <= u16::MAX");
        v.extend_from_slice(&len.to_be_bytes());
        v.extend_from_slice(value);
        v
    }

    /// Build an 8BIM resource block with the given resource_id and payload.
    /// Uses an empty pascal name (padded to 2 bytes) and pads data to even length.
    fn bim_block(resource_id: u16, payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(b"8BIM");
        v.extend_from_slice(&resource_id.to_be_bytes());
        v.extend_from_slice(&[0x00, 0x00]); // empty pascal name, padded to even
        let plen = u32::try_from(payload.len()).expect("payload fits in u32");
        v.extend_from_slice(&plen.to_be_bytes());
        v.extend_from_slice(payload);
        if !payload.len().is_multiple_of(2) {
            v.push(0x00); // pad data to even
        }
        v
    }

    /// Build an 8BIM block with a non-empty pascal name (exercises pascal padding).
    fn bim_block_named(resource_id: u16, name: &[u8], payload: &[u8]) -> Vec<u8> {
        assert!(name.len() < 256);
        let mut v = Vec::new();
        v.extend_from_slice(b"8BIM");
        v.extend_from_slice(&resource_id.to_be_bytes());
        v.push(name.len() as u8);
        v.extend_from_slice(name);
        if !(1 + name.len()).is_multiple_of(2) {
            v.push(0x00);
        }
        let plen = u32::try_from(payload.len()).unwrap();
        v.extend_from_slice(&plen.to_be_bytes());
        v.extend_from_slice(payload);
        if !payload.len().is_multiple_of(2) {
            v.push(0x00);
        }
        v
    }

    /// Wrap a Photoshop image-resources blob (one or more 8BIM blocks) in a JPEG
    /// APP13 segment, between SOI and SOS markers.
    fn jpeg_with_photoshop(blob: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(PHOTOSHOP_HEADER);
        payload.extend_from_slice(blob);
        // APP13 length field counts itself + payload (per JPEG spec).
        let seg_len = u16::try_from(2 + payload.len()).unwrap();

        let mut v = vec![0xFF, 0xD8]; // SOI
        v.extend_from_slice(&[0xFF, 0xED]); // APP13
        v.extend_from_slice(&seg_len.to_be_bytes());
        v.extend_from_slice(&payload);
        v.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]); // SOS marker (stops scan)
        v
    }

    /// Build a minimal TIFF (one IFD, one entry) carrying the given tag/payload.
    /// IPTC payloads always live at an offset past the IFD (they exceed 4 bytes).
    fn tiff_with_tag(big_endian: bool, tag: u16, typ: u16, payload: &[u8]) -> Vec<u8> {
        // Layout: header(8) | IFD count(2) | entry(12) | next_ifd(4) | payload@offset
        let payload_offset: u32 = 8 + 2 + 12 + 4;
        let count: u32 = payload.len() as u32; // type 1/7 → 1 byte per count

        let u16b = |x: u16| -> [u8; 2] {
            if big_endian {
                x.to_be_bytes()
            } else {
                x.to_le_bytes()
            }
        };
        let u32b = |x: u32| -> [u8; 4] {
            if big_endian {
                x.to_be_bytes()
            } else {
                x.to_le_bytes()
            }
        };

        let mut v = Vec::new();
        v.extend_from_slice(if big_endian { b"MM" } else { b"II" });
        v.extend_from_slice(&u16b(42));
        v.extend_from_slice(&u32b(8)); // IFD at offset 8
        v.extend_from_slice(&u16b(1)); // one entry
        v.extend_from_slice(&u16b(tag));
        v.extend_from_slice(&u16b(typ));
        v.extend_from_slice(&u32b(count));
        v.extend_from_slice(&u32b(payload_offset));
        v.extend_from_slice(&u32b(0)); // no next IFD
        v.extend_from_slice(payload);
        v
    }

    // =========================================================================
    // 8BIM resource block parsing
    // =========================================================================

    #[test]
    fn bim_finds_iptc_among_other_resources() {
        let iim = iim_record(2, 5, b"Title");
        let mut blob = Vec::new();
        blob.extend_from_slice(&bim_block(0x03ED, b"\x00\x01\x02\x03")); // non-IPTC resource
        blob.extend_from_slice(&bim_block(0x0404, &iim));
        blob.extend_from_slice(&bim_block(0x0425, b"trailing"));

        let extracted = extract_iptc_from_8bim(&blob).expect("found IPTC block");
        assert_eq!(parse_iptc_iim(extracted).object_name, Some("Title".into()));
    }

    #[test]
    fn bim_returns_none_when_no_iptc_resource() {
        let blob = bim_block(0x03ED, b"not iptc data");
        assert!(extract_iptc_from_8bim(&blob).is_none());
    }

    #[test]
    fn bim_handles_named_resource_padding() {
        // Non-empty pascal name exercises the `(1 + n) % 2` padding branch.
        let iim = iim_record(2, 120, b"Caption text");
        // Name "ab" → 1 + 2 = 3 bytes → +1 pad. Resource before the IPTC one.
        let blob_pre = bim_block_named(0x03ED, b"ab", b"prelude");
        let blob_iptc = bim_block_named(0x0404, b"x", &iim); // name "x" → no extra pad
        let mut blob = blob_pre;
        blob.extend_from_slice(&blob_iptc);
        let extracted = extract_iptc_from_8bim(&blob).expect("found IPTC");
        assert_eq!(
            parse_iptc_iim(extracted).caption,
            Some("Caption text".into())
        );
    }

    #[test]
    fn bim_accepts_blob_with_photoshop_header() {
        // extract_iptc_from_8bim must transparently strip the "Photoshop 3.0\0" header.
        let iim = iim_record(2, 5, b"Header");
        let mut blob = Vec::new();
        blob.extend_from_slice(PHOTOSHOP_HEADER);
        blob.extend_from_slice(&bim_block(0x0404, &iim));
        let extracted = extract_iptc_from_8bim(&blob).unwrap();
        assert_eq!(parse_iptc_iim(extracted).object_name, Some("Header".into()));
    }

    // =========================================================================
    // JPEG APP13 scanning
    // =========================================================================

    #[test]
    fn jpeg_round_trip_with_synthetic_app13() {
        let mut iim = Vec::new();
        iim.extend_from_slice(&iim_record(2, 5, b"My Photo"));
        iim.extend_from_slice(&iim_record(2, 120, b"On a winter morning"));
        iim.extend_from_slice(&iim_record(2, 25, b"snow"));
        iim.extend_from_slice(&iim_record(2, 25, b"winter"));
        let jpeg = jpeg_with_photoshop(&bim_block(0x0404, &iim));

        let result = read_iptc_from_jpeg(&jpeg);
        assert_eq!(result.object_name, Some("My Photo".into()));
        assert_eq!(result.caption, Some("On a winter morning".into()));
        assert_eq!(result.keywords, vec!["snow", "winter"]);
    }

    #[test]
    fn jpeg_with_no_app13_returns_default() {
        // SOI + JFIF APP0 + SOS, no APP13 anywhere.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00];
        jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);
        assert_eq!(read_iptc_from_jpeg(&jpeg), IptcData::default());
    }

    #[test]
    fn jpeg_with_app13_but_no_iptc_resource_returns_default() {
        // APP13 holds a non-IPTC 8BIM resource only.
        let jpeg = jpeg_with_photoshop(&bim_block(0x03ED, b"not iptc"));
        assert_eq!(read_iptc_from_jpeg(&jpeg), IptcData::default());
    }

    #[test]
    fn jpeg_skips_rst_markers_without_length_field() {
        // Inject restart markers (0xFF 0xD0..0xD7) before APP13 — they have no length
        // field, so the marker-walk must advance by 2, not by 2+length.
        let iim = iim_record(2, 5, b"After RST");
        let body = bim_block(0x0404, &iim);

        let mut payload = Vec::new();
        payload.extend_from_slice(PHOTOSHOP_HEADER);
        payload.extend_from_slice(&body);
        let seg_len = u16::try_from(2 + payload.len()).unwrap();

        let mut jpeg = vec![0xFF, 0xD8];
        // RST0..RST3 inline — no length bytes.
        jpeg.extend_from_slice(&[0xFF, 0xD0, 0xFF, 0xD1, 0xFF, 0xD2, 0xFF, 0xD3]);
        jpeg.extend_from_slice(&[0xFF, 0xED]);
        jpeg.extend_from_slice(&seg_len.to_be_bytes());
        jpeg.extend_from_slice(&payload);
        jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);

        let result = read_iptc_from_jpeg(&jpeg);
        assert_eq!(result.object_name, Some("After RST".into()));
    }

    #[test]
    fn jpeg_stops_scanning_at_sos_marker() {
        // SOS appears *before* a synthetic APP13 — the scanner must stop and miss it.
        let iim = iim_record(2, 5, b"Past SOS");
        let mut payload = Vec::new();
        payload.extend_from_slice(PHOTOSHOP_HEADER);
        payload.extend_from_slice(&bim_block(0x0404, &iim));
        let seg_len = u16::try_from(2 + payload.len()).unwrap();

        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]); // SOS — stop here
        jpeg.extend_from_slice(&[0xFF, 0xED]);
        jpeg.extend_from_slice(&seg_len.to_be_bytes());
        jpeg.extend_from_slice(&payload);

        assert_eq!(read_iptc_from_jpeg(&jpeg), IptcData::default());
    }

    #[test]
    fn jpeg_skips_stuffed_ff00_bytes() {
        // 0xFF 0x00 inside compressed data is a stuffed byte, not a marker.
        // The scanner's "if data[pos+1] != 0x00" branch should advance one byte and
        // keep going, eventually finding the real APP13.
        let iim = iim_record(2, 25, b"keyword-after-stuffing");
        let mut payload = Vec::new();
        payload.extend_from_slice(PHOTOSHOP_HEADER);
        payload.extend_from_slice(&bim_block(0x0404, &iim));
        let seg_len = u16::try_from(2 + payload.len()).unwrap();

        let mut jpeg = vec![0xFF, 0xD8];
        // Stuffed-byte noise before the APP13 marker.
        jpeg.extend_from_slice(&[0xFF, 0x00, 0xFF, 0x00, 0xAB, 0xCD]);
        jpeg.extend_from_slice(&[0xFF, 0xED]);
        jpeg.extend_from_slice(&seg_len.to_be_bytes());
        jpeg.extend_from_slice(&payload);
        jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x02]);

        let result = read_iptc_from_jpeg(&jpeg);
        assert_eq!(result.keywords, vec!["keyword-after-stuffing"]);
    }

    #[test]
    fn jpeg_read_iptc_dispatches_by_extension() {
        // .jpg / .jpeg → JPEG path; .png/.bmp → unsupported (default).
        let dir = tempfile::tempdir().expect("tmpdir");

        let iim = iim_record(2, 5, b"Disp");
        let jpeg = jpeg_with_photoshop(&bim_block(0x0404, &iim));

        let jpg = dir.path().join("img.jpg");
        std::fs::write(&jpg, &jpeg).unwrap();
        assert_eq!(read_iptc(&jpg).object_name, Some("Disp".into()));

        let jpeg_ext = dir.path().join("img.JPEG"); // case-insensitive ext match
        std::fs::write(&jpeg_ext, &jpeg).unwrap();
        assert_eq!(read_iptc(&jpeg_ext).object_name, Some("Disp".into()));

        let png = dir.path().join("img.png");
        std::fs::write(&png, &jpeg).unwrap();
        assert_eq!(read_iptc(&png), IptcData::default());
    }

    // =========================================================================
    // TIFF parsing
    // =========================================================================

    #[test]
    fn tiff_little_endian_with_iptc_naa_tag() {
        let mut iim = Vec::new();
        iim.extend_from_slice(&iim_record(2, 5, b"LE Title"));
        iim.extend_from_slice(&iim_record(2, 120, b"LE Caption"));
        iim.extend_from_slice(&iim_record(2, 25, b"alpha"));
        let tiff = tiff_with_tag(false, 33723, 1, &iim);

        let result = read_iptc_from_tiff(&tiff);
        assert_eq!(result.object_name, Some("LE Title".into()));
        assert_eq!(result.caption, Some("LE Caption".into()));
        assert_eq!(result.keywords, vec!["alpha"]);
    }

    #[test]
    fn tiff_big_endian_with_iptc_naa_tag() {
        let iim = iim_record(2, 5, b"BE Title");
        let tiff = tiff_with_tag(true, 33723, 7, &iim);
        assert_eq!(
            read_iptc_from_tiff(&tiff).object_name,
            Some("BE Title".into())
        );
    }

    #[test]
    fn tiff_falls_back_to_photoshop_8bim_tag() {
        // No 33723 tag, only 34377 (Photoshop ImageResources containing 8BIM IPTC).
        let iim = iim_record(2, 120, b"Via Photoshop");
        let blob = bim_block(0x0404, &iim);
        let tiff = tiff_with_tag(false, 34377, 1, &blob);
        assert_eq!(
            read_iptc_from_tiff(&tiff).caption,
            Some("Via Photoshop".into())
        );
    }

    #[test]
    fn tiff_invalid_byte_order_returns_default() {
        let mut tiff = vec![b'X', b'Y'];
        tiff.extend_from_slice(&[0u8; 10]);
        assert_eq!(read_iptc_from_tiff(&tiff), IptcData::default());
    }

    #[test]
    fn tiff_invalid_magic_returns_default() {
        // II + magic 41 (not 42) = invalid.
        let mut tiff = vec![b'I', b'I', 41, 0];
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&[0u8; 10]);
        assert_eq!(read_iptc_from_tiff(&tiff), IptcData::default());
    }

    #[test]
    fn tiff_too_small_returns_default() {
        assert_eq!(read_iptc_from_tiff(&[]), IptcData::default());
        assert_eq!(
            read_iptc_from_tiff(&[0xFF, 0xFF, 0xFF]),
            IptcData::default()
        );
    }

    #[test]
    fn tiff_unrelated_tag_returns_default() {
        // Tag 256 (ImageWidth) — not an IPTC source. Parser must skip and return empty.
        let tiff = tiff_with_tag(false, 256, 3, &[0x00, 0x04]);
        assert_eq!(read_iptc_from_tiff(&tiff), IptcData::default());
    }

    #[test]
    fn read_iptc_dispatches_tiff_extension() {
        let dir = tempfile::tempdir().unwrap();
        let iim = iim_record(2, 5, b"From TIFF");
        let tiff = tiff_with_tag(true, 33723, 1, &iim);

        let tif = dir.path().join("photo.tif");
        std::fs::write(&tif, &tiff).unwrap();
        assert_eq!(read_iptc(&tif).object_name, Some("From TIFF".into()));

        let tiff_ext = dir.path().join("photo.TIFF");
        std::fs::write(&tiff_ext, &tiff).unwrap();
        assert_eq!(read_iptc(&tiff_ext).object_name, Some("From TIFF".into()));
    }
}
