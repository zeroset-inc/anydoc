//! ZIP archive access with decompression limits.

use crate::error::ConvertError;
use crate::package::limits;
use crate::package::xml::{Element, parse_xml};
use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::rc::Rc;

/// A ZIP-based document package (OOXML, ODF, EPUB).
pub struct Package<'a> {
    zip: zip::ZipArchive<Cursor<&'a [u8]>>,
    total_read: u64,
    /// Decompressed parts by normalized name: repeated references are served
    /// from the cache instead of re-decompressing and re-charging the
    /// total-bytes budget (which would falsely trip on valid documents that
    /// reference one part many times). Bounded by `MAX_TOTAL_BYTES`. Buffers
    /// are shared (`Rc`), so a cache hit never copies the bytes.
    cache: HashMap<String, Rc<[u8]>>,
}

pub(crate) enum AssetPart {
    Missing,
    Loaded(Vec<u8>),
    /// Payload exceeded the application cap; value is an observed lower bound.
    PolicyExceeded(usize),
}

impl<'a> Package<'a> {
    pub fn open(bytes: &'a [u8]) -> Result<Self, ConvertError> {
        let zip = zip::ZipArchive::new(Cursor::new(bytes))
            .map_err(|e| ConvertError::malformed(format!("not a readable zip archive: {e}")))?;
        if zip.len() > limits::MAX_ENTRY_COUNT {
            return Err(ConvertError::ResourceLimit {
                limit: "max_entry_count",
                detail: format!("archive contains {} entries", zip.len()),
            });
        }
        Ok(Package { zip, total_read: 0, cache: HashMap::new() })
    }

    /// Read a part's bytes. `Ok(None)` means the part is absent (a valid
    /// state for optional parts); `Err` means it exists but cannot be read.
    /// Callers apply the unified policy: skip + log when useful output
    /// remains, propagate when the part is the primary content.
    pub fn part(&mut self, name: &str) -> Result<Option<Rc<[u8]>>, ConvertError> {
        // OPC part URIs may carry a leading slash; entries never do.
        let name = name.trim_start_matches('/');
        if let Some(bytes) = self.cache.get(name) {
            return Ok(Some(Rc::clone(bytes)));
        }
        let mut file = match self.zip.by_name(name) {
            Ok(f) => f,
            Err(zip::result::ZipError::FileNotFound) => return Ok(None),
            Err(e) => {
                return Err(ConvertError::Malformed {
                    part: Some(name.to_string()),
                    detail: format!("unreadable archive entry: {e}"),
                });
            }
        };
        if file.size() > limits::MAX_ENTRY_BYTES {
            return Err(ConvertError::ResourceLimit {
                limit: "max_entry_bytes",
                detail: format!("{name} declares {} decompressed bytes", file.size()),
            });
        }
        // The declared size can lie; read through a hard-capped reader. The
        // cap is whichever budget has less room: the per-entry limit or what
        // remains of the whole-archive total.
        let remaining_total = limits::MAX_TOTAL_BYTES.saturating_sub(self.total_read);
        let cap = limits::MAX_ENTRY_BYTES.min(remaining_total);
        let mut bytes = Vec::new();
        let read = (&mut file).take(cap + 1).read_to_end(&mut bytes).map_err(|e| {
            ConvertError::Malformed {
                part: Some(name.to_string()),
                detail: format!("corrupt archive entry: {e}"),
            }
        })? as u64;
        if read > cap {
            return Err(if remaining_total < limits::MAX_ENTRY_BYTES {
                ConvertError::ResourceLimit {
                    limit: "max_total_bytes",
                    detail: format!("{name} exceeds the archive's remaining decompression budget"),
                }
            } else {
                ConvertError::ResourceLimit {
                    limit: "max_entry_bytes",
                    detail: format!("{name} exceeds the decompression cap"),
                }
            });
        }
        self.total_read += read;
        let bytes: Rc<[u8]> = Rc::from(bytes);
        self.cache.insert(name.to_string(), Rc::clone(&bytes));
        Ok(Some(bytes))
    }

    /// True when a part exists, without reading (or budget-charging) it.
    pub fn has_part(&self, name: &str) -> bool {
        self.zip.index_for_name(name.trim_start_matches('/')).is_some()
    }

