use log_db;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;
use pyo3::types::PyTuple;

type Field = String;

#[pyclass]
#[derive(Clone)]
struct RecordField {
    record_field: log_db::RecordField,
}

#[pymethods]
impl RecordField {
    #[staticmethod]
    fn int() -> Self {
        RecordField {
            record_field: log_db::RecordField::int(),
        }
    }

    #[staticmethod]
    fn float() -> Self {
        RecordField {
            record_field: log_db::RecordField::float(),
        }
    }

    #[staticmethod]
    fn string() -> Self {
        RecordField {
            record_field: log_db::RecordField::string(),
        }
    }

    #[staticmethod]
    fn bytes() -> Self {
        RecordField {
            record_field: log_db::RecordField::bytes(),
        }
    }

    fn nullable(&self) -> Self {
        RecordField {
            record_field: self.record_field.clone().nullable(),
        }
    }
}

#[pyclass]
#[derive(Clone)]
struct MemtableEvictPolicy {
    memtable_evict_policy: log_db::MemtableEvictPolicy,
}

#[pyclass]
#[derive(Clone)]
struct WriteDurability {
    write_durability: log_db::WriteDurability,
}

#[pyclass]
struct Config {
    #[pyo3(get, set)]
    data_dir: Option<String>,
    #[pyo3(get, set)]
    segment_size: Option<usize>,
    #[pyo3(get, set)]
    memtable_capacity: Option<usize>,
    #[pyo3(get, set)]
    fields: Option<Vec<(Field, RecordField)>>,
    #[pyo3(get, set)]
    primary_key: Option<Field>,
    #[pyo3(get, set)]
    secondary_keys: Option<Vec<Field>>,
    #[pyo3(get, set)]
    memtable_evict_policy: Option<MemtableEvictPolicy>,
    #[pyo3(get, set)]
    write_durability: Option<WriteDurability>,
}

#[pymethods]
impl Config {
    pub fn initialize(&self) -> PyResult<DB> {
        let mut config = log_db::DB::configure();
        if self.data_dir.is_some() {
            config.data_dir(&self.data_dir.as_ref().unwrap().to_string());
        }
        if self.segment_size.is_some() {
            config.segment_size(self.segment_size.unwrap());
        }
        if self.memtable_capacity.is_some() {
            config.memtable_capacity(self.memtable_capacity.unwrap());
        }
        if self.fields.is_some() {
            let mut fields = Vec::new();
            for (field, record_field) in self.fields.as_ref().unwrap() {
                fields.push((field.to_string(), record_field.record_field.clone()));
            }
            config.fields(fields);
        }
        if self.primary_key.is_some() {
            config.primary_key(self.primary_key.as_ref().unwrap().to_string());
        }
        if self.secondary_keys.is_some() {
            let tmp = self.secondary_keys.as_ref().unwrap();
            config.secondary_keys(tmp.clone());
        }
        if self.memtable_evict_policy.is_some() {
            let tmp = self.memtable_evict_policy.as_ref().unwrap();
            config.memtable_evict_policy(tmp.memtable_evict_policy.clone());
        }
        if self.write_durability.is_some() {
            let tmp = self.write_durability.as_ref().unwrap();
            config.write_durability(tmp.write_durability.clone());
        }

        let db = config.initialize().map_err(|e| PyException::new_err(e))?;
        Ok(DB { db })
    }
}

#[pyclass]
#[derive(Clone)]
struct RecordValue {
    record_value: log_db::RecordValue,
}

#[pymethods]
impl RecordValue {
    #[staticmethod]
    fn int(value: i64) -> Self {
        RecordValue {
            record_value: log_db::RecordValue::Int(value),
        }
    }

    #[staticmethod]
    fn float(value: f64) -> Self {
        RecordValue {
            record_value: log_db::RecordValue::Float(value),
        }
    }

    #[staticmethod]
    fn string(value: &str) -> Self {
        RecordValue {
            record_value: log_db::RecordValue::String(value.to_string()),
        }
    }

    #[staticmethod]
    fn bytes(value: &[u8]) -> Self {
        RecordValue {
            record_value: log_db::RecordValue::Bytes(value.to_vec()),
        }
    }

    #[staticmethod]
    fn null() -> Self {
        RecordValue {
            record_value: log_db::RecordValue::Null,
        }
    }
}

#[pyclass]
struct Record {
    values: Vec<RecordValue>,
}

#[pymethods]
impl Record {
    #[new]
    #[pyo3(signature = (*py_args))]
    fn new(py_args: Vec<RecordValue>) -> Self {
        Record { values: py_args }
    }
}

#[pyclass]
struct DB {
    db: log_db::DB<Field>,
}

#[pymethods]
impl DB {
    fn upsert(&mut self, record: &Record) -> PyResult<()> {
        let values = record
            .values
            .iter()
            .map(|v| v.record_value.clone())
            .collect();

        self.db
            .upsert(&log_db::Record { values })
            .map_err(|e| PyException::new_err(e))?;
        Ok(())
    }

    #[staticmethod]
    pub fn configure() -> Config {
        Config {
            data_dir: None,
            segment_size: None,
            memtable_capacity: None,
            fields: None,
            primary_key: None,
            secondary_keys: None,
            memtable_evict_policy: None,
            write_durability: None,
        }
    }
}

// #[pyfunction]
// fn sum_as_string(a: usize, b: usize) -> PyResult<String> {
//     Ok((a + b).to_string())
// }

#[pymodule]
fn log_db_py(m: &Bound<'_, PyModule>) -> PyResult<()> {
    //m.add_function(wrap_pyfunction!(sum_as_string, m)?)?;
    m.add_class::<DB>()?;
    m.add_class::<RecordField>()?;
    m.add_class::<RecordValue>()?;
    m.add_class::<Record>()?;
    Ok(())
}
