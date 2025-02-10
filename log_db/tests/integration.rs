#[macro_use]
extern crate log;
extern crate ctor;
extern crate tempfile;

use ctor::ctor;
use env_logger;
use log_db::*;
use serial_test::serial;
use std::fs::{self};
use std::path::Path;
use std::thread;
use std::time::Duration;
use tempfile::tempdir;

pub fn tmp_dir() -> String {
    let dir = tempdir()
        .expect("Failed to create temporary directory")
        .path()
        .to_str()
        .expect("Failed to convert temporary directory path to string")
        .to_string();
    fs::create_dir_all(&dir).expect("Failed to create temporary directory");
    dir
}

#[ctor]
fn init_logger() {
    let _ = env_logger::builder().is_test(true).try_init();
}

#[derive(Eq, PartialEq, Clone, Debug)]
enum Field {
    Id,
    Name,
    Data,
}

#[derive(Debug)]
struct Inst {
    pub id: i64,
    pub name: Option<String>,
    pub data: Vec<u8>,
}

impl Inst {
    fn schema() -> Vec<(Field, Type)> {
        vec![
            (Field::Id, Type::int()),
            (Field::Name, Type::string().nullable()),
            (Field::Data, Type::bytes()),
        ]
    }
    fn primary_key() -> Field {
        Field::Id
    }
    fn secondary_keys() -> Vec<Field> {
        vec![Field::Name]
    }

    fn into_record(self) -> Vec<Value> {
        vec![
            Value::Int(self.id),
            match self.name {
                Some(name) => Value::String(name),
                None => Value::Null,
            },
            Value::Bytes(self.data),
        ]
    }

    fn from_record(record: Vec<Value>) -> Self {
        let mut it = record.into_iter();

        Inst {
            id: match it.next().unwrap() {
                Value::Int(id) => id,
                other => panic!("Invalid value type: {:?}", other),
            },
            name: match it.next().unwrap() {
                Value::String(name) => Some(name),
                Value::Null => None,
                other => panic!("Invalid value type: {:?}", other),
            },
            data: match it.next().unwrap() {
                Value::Bytes(data) => data,
                other => panic!("Invalid value type: {:?}", other),
            },
        }
    }
}

#[test]
fn test_initialize_only() {
    let data_dir = tmp_dir();
    let _db = DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");
}

#[test]
fn test_upsert_and_get_with_primary_memtable() {
    let data_dir = tmp_dir();
    let mut db = DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    let id = 1;
    let inst = Inst {
        id,
        name: Some("Alice".to_string()),
        data: vec![0, 1, 2],
    };
    db.upsert(inst).unwrap();

    let result = db.get(&Value::Int(1)).unwrap().unwrap();

    // Check that the IDs match
    assert!(result.id == id);
}

#[test]
fn test_upsert_and_get() {
    let data_dir = tmp_dir();
    let mut db = DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    // Insert some records
    db.upsert(Inst {
        id: 0,
        name: None,
        data: vec![3, 4, 5],
    })
    .unwrap();

    db.upsert(Inst {
        id: 1,
        name: Some("Alice".to_string()),
        data: vec![0, 1, 2],
    })
    .unwrap();

    db.upsert(Inst {
        id: 1,
        name: Some("Bob".to_string()),
        data: vec![0, 1, 2],
    })
    .unwrap();

    db.upsert(Inst {
        id: 2,
        name: Some("George".to_string()),
        data: vec![],
    })
    .unwrap();

    // Get with ID = 0
    let result = db.get(&Value::Int(0)).unwrap().unwrap();

    // Should match id == 0
    assert!(result.id == 0);
    assert!(result.name == None);

    // Get with ID = 1
    let result = db.get(&Value::Int(1)).unwrap().unwrap();

    // Should match newest inst with id == 1
    assert!(result.id == 1);
    assert!(result.name == Some("Bob".to_owned()));
}

#[test]
fn test_get_nonexistant() {
    let data_dir = tmp_dir();
    let mut db = DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    let result = db.get(&Value::Int(0)).unwrap();
    assert!(result.is_none());
}