    /// A part's declared decompressed size without reading its payload.
    pub fn part_size(&mut self, name: &str) -> Result<Option<u64>, ConvertError> {
        let name = name.trim_start_matches('/');
        if let Some(bytes) = self.cache.get(name) {
            return Ok(Some(bytes.len() as u64));
        }
        match self.zip.by_name(name) {
            Ok(file) => Ok(Some(file.size())),
            Err(zip::result::ZipError::FileNotFound) => Ok(None),
            Err(e) => Err(ConvertError::Malformed {
                part: Some(name.to_string()),
                detail: format!("unreadable archive entry: {e}"),
            }),
        }
    }

    /// Read a part that must exist for any meaningful output.
    pub fn required_part(&mut self, name: &str) -> Result<Rc<[u8]>, ConvertError> {
        self.part(name)?.ok_or_else(|| ConvertError::MissingPart { part: name.to_string() })
    }

    /// Read an optional part under the unified recovery policy: absent is a
    /// valid state (`Ok(None)`, silent); an unreadable part is skipped with a
    /// log (`Ok(None)`); fatal resource-limit errors always propagate.
    pub fn optional_part(&mut self, name: &str) -> Result<Option<Rc<[u8]>>, ConvertError> {
        match self.part(name) {
            Ok(bytes) => Ok(bytes),
            Err(e) if e.is_fatal() => Err(e),
            Err(e) => {
                log::warn!("skipping unreadable part {name}: {e}");
                Ok(None)
            }
        }
    }

    /// Read an asset into an owned buffer without placing a second copy in
    /// the package cache. An application cap bounds even forged ZIP sizes.
    pub(crate) fn asset_part(
        &mut self,
        name: &str,
        application_cap: Option<usize>,
    ) -> Result<AssetPart, ConvertError> {
        let result = self.read_asset_part(name, application_cap);
        match result {
            Ok(result) => Ok(result),
            Err(e) if e.is_fatal() => Err(e),
            Err(e) => {
                log::warn!("skipping unreadable part {name}: {e}");
                Ok(AssetPart::Missing)
            }
        }
    }

    fn read_asset_part(
        &mut self,
        name: &str,
        application_cap: Option<usize>,
    ) -> Result<AssetPart, ConvertError> {
        let name = name.trim_start_matches('/');
        if let Some(bytes) = self.cache.get(name) {
            return Ok(match application_cap {
                Some(cap) if bytes.len() > cap => AssetPart::PolicyExceeded(cap + 1),
                _ => AssetPart::Loaded(bytes.to_vec()),
            });
        }
        let mut file = match self.zip.by_name(name) {
            Ok(file) => file,
            Err(zip::result::ZipError::FileNotFound) => return Ok(AssetPart::Missing),
            Err(e) => {
                return Err(ConvertError::Malformed {
                    part: Some(name.to_string()),
                    detail: format!("unreadable archive entry: {e}"),
                });
            }
        };
        if file.size() > limits::MAX_ENTRY_BYTES {
            return Err(ConvertError::ResourceLimit {
                limit: "max_entry_bytes",
                detail: format!("{name} declares {} decompressed bytes", file.size()),
            });
        }
        let remaining_total = limits::MAX_TOTAL_BYTES.saturating_sub(self.total_read);
        let hard_cap = limits::MAX_ENTRY_BYTES.min(remaining_total);
        let cap = application_cap.map_or(hard_cap, |limit| hard_cap.min(limit as u64));
        let mut bytes = Vec::new();
        let read = (&mut file).take(cap + 1).read_to_end(&mut bytes).map_err(|e| {
            ConvertError::Malformed {
                part: Some(name.to_string()),
                detail: format!("corrupt archive entry: {e}"),
            }
        })? as u64;
        self.total_read = self.total_read.saturating_add(read);
        if read > cap {
            if application_cap.is_some_and(|limit| (limit as u64) < hard_cap) {
                return Ok(AssetPart::PolicyExceeded(read as usize));
            }
            return Err(if remaining_total < limits::MAX_ENTRY_BYTES {
                ConvertError::ResourceLimit {
                    limit: "max_total_bytes",
                    detail: format!("{name} exceeds the archive's remaining decompression budget"),
                }
            } else {
                ConvertError::ResourceLimit {
                    limit: "max_entry_bytes",
                    detail: format!("{name} exceeds the decompression cap"),
                }
            });
        }
        Ok(AssetPart::Loaded(bytes))
    }

