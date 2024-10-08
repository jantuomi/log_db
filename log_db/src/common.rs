use std::fmt::Display;
use std::fs::{metadata, File};
use std::io::{self};
use std::path::PathBuf;

// For Unix-like systems
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

// For Windows
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

pub const ACTIVE_LOG_FILENAME: &str = "db";
pub const EXCL_LOCK_REQUEST_FILENAME: &str = "excl_lock_req";
pub const DEFAULT_READ_BUF_SIZE: usize = 1024 * 1024; // 1 MB
pub const FIELD_SEPARATOR: u8 = b'\x1C';
pub const ESCAPE_CHARACTER: u8 = b'\x1D';
pub const TEST_RESOURCES_DIR: &str = "tests/resources";

// Special sequences. Note: these must have the same length!
// Since the log is read both forwards and backwards, we must have a signal
// character (ESCAPE_CHARACTER) on both sides of the special sequence.
pub const SEQ_RECORD_SEP: &[u8] = &[
    ESCAPE_CHARACTER,
    FIELD_SEPARATOR,
    FIELD_SEPARATOR,
    ESCAPE_CHARACTER,
];
pub const SEQ_LIT_ESCAPE: &[u8] = &[
    ESCAPE_CHARACTER,
    ESCAPE_CHARACTER,
    ESCAPE_CHARACTER,
    ESCAPE_CHARACTER,
];
pub const SEQ_LIT_FIELD_SEP: &[u8] = &[
    ESCAPE_CHARACTER,
    ESCAPE_CHARACTER,
    FIELD_SEPARATOR,
    ESCAPE_CHARACTER,
];