struct InstTestNullable {}
impl InstTestNullable {
    fn schema() -> Vec<(Field, Type)> {
        vec![(Field::Id, Type::int())]
    }
    fn primary_key() -> Field {
        Field::Id
    }
    fn secondary_keys() -> Vec<Field> {
        vec![]
    }

    fn into_record(self) -> Vec<Value> {
        vec![Value::Null]
    }

    fn from_record(_record: Vec<Value>) -> Self {
        Self {}
    }
}

#[test]
fn test_upsert_fails_on_null_in_non_nullable_field() {
    let data_dir = tmp_dir();
    let mut db = DB::configure()
        .schema(InstTestNullable::schema())
        .primary_key(InstTestNullable::primary_key())
        .secondary_keys(InstTestNullable::secondary_keys())
        .from_record(InstTestNullable::from_record)
        .into_record(InstTestNullable::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    // Null value
    assert!(db.upsert(InstTestNullable {}).is_err());
}

struct InstTestNumValues {}
impl InstTestNumValues {
    fn schema() -> Vec<(Field, Type)> {
        vec![(Field::Id, Type::int()), (Field::Name, Type::string())]
    }
    fn primary_key() -> Field {
        Field::Id
    }
    fn secondary_keys() -> Vec<Field> {
        vec![]
    }

    fn into_record(self) -> Vec<Value> {
        vec![Value::Int(0)]
    }

    fn from_record(_record: Vec<Value>) -> Self {
        Self {}
    }
}

#[test]
fn test_upsert_fails_on_invalid_number_of_values() {
    let data_dir = tmp_dir();
    let mut db = DB::configure()
        .schema(InstTestNumValues::schema())
        .primary_key(InstTestNumValues::primary_key())
        .secondary_keys(InstTestNumValues::secondary_keys())
        .from_record(InstTestNumValues::from_record)
        .into_record(InstTestNumValues::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    // Missing values
    assert!(db.upsert(InstTestNumValues {}).is_err());
}

struct InstTestInvalidType {}
impl InstTestInvalidType {
    fn schema() -> Vec<(Field, Type)> {
        vec![(Field::Id, Type::int())]
    }
    fn primary_key() -> Field {
        Field::Id
    }
    fn secondary_keys() -> Vec<Field> {
        vec![]
    }

    fn into_record(self) -> Vec<Value> {
        vec![Value::String("foo".to_string())]
    }

    fn from_record(_record: Vec<Value>) -> Self {
        Self {}
    }
}

#[test]
fn test_upsert_fails_on_invalid_value_type() {
    let data_dir = tmp_dir();
    let mut db = DB::configure()
        .schema(InstTestInvalidType::schema())
        .primary_key(InstTestInvalidType::primary_key())
        .secondary_keys(InstTestInvalidType::secondary_keys())
        .from_record(InstTestInvalidType::from_record)
        .into_record(InstTestInvalidType::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    // Invalid type
    assert!(db.upsert(InstTestInvalidType {}).is_err());
}

#[test]
fn test_upsert_and_find_by() {
    let data_dir = tmp_dir();
    let mut db = DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    // Insert some records
    db.upsert(Inst {
        id: 0,
        name: Some("John".to_string()),
        data: vec![3, 4, 5],
    })
    .unwrap();

    db.upsert(Inst {
        id: 1,
        name: Some("John".to_string()),
        data: vec![1, 2, 3],
    })
    .unwrap();

    db.upsert(Inst {
        id: 2,
        name: Some("George".to_string()),
        data: vec![1, 2, 3],
    })
    .unwrap();

    // There should be 2 Johns
    let johns = db
        .find_by(&Field::Name, &Value::String("John".to_string()))
        .expect("Failed to find all Johns");

    assert_eq!(johns.len(), 2);
}

struct InstSingleId {
    pub id: i64,
}

impl InstSingleId {
    fn schema() -> Vec<(Field, Type)> {
        vec![(Field::Id, Type::int())]
    }
    fn primary_key() -> Field {
        Field::Id
    }
    fn secondary_keys() -> Vec<Field> {
        vec![]
    }

    fn into_record(self) -> Vec<Value> {
        vec![Value::Int(self.id)]
    }

    fn from_record(record: Vec<Value>) -> Self {
        Self {
            id: match record[0] {
                Value::Int(id) => id,
                _ => panic!("Invalid value type"),
            },
        }
    }
}

#[test]
#[serial]
fn test_multiple_writing_threads() {
    let data_dir = tmp_dir();
    debug!("Data dir: {:?}", data_dir);
    let mut threads = vec![];
    let threads_n = 100;

    for i in 0..threads_n {
        let data_dir = data_dir.clone();
        threads.push(thread::spawn(move || {
            let mut db = DB::configure()
                .schema(InstSingleId::schema())
                .primary_key(InstSingleId::primary_key())
                .secondary_keys(InstSingleId::secondary_keys())
                .from_record(InstSingleId::from_record)
                .into_record(InstSingleId::into_record)
                .data_dir(&data_dir)
                .initialize()
                .expect("Failed to initialize DB instance");

            db.upsert(InstSingleId { id: i })
                .expect("Failed to upsert record");
        }));
    }

    for thread in threads {
        thread.join().expect("Failed to join thread");
    }

    // Read the records
    let mut db = DB::configure()
        .schema(InstSingleId::schema())
        .primary_key(InstSingleId::primary_key())
        .secondary_keys(InstSingleId::secondary_keys())
        .from_record(InstSingleId::from_record)
        .into_record(InstSingleId::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    for i in 0..threads_n {
        let result = db
            .get(&Value::Int(i))
            .expect("Failed to get record")
            .expect("Record not found");

        assert!(result.id == i);
    }
}

#[test]
#[serial]
fn test_one_writer_and_multiple_reading_threads() {
    let data_dir = tmp_dir();
    let mut threads = vec![];
    let threads_n = 100;

    // Add readers that poll for the records
    for i in 0..threads_n {
        let data_dir = data_dir.clone();
        threads.push(thread::spawn(move || {
            let mut db = DB::configure()
                .schema(InstSingleId::schema())
                .primary_key(InstSingleId::primary_key())
                .secondary_keys(InstSingleId::secondary_keys())
                .from_record(InstSingleId::from_record)
                .into_record(InstSingleId::into_record)
                .data_dir(&data_dir)
                .segment_size(1000) // should cause rotations
                .initialize()
                .expect("Failed to initialize DB instance");

            let mut timeout = 5;
            loop {
                let result = db.get(&Value::Int(i)).expect("Failed to get record");
                match result {
                    None => {
                        thread::sleep(Duration::from_millis(timeout));
                        timeout = std::cmp::min(timeout * 2, 100);
                        continue;
                    }
                    Some(result) => {
                        assert!(result.id == i);
                        break;
                    }
                };
            }
        }));
    }

    // Add a writer that inserts the records
    threads.push(thread::spawn(move || {
        let mut db = DB::configure()
            .schema(InstSingleId::schema())
            .primary_key(InstSingleId::primary_key())
            .secondary_keys(InstSingleId::secondary_keys())
            .from_record(InstSingleId::from_record)
            .into_record(InstSingleId::into_record)
            .data_dir(&data_dir)
            .initialize()
            .expect("Failed to initialize DB instance");

        for i in 0..threads_n {
            db.upsert(InstSingleId { id: i })
                .expect("Failed to upsert record");

            db.do_maintenance_tasks() // Run maintenance tasks after every write, just to test it
                .expect("Failed to do maintenance tasks");
        }
    }));

    for thread in threads {
        thread.join().expect("Failed to join thread");
    }
}

#[test]
fn test_log_is_rotated_when_capacity_reached() {
    let data_dir = tmp_dir();
    let data_dir_path = Path::new(&data_dir);

    // Hand-calculated record length, find record below
    let record_len = 1 // tombstone tag
        + (1 + 8)      // int tag + i64
        + (1 + 8 + 4)  // string tag + string length + string data
        + (1 + 8 + 3); // bytes tag + bytes length + bytes data

    let mut db = DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .segment_size(10 * record_len) // small log segment size
        .initialize()
        .expect("Failed to initialize DB instance");

    // Insert more records than fits the capacity
    for _ in 0..25 {
        db.upsert(Inst {
            id: 0,
            name: Some("John".to_string()),
            data: vec![3, 4, 5],
        })
        .expect("Failed to upsert record");

        db.do_maintenance_tasks()
            .expect("Failed to do maintenance tasks");
    }

    // Check that the rotated segments exist
    assert!(data_dir_path.join("metadata").with_extension("1").exists());
    assert!(data_dir_path.join("metadata").with_extension("2").exists());

    // 3rd segment should not exist (note negation)
    assert!(!data_dir_path.join("metadata").with_extension("3").exists());
}

#[test]
fn test_delete() {
    let data_dir = tmp_dir();
    let mut db = DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    // Insert some records
    db.upsert(Inst {
        id: 0,
        name: Some("John".to_string()),
        data: vec![3, 4, 5],
    })
    .unwrap();

    db.upsert(Inst {
        id: 1,
        name: Some("John".to_string()),
        data: vec![1, 2, 3],
    })
    .unwrap();

    db.delete(&Value::Int(0)).unwrap();

    // Check that the record is deleted
    assert!(db.get(&Value::Int(0)).unwrap().is_none());

    // Check that the secondary index is updated
    assert_eq!(
        db.find_by(&Field::Name, &Value::String("John".to_string()))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn test_delete_by() {
    let data_dir = tmp_dir();
    let mut db = DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    // Insert some records
    db.upsert(Inst {
        id: 0,
        name: Some("John".to_string()),
        data: vec![3, 4, 5],
    })
    .unwrap();

    db.upsert(Inst {
        id: 1,
        name: Some("John".to_string()),
        data: vec![1, 2, 3],
    })
    .unwrap();

    db.upsert(Inst {
        id: 2,
        name: Some("Bob".to_string()),
        data: vec![1, 2, 3],
    })
    .unwrap();

    db.delete_by(&Field::Name, &Value::String("John".to_string()))
        .unwrap();

    // Check that the record is deleted
    assert!(db.get(&Value::Int(0)).unwrap().is_none());
    assert!(db.get(&Value::Int(1)).unwrap().is_none());

    // Check that the secondary index is updated
    assert_eq!(
        db.find_by(&Field::Name, &Value::String("John".to_string()))
            .unwrap()
            .len(),
        0
    );
    assert_eq!(
        db.find_by(&Field::Name, &Value::String("Bob".to_string()))
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn test_range_by_id() {
    let data_dir = tmp_dir();
    let mut db = DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    let inserted_ids = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

    for id in inserted_ids.iter() {
        db.upsert(Inst {
            id: *id,
            name: Some("Foobar".to_string()),
            data: vec![],
        })
        .unwrap();
    }

    // Test range [3, 7)
    let received = db
        .range_by(&Field::Id, &Value::Int(3)..&Value::Int(7))
        .unwrap();
    let received_ids: Vec<i64> = received.iter().map(|inst| inst.id).collect();

    assert_eq!(received_ids, vec![3, 4, 5, 6]);

    // Test range [3, 7]
    let received = db
        .range_by(&Field::Id, &Value::Int(3)..=&Value::Int(7))
        .unwrap();
    let received_ids: Vec<i64> = received.iter().map(|inst| inst.id).collect();

    assert_eq!(received_ids, vec![3, 4, 5, 6, 7]);

    // Test range (-inf, 7]
    let received = db.range_by(&Field::Id, ..=&Value::Int(7)).unwrap();
    let received_ids: Vec<i64> = received.iter().map(|inst| inst.id).collect();

    assert_eq!(received_ids, vec![0, 1, 2, 3, 4, 5, 6, 7]);

    // Test range (3, inf)
    let received = db.range_by(&Field::Id, &Value::Int(3)..).unwrap();
    let received_ids: Vec<i64> = received.iter().map(|inst| inst.id).collect();

    assert_eq!(received_ids, vec![3, 4, 5, 6, 7, 8, 9]);
}

#[test]
fn test_batch_find_by() {
    let data_dir = tmp_dir();
    let mut db = DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    let inserted_ids = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9];

    for id in inserted_ids.iter() {
        db.upsert(Inst {
            id: *id,
            name: Some("Foobar".to_string()),
            data: vec![],
        })
        .unwrap();
    }

    let batch: Vec<Value> = (2..5).map(Value::Int).collect();
    let result = db.batch_find_by(&Field::Id, &batch).unwrap();

    assert_eq!(result.len(), batch.len());
    assert_eq!(
        result.iter().map(|(tag, _)| *tag).collect::<Vec<usize>>(),
        vec![0, 1, 2]
    );
    assert_eq!(
        result.iter().map(|(_, inst)| inst.id).collect::<Vec<i64>>(),
        vec![2, 3, 4]
    );
}

#[test]
fn test_commit_transaction() {
    let data_dir = tmp_dir();
    let mut db = DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    db.tx_begin().expect("Failed to begin transaction");

    db.upsert(Inst {
        id: 0,
        name: Some("John".to_string()),
        data: vec![3, 4, 5],
    })
    .unwrap();

    db.upsert(Inst {
        id: 1,
        name: Some("John".to_string()),
        data: vec![1, 2, 3],
    })
    .unwrap();

    db.tx_commit().expect("Failed to commit transaction");

    let johns = db
        .find_by(&Field::Name, &Value::String("John".to_string()))
        .unwrap();

    assert_eq!(johns.len(), 2);
}

#[test]
fn test_rollback_transaction() {
    let data_dir = tmp_dir();
    let mut db = DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    db.tx_begin().expect("Failed to begin transaction");

    db.upsert(Inst {
        id: 0,
        name: Some("John".to_string()),
        data: vec![3, 4, 5],
    })
    .unwrap();

    db.upsert(Inst {
        id: 1,
        name: Some("John".to_string()),
        data: vec![1, 2, 3],
    })
    .unwrap();

    db.tx_rollback().expect("Failed to roll back transaction");

    let johns = db
        .find_by(&Field::Name, &Value::String("John".to_string()))
        .unwrap();

    assert_eq!(johns.len(), 0);
}

#[derive(Eq, PartialEq, Clone, Debug)]
enum FieldWithNewNullableField {
    Id,
    Name,
    Data,
    MaybeStr,
}

struct InstWithNewNullableField {
    pub id: i64,
    pub name: Option<String>,
    pub data: Vec<u8>,
    pub maybe_str: Option<String>,
}

impl InstWithNewNullableField {
    fn schema() -> Vec<(FieldWithNewNullableField, Type)> {
        vec![
            (FieldWithNewNullableField::Id, Type::int()),
            (FieldWithNewNullableField::Name, Type::string().nullable()),
            (FieldWithNewNullableField::Data, Type::bytes()),
            (
                FieldWithNewNullableField::MaybeStr,
                Type::string().nullable(),
            ),
        ]
    }

    fn primary_key() -> FieldWithNewNullableField {
        FieldWithNewNullableField::Id
    }

    fn secondary_keys() -> Vec<FieldWithNewNullableField> {
        vec![FieldWithNewNullableField::Name]
    }

    fn into_record(self) -> Vec<Value> {
        vec![
            Value::Int(self.id),
            match self.name {
                Some(name) => Value::String(name),
                None => Value::Null,
            },
            Value::Bytes(self.data),
            match self.maybe_str {
                Some(maybe_str) => Value::String(maybe_str),
                None => Value::Null,
            },
        ]
    }

    fn from_record(record: Vec<Value>) -> Self {
        let mut it = record.into_iter();

        InstWithNewNullableField {
            id: match it.next().unwrap() {
                Value::Int(id) => id,
                other => panic!("Invalid value type: {:?}", other),
            },
            name: match it.next().unwrap() {
                Value::String(name) => Some(name),
                Value::Null => None,
                other => panic!("Invalid value type: {:?}", other),
            },
            data: match it.next().unwrap() {
                Value::Bytes(data) => data,
                other => panic!("Invalid value type: {:?}", other),
            },
            maybe_str: match it.next() {
                Some(Value::String(maybe_str)) => Some(maybe_str),
                Some(Value::Null) => None,
                None => None,
                other => panic!("Invalid value type: {:?}", other),
            },
        }
    }
}

#[test]
fn test_add_nullable_field() {
    let data_dir = tmp_dir();

    // Insert a record with 3 fields
    {
        let mut db = DB::configure()
            .schema(Inst::schema())
            .primary_key(Inst::primary_key())
            .secondary_keys(Inst::secondary_keys())
            .from_record(Inst::from_record)
            .into_record(Inst::into_record)
            .data_dir(&data_dir)
            .initialize()
            .expect("Failed to initialize DB instance");

        db.upsert(Inst {
            id: 0,
            name: Some("John".to_string()),
            data: vec![3, 4, 5],
        })
        .unwrap();
    }

    // Insert a record with 4 fields (last is nullable)
    let mut db = DB::configure()
        .schema(InstWithNewNullableField::schema())
        .primary_key(InstWithNewNullableField::primary_key())
        .secondary_keys(InstWithNewNullableField::secondary_keys())
        .from_record(InstWithNewNullableField::from_record)
        .into_record(InstWithNewNullableField::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    db.upsert(InstWithNewNullableField {
        id: 1,
        name: Some("John".to_string()),
        data: vec![3, 4, 5],
        maybe_str: None,
    })
    .unwrap();

    let johns = db
        .find_by(
            &FieldWithNewNullableField::Name,
            &Value::String("John".to_string()),
        )
        .unwrap();

    assert_eq!(johns.len(), 2);
}

#[test]
fn test_add_non_nullable_field() {
    let data_dir = tmp_dir();

    // Insert a record with just one field
    {
        let mut db = DB::configure()
            .schema(InstSingleId::schema())
            .primary_key(InstSingleId::primary_key())
            .secondary_keys(InstSingleId::secondary_keys())
            .from_record(InstSingleId::from_record)
            .into_record(InstSingleId::into_record)
            .data_dir(&data_dir)
            .initialize()
            .expect("Failed to initialize DB instance");

        db.upsert(InstSingleId { id: 0 }).unwrap();
    }

    // Configure the DB with three fields, one of which is non-nullable
    // This should fail
    match DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .initialize()
    {
        Ok(_) => panic!("Expected initialization to fail"),
        Err(DBError::ValidationError(e)) => {
            // Expected
            error!("Expected: {:?}", e);
        }
        Err(e) => panic!("Unexpected error: {:?}", e),
    }
}

#[test]
fn test_delete_by_multiple_indexes() {
    let data_dir = tmp_dir();

    let mut db = DB::configure()
        .schema(Inst::schema())
        .primary_key(Inst::primary_key())
        .secondary_keys(Inst::secondary_keys())
        .from_record(Inst::from_record)
        .into_record(Inst::into_record)
        .data_dir(&data_dir)
        .initialize()
        .expect("Failed to initialize DB instance");

    // Insert some identical records
    for _ in 0..10 {
        db.upsert(Inst {
            id: 0,
            name: Some("foo".to_string()),
            data: vec![],
        })
        .unwrap();
    }

    // Delete by name
    db.delete_by(&Field::Name, &Value::String("foo".to_string()))
        .unwrap();

    // Check that the records are deleted by finding by name
    let result = db
        .find_by(&Field::Name, &Value::String("foo".to_string()))
        .unwrap();
    assert_eq!(result.len(), 0);

    // Double check with find by id
    let result = db.find_by(&Field::Id, &Value::Int(0)).unwrap();
    assert_eq!(result.len(), 0);
}
