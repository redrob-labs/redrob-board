//! A ZIP archive with stored (uncompressed) entries: the fabrication files a board house takes as
//! one upload. Gerbers are small text, so compression buys nothing worth a dependency; every
//! reader accepts method 0.

use std::io::{self, Write};

/// CRC-32 (IEEE), the checksum ZIP entries carry.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn le16(w: &mut Vec<u8>, v: u16) {
    w.extend_from_slice(&v.to_le_bytes());
}
fn le32(w: &mut Vec<u8>, v: u32) {
    w.extend_from_slice(&v.to_le_bytes());
}

/// Write `files` (`(name, bytes)`) as one archive. Names are stored as given (use `/` separators).
pub fn write_zip<W: Write>(mut out: W, files: &[(String, Vec<u8>)]) -> io::Result<()> {
    let mut body = Vec::new();
    let mut central = Vec::new();
    // DOS date/time: 2026-01-01 00:00, fixed so the archive is reproducible.
    let (dos_time, dos_date) = (0u16, ((2026 - 1980) << 9 | 1 << 5 | 1) as u16);
    for (name, data) in files {
        let offset = body.len() as u32;
        let crc = crc32(data);
        let name_b = name.as_bytes();
        // Local file header.
        le32(&mut body, 0x0403_4B50);
        le16(&mut body, 20); // version needed
        le16(&mut body, 0x0800); // UTF-8 names
        le16(&mut body, 0); // stored
        le16(&mut body, dos_time);
        le16(&mut body, dos_date);
        le32(&mut body, crc);
        le32(&mut body, data.len() as u32);
        le32(&mut body, data.len() as u32);
        le16(&mut body, name_b.len() as u16);
        le16(&mut body, 0);
        body.extend_from_slice(name_b);
        body.extend_from_slice(data);
        // Central directory entry.
        le32(&mut central, 0x0201_4B50);
        le16(&mut central, 20);
        le16(&mut central, 20);
        le16(&mut central, 0x0800);
        le16(&mut central, 0);
        le16(&mut central, dos_time);
        le16(&mut central, dos_date);
        le32(&mut central, crc);
        le32(&mut central, data.len() as u32);
        le32(&mut central, data.len() as u32);
        le16(&mut central, name_b.len() as u16);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le16(&mut central, 0);
        le32(&mut central, 0);
        le32(&mut central, offset);
        central.extend_from_slice(name_b);
    }
    let mut end = Vec::new();
    le32(&mut end, 0x0605_4B50);
    le16(&mut end, 0);
    le16(&mut end, 0);
    le16(&mut end, files.len() as u16);
    le16(&mut end, files.len() as u16);
    le32(&mut end, central.len() as u32);
    le32(&mut end, body.len() as u32);
    le16(&mut end, 0);
    out.write_all(&body)?;
    out.write_all(&central)?;
    out.write_all(&end)
}

/// Read a stored-only archive back: `(name, bytes)` per entry, checksums verified.
pub fn read_zip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let u16at = |i: usize| u16::from_le_bytes([bytes[i], bytes[i + 1]]) as usize;
    let u32at = |i: usize| u32::from_le_bytes([bytes[i], bytes[i + 1], bytes[i + 2], bytes[i + 3]]) as usize;
    if bytes.len() < 22 {
        return Err("too short".into());
    }
    let eocd = bytes.len() - 22;
    if u32at(eocd) != 0x0605_4B50 {
        return Err("no end-of-central-directory record".into());
    }
    let n = u16at(eocd + 10);
    let mut p = u32at(eocd + 16);
    let mut out = Vec::new();
    for _ in 0..n {
        if u32at(p) != 0x0201_4B50 {
            return Err("bad central directory entry".into());
        }
        let method = u16at(p + 10);
        if method != 0 {
            return Err(format!("entry uses method {method}, only stored is read here"));
        }
        let crc = u32at(p + 16) as u32;
        let size = u32at(p + 24);
        let name_len = u16at(p + 28);
        let extra = u16at(p + 30);
        let comment = u16at(p + 32);
        let offset = u32at(p + 42);
        let name = String::from_utf8_lossy(&bytes[p + 46..p + 46 + name_len]).to_string();
        let lname = u16at(offset + 26);
        let lextra = u16at(offset + 28);
        let start = offset + 30 + lname + lextra;
        let data = bytes[start..start + size].to_vec();
        if crc32(&data) != crc {
            return Err(format!("{name}: checksum mismatch"));
        }
        out.push((name, data));
        p += 46 + name_len + extra + comment;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vectors() {
        assert_eq!(crc32(b""), 0);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn round_trip() {
        let files = vec![("a/board-F_Cu.gtl".to_string(), b"G04 hi*\nM02*\n".to_vec()), ("bom.csv".to_string(), vec![0u8; 1000])];
        let mut buf = Vec::new();
        write_zip(&mut buf, &files).unwrap();
        assert_eq!(&buf[..4], &[0x50, 0x4B, 0x03, 0x04]);
        assert_eq!(read_zip(&buf).unwrap(), files);
        let mut bad = buf.clone();
        bad[30 + 16 + 2] ^= 1; // a byte inside the first entry's data (30-byte header, 16-byte name)
        assert!(read_zip(&bad).unwrap_err().contains("checksum"));
    }
}