/// There are three special sequences that need to be handled:
/// Here: SC = escape char, FS = field separator.
/// - SC FS FS SC  -> actual record separator
/// - SC SC FS SC  -> literal FS
/// - SC SC SC SC  -> literal SC
///
/// Returns SpecialSequence or None if not valid.
pub fn validate_special(buf: &[u8]) -> Option<SpecialSequence> {
    match buf {
        SEQ_RECORD_SEP => Some(SpecialSequence::RecordSeparator),
        SEQ_LIT_FIELD_SEP => Some(SpecialSequence::LiteralFieldSeparator),
        SEQ_LIT_ESCAPE => Some(SpecialSequence::LiteralEscape),
        _ => None,
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum SpecialSequence {
    RecordSeparator,
    LiteralFieldSeparator,
    LiteralEscape,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum MemtableEvictPolicy {
    LeastWritten,
    LeastRead,
    LeastReadOrWritten,
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub enum WriteDurability {
    /// Changes are written to an application-level write buffer without flushing to the OS write buffer or syncing to disk.
    /// The buffered writer will batch writes to the OS buffer for maximum performance.
    /// Offers the lowest durability guarantees but is very fast.
    Async,
    /// Changes are written to the OS write buffer but not immediately synced to disk.
    /// Offers better durability guarantees than Async but is slower.
    /// This is generally recommended. Most OSes will sync the write buffer to disk within a few seconds.
    Flush,
    /// Changes are written to the OS write buffer and synced to disk immediately.
    /// Offers the best durability guarantees but is the slowest.
    FlushSync,
}

impl Display for WriteDurability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        write!(f, "{:?}", self)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub enum IndexableValue {
    Int(i64),
    String(String),
}

#[derive(Debug, Clone)]
pub enum RecordFieldType {
    Int,
    Float,
    String,
    Bytes,
}

#[derive(Debug, Clone)]
pub struct RecordField {
    pub field_type: RecordFieldType,
    pub nullable: bool,
}

impl RecordField {
    pub fn int() -> Self {
        RecordField {
            field_type: RecordFieldType::Int,
            nullable: false,
        }
    }

    pub fn float() -> Self {
        RecordField {
            field_type: RecordFieldType::Float,
            nullable: false,
        }
    }

    pub fn string() -> Self {
        RecordField {
            field_type: RecordFieldType::String,
            nullable: false,
        }
    }

    pub fn bytes() -> Self {
        RecordField {
            field_type: RecordFieldType::Bytes,
            nullable: false,
        }
    }

    pub fn nullable(&mut self) -> Self {
        let mut new = self.clone();
        new.nullable = true;
        new
    }
}

#[derive(Debug, Clone)]
pub enum RecordValue {
    Null,
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
}

impl RecordValue {
    pub fn serialize(&self) -> Vec<u8> {
        match self {
            RecordValue::Null => {
                vec![0] // Tag for Null
            }
            RecordValue::Int(i) => {
                let mut bytes = vec![1]; // Tag for Int
                let data_bytes = escape_bytes(&i.to_be_bytes());
                bytes.extend(&data_bytes);
                bytes
            }
            RecordValue::Float(f) => {
                let mut bytes = vec![2]; // Tag for Float
                let data_bytes = escape_bytes(&f.to_be_bytes());
                bytes.extend(&data_bytes);
                bytes
            }
            RecordValue::String(s) => {
                let mut bytes = vec![3]; // Tag for String
                let length = s.len() as u64;
                let length_bytes = escape_bytes(&length.to_be_bytes());
                bytes.extend(&length_bytes);
                let data_bytes = escape_bytes(s.as_bytes());
                bytes.extend(&data_bytes);
                bytes
            }
            RecordValue::Bytes(b) => {
                let mut bytes = vec![4]; // Tag for Bytes
                let length = b.len() as u64;
                let length_bytes = escape_bytes(&length.to_be_bytes());
                bytes.extend(&length_bytes);
                let data_bytes = escape_bytes(b);
                bytes.extend(&data_bytes);
                bytes
            }
        }
    }

    /// Deserialize a RecordValue from a byte slice.
    /// Returns the deserialized RecordValue and the number of bytes consumed.
    pub fn deserialize(bytes: &[u8]) -> (RecordValue, usize) {
        match bytes[0] {
            0 => (RecordValue::Null, 1),
            1 => {
                let mut int_bytes = [0; 8];
                int_bytes.copy_from_slice(&bytes[1..1 + 8]);
                (RecordValue::Int(i64::from_be_bytes(int_bytes)), 1 + 8)
            }
            2 => {
                let mut float_bytes = [0; 8];
                float_bytes.copy_from_slice(&bytes[1..1 + 8]);
                (RecordValue::Float(f64::from_be_bytes(float_bytes)), 1 + 8)
            }
            3 => {
                let length_bytes = &bytes[1..1 + 8];
                let length = u64::from_be_bytes(length_bytes.try_into().unwrap()) as usize;
                (
                    RecordValue::String(
                        String::from_utf8(bytes[1 + 8..1 + 8 + length].to_vec()).unwrap(),
                    ),
                    1 + 8 + length,
                )
            }
            4 => {
                let length_bytes = &bytes[1..1 + 8];
                let length = u64::from_be_bytes(length_bytes.try_into().unwrap()) as usize;
                (
                    RecordValue::Bytes(bytes[1 + 8..1 + 8 + length].to_vec()),
                    1 + 8 + length,
                )
            }
            _ => panic!("Invalid tag: {}", bytes[0]),
        }
    }

    pub fn as_indexable(&self) -> Option<IndexableValue> {
        match self {
            RecordValue::Int(i) => Some(IndexableValue::Int(*i)),
            RecordValue::String(s) => Some(IndexableValue::String(s.clone())),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Record {
    pub values: Vec<RecordValue>,
}

impl Record {
    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in &self.values {
            bytes.extend(value.serialize());
        }
        bytes
    }

    pub fn deserialize(bytes: &[u8]) -> Record {
        let mut values = Vec::new();
        let mut start = 0;
        while start < bytes.len() {
            let (rv, consumed) = RecordValue::deserialize(&bytes[start..]);
            values.push(rv);
            start += consumed;
        }
        Record { values }
    }
}

pub fn escape_bytes(buf: &[u8]) -> Vec<u8> {
    let mut result = Vec::new();
    for byte in buf {
        match byte {
            &FIELD_SEPARATOR => {
                result.extend(SEQ_LIT_FIELD_SEP);
            }
            &ESCAPE_CHARACTER => {
                result.extend(SEQ_LIT_ESCAPE);
            }
            _ => result.push(*byte),
        }
    }
    result
}

pub fn is_file_same_as_path(file: &File, path: &PathBuf) -> io::Result<bool> {
    // Get the metadata for the open file handle
    let file_metadata = file.metadata()?;

    // Get the metadata for the file at the specified path
    let path_metadata = metadata(path)?;

    // Platform-specific comparison
    #[cfg(unix)]
    {
        Ok(
            file_metadata.dev() == path_metadata.dev()
                && file_metadata.ino() == path_metadata.ino(),
        )
    }

    #[cfg(windows)]
    {
        Ok(file_metadata.file_index() == path_metadata.file_index()
            && file_metadata.volume_serial_number() == path_metadata.volume_serial_number())
    }
}
