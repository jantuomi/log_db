use std::cmp::Ordering;
use std::collections::HashSet;
use std::fmt::Display;
use std::fs::{metadata, File};
use std::io::{self};
use std::path::{Path, PathBuf};
use uuid::Uuid;

// For Unix-like systems
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

// For Windows
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;

pub const ACTIVE_SYMLINK_FILENAME: &str = "active";
pub const METADATA_FILE_HEADER_SIZE: usize = 24;
pub const EXCL_LOCK_REQUEST_FILENAME: &str = "excl_lock_req";
pub const INIT_LOCK_FILENAME: &str = "init_lock";
pub const DEFAULT_READ_BUF_SIZE: usize = 1024 * 1024; // 1 MB
pub const TEST_RESOURCES_DIR: &str = "tests/resources";

#[derive(Debug, Eq, PartialEq)]
pub enum SpecialSequence {
    RecordSeparator,
    LiteralFieldSeparator,
    LiteralEscape,
}

/// LogKey is a packed struct that contains:
/// - a log segment number (16 bits)
/// - a log index within the segment (48 bits)
#[derive(Debug, Clone, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct LogKey(u64);

impl LogKey {
    pub fn new(segment_num: u16, index: u64) -> Self {
        assert!(index < (1 << 48), "Index must fit in 48 bits");
        LogKey((segment_num as u64) << 48 | index)
    }

    pub fn segment_num(&self) -> u16 {
        (self.0 >> 48) as u16
    }

    pub fn index(&self) -> u64 {
        self.0 & 0x0000_FFFF_FFFF_FFFF
    }
}

/// LogKeySet is a non-empty set of LogKeys.
#[derive(Debug, Clone, Eq, PartialEq)]
pub struct LogKeySet {
    set: HashSet<LogKey>,
}

impl PartialOrd for LogKeySet {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let self_max_elem = self.set.iter().max()?;
        let other_max_elem = other.set.iter().max()?;
        Some(self_max_elem.cmp(other_max_elem))
    }
}

impl LogKeySet {
    /// Create a new LogKeySet with an initial LogKey.
    /// The initial LogKey is required since LogKeySet must be non-empty.
    pub fn new_with_initial(key: &LogKey) -> Self {
        let mut set = HashSet::new();
        set.insert(key.clone());
        LogKeySet { set }
    }

    pub fn from_slice(keys: &[LogKey]) -> Self {
        assert!(
            !keys.is_empty(),
            "LogKeySet::from_slice must be supplied a non-empty slice"
        );
        let mut set = HashSet::with_capacity(keys.len());
        keys.iter().for_each(|key| {
            set.insert(key.clone());
        });
        LogKeySet { set }
    }

    pub fn iter(&self) -> std::collections::hash_set::Iter<'_, LogKey> {
        self.set.iter()
    }

    /// The number of LogKeys in the set.
    pub fn len(&self) -> usize {
        self.set.len()
    }

    /// Insert a LogKey into the set.
    pub fn insert(&mut self, key: LogKey) {
        self.set.insert(key);
    }

    /// Remove a LogKey from the set. Return Ok(()) if the key was found and removed.
    /// Return io::Error::InvalidInput if trying to remove the last element.
    /// Return io::Error::NotFound if the key was not found.
    pub fn remove(&mut self, key: &LogKey) -> Result<(), io::Error> {
        if self.set.len() == 1 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Cannot remove the last element from LogKeySet",
            ));
        }
        let removed = self.set.remove(key);

        if !removed {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "LogKey not found in LogKeySet",
            ));
        }

        assert!(
            self.set.len() > 0,
            "LogKeySet should not be empty after removal"
        );

        Ok(())
    }

    /// Get a reference to the set of LogKeys.
    pub fn log_keys(&self) -> &HashSet<LogKey> {
        &self.set
    }
}

impl Ord for LogKeySet {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other)
            .expect("LogKeySet comparison failed, possibly due to empty set")
    }
}

pub struct MetadataHeader {
    pub version: u8,
    pub uuid: Uuid,
}

const METADATA_HEADER_PADDING: &[u8] = &[0; 7];
impl MetadataHeader {
    pub fn serialize(&self) -> Vec<u8> {
        let uuid_bytes = self.uuid.as_bytes().to_vec();

        let mut header = vec![self.version];
        header.extend(METADATA_HEADER_PADDING);
        header.extend(uuid_bytes);

        assert_eq!(header.len(), METADATA_FILE_HEADER_SIZE);

        header
    }

    pub fn deserialize(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len(), METADATA_FILE_HEADER_SIZE);

        let version = bytes[0];
        let uuid = Uuid::from_slice(&bytes[8..24]).expect("Failed to deserialize Uuid");

        MetadataHeader { version, uuid }
    }
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
                bytes.extend(&i.to_be_bytes());
                bytes
            }
            RecordValue::Float(f) => {
                let mut bytes = vec![2]; // Tag for Float
                bytes.extend(&f.to_be_bytes());
                bytes
            }
            RecordValue::String(s) => {
                let mut bytes = vec![3]; // Tag for String
                let length = s.len() as u64;
                bytes.extend(&length.to_be_bytes());
                bytes.extend(s.as_bytes());
                bytes
            }
            RecordValue::Bytes(b) => {
                let mut bytes = vec![4]; // Tag for Bytes
                let length = b.len() as u64;
                bytes.extend(&length.to_be_bytes());
                bytes.extend(b);
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

/// A path to a log segment file along with its type
pub enum SegmentPath {
    /// A symbolic link to the active log file
    ActiveSymlink(String),
    /// A compacted segment that is no longer being written to
    Compacted(String),
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

pub fn symlink(original: &Path, link: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(original, link)
    }

    #[cfg(windows)]
    {
        std::os::windows::fs::symlink_file(original, link)
    }
}