    /// Read and parse an optional XML part under the unified recovery policy:
    /// absent -> `Ok(None)`; unreadable or corrupt -> skipped with a log;
    /// fatal resource-limit errors always propagate.
    pub fn optional_xml_part(&mut self, name: &str) -> Result<Option<Element>, ConvertError> {
        let Some(bytes) = self.optional_part(name)? else {
            return Ok(None);
        };
        match parse_xml(&bytes) {
            Ok(tree) => Ok(Some(tree)),
            Err(e) if e.is_fatal() => Err(e),
            Err(e) => {
                log::warn!("skipping corrupt part {name}: {e}");
                Ok(None)
            }
        }
    }

    /// Read and parse an XML part that must exist and parse for any
    /// meaningful output.
    pub fn required_xml_part(&mut self, name: &str) -> Result<Element, ConvertError> {
        let bytes = self.required_part(name)?;
        parse_xml(&bytes)
    }
}

/// A zip-open failure on OOXML input may actually be an OLE compound file:
/// an encrypted package, or a legacy binary document with the wrong
/// extension.
pub fn probe_ole(bytes: &[u8]) -> Option<ConvertError> {
    const OLE_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];
    if !bytes.starts_with(&OLE_MAGIC) {
        return None;
    }
    let cursor = Cursor::new(bytes);
    if let Ok(file) = cfb::CompoundFile::open(cursor)
        && (file.exists("EncryptionInfo") || file.exists("EncryptedPackage"))
    {
        return Some(ConvertError::Encrypted);
    }
    Some(ConvertError::malformed(
        "OLE compound document where an OOXML package was expected (legacy binary format?)",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn one_part_zip(name: &str, bytes: &[u8]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(bytes).unwrap();
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn repeated_reads_are_cached_and_charged_once() {
        let data = one_part_zip("media/a.bin", &[7u8; 4096]);
        let mut pkg = Package::open(&data).unwrap();
        for _ in 0..5 {
            assert_eq!(pkg.part("media/a.bin").unwrap().unwrap().len(), 4096);
        }
        assert_eq!(pkg.total_read, 4096, "repeated reads must not re-charge the budget");
    }

    #[test]
    fn total_budget_exhaustion_reports_max_total_bytes() {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for name in ["a.bin", "b.bin"] {
            w.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            w.write_all(&[7u8; 4096]).unwrap();
        }
        let data = w.finish().unwrap().into_inner();
        let mut pkg = Package::open(&data).unwrap();
        assert!(pkg.part("a.bin").unwrap().is_some());
        // Simulate a large archive having consumed almost the whole total
        // budget across earlier entries; the next entry no longer fits.
        pkg.total_read = limits::MAX_TOTAL_BYTES - 100;
        let err = pkg.part("b.bin").unwrap_err();
        assert!(
            matches!(err, ConvertError::ResourceLimit { limit: "max_total_bytes", .. }),
            "expected max_total_bytes, got: {err}"
        );
    }

    #[test]
    fn leading_slash_part_names_normalize() {
        let data = one_part_zip("word/document.xml", b"<x/>");
        let mut pkg = Package::open(&data).unwrap();
        assert!(pkg.part("/word/document.xml").unwrap().is_some());
    }

    #[test]
    fn asset_read_cap_handles_a_forged_declared_size_without_large_allocation() {
        let payload = vec![7u8; 4096];
        let mut data = one_part_zip("media/a.bin", &payload);
        // Forge both local and central uncompressed-size fields downward.
        // The deflate stream still expands to 4096 bytes.
        let local = data.windows(4).position(|w| w == b"PK\x03\x04").unwrap();
        data[local + 22..local + 26].copy_from_slice(&1u32.to_le_bytes());
        let central = data.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
        data[central + 24..central + 28].copy_from_slice(&1u32.to_le_bytes());

        let mut pkg = Package::open(&data).unwrap();
        assert_eq!(pkg.part_size("media/a.bin").unwrap(), Some(1));
        let result = pkg.asset_part("media/a.bin", Some(8)).unwrap();
        assert!(matches!(result, AssetPart::PolicyExceeded(9)));
        assert_eq!(pkg.total_read, 9, "only cap + one byte should be decompressed");
    }
}
