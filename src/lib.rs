use pyo3::exceptions::{PyException, PyKeyError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyBytes;
use pyo3::IntoPyObjectExt;
use std::ops::Bound as B;
use std::path::PathBuf;
use std::sync::Mutex;

pyo3::create_exception!(pyfjall, Error, PyException, "Error raised by the fjall storage engine.");

fn err(e: fjall::Error) -> PyErr {
    Error::new_err(e.to_string())
}

fn bytes<'py>(py: Python<'py>, b: &[u8]) -> Bound<'py, PyBytes> {
    PyBytes::new(py, b)
}

fn persist_mode(mode: &str) -> PyResult<fjall::PersistMode> {
    use fjall::PersistMode::*;
    match mode {
        "buffer" => Ok(Buffer),
        "sync_data" => Ok(SyncData),
        "sync_all" => Ok(SyncAll),
        _ => Err(PyValueError::new_err("mode must be 'buffer', 'sync_data' or 'sync_all'")),
    }
}

/// A fjall database holding one or more keyspaces.
#[pyclass(frozen, module = "pyfjall")]
struct Database {
    db: fjall::Database,
}

#[pymethods]
impl Database {
    #[new]
    #[pyo3(signature = (path, *, cache_size=None, worker_threads=None, temporary=false))]
    fn new(
        py: Python<'_>,
        path: PathBuf,
        cache_size: Option<u64>,
        worker_threads: Option<usize>,
        temporary: bool,
    ) -> PyResult<Self> {
        let mut b = fjall::Database::builder(path).temporary(temporary);
        if let Some(n) = cache_size {
            b = b.cache_size(n);
        }
        if let Some(n) = worker_threads {
            b = b.worker_threads(n);
        }
        Ok(Self { db: py.detach(|| b.open()).map_err(err)? })
    }

    #[pyo3(signature = (name, *, max_memtable_size=None, kv_separation=false))]
    fn keyspace(&self, name: &str, max_memtable_size: Option<u64>, kv_separation: bool) -> PyResult<Keyspace> {
        let ks = self
            .db
            .keyspace(name, || {
                let mut o = fjall::KeyspaceCreateOptions::default();
                if let Some(n) = max_memtable_size {
                    o = o.max_memtable_size(n);
                }
                if kv_separation {
                    o = o.with_kv_separation(Some(fjall::KvSeparationOptions::default()));
                }
                o
            })
            .map_err(err)?;
        Ok(Keyspace { ks })
    }

    fn keyspace_exists(&self, name: &str) -> bool {
        self.db.keyspace_exists(name)
    }

    fn keyspace_count(&self) -> usize {
        self.db.keyspace_count()
    }

    fn list_keyspace_names(&self) -> Vec<String> {
        self.db.list_keyspace_names().iter().map(|s| s.to_string()).collect()
    }

    fn delete_keyspace(&self, py: Python<'_>, keyspace: &Keyspace) -> PyResult<()> {
        let ks = keyspace.ks.clone();
        py.detach(|| self.db.delete_keyspace(ks)).map_err(err)
    }

    fn batch(&self) -> Batch {
        Batch { b: Some(self.db.batch()) }
    }

    #[pyo3(signature = (mode="sync_all"))]
    fn persist(&self, py: Python<'_>, mode: &str) -> PyResult<()> {
        let m = persist_mode(mode)?;
        py.detach(|| self.db.persist(m)).map_err(err)
    }

    fn disk_space(&self) -> PyResult<u64> {
        self.db.disk_space().map_err(err)
    }

    fn __enter__<'py>(slf: Bound<'py, Self>) -> Bound<'py, Self> {
        slf
    }

    fn __exit__(&self, py: Python<'_>, _t: Bound<'_, PyAny>, _v: Bound<'_, PyAny>, _tb: Bound<'_, PyAny>) -> PyResult<()> {
        self.persist(py, "sync_all")
    }
}

/// Handle to a keyspace (its own LSM-tree). Keys and values are `bytes`.
#[pyclass(frozen, module = "pyfjall")]
struct Keyspace {
    ks: fjall::Keyspace,
}

#[pymethods]
impl Keyspace {
    #[getter]
    fn name(&self) -> String {
        self.ks.name().to_string()
    }

    fn insert(&self, py: Python<'_>, key: &[u8], value: &[u8]) -> PyResult<()> {
        py.detach(|| self.ks.insert(key, value)).map_err(err)
    }

    fn get<'py>(&self, py: Python<'py>, key: &[u8]) -> PyResult<Option<Bound<'py, PyBytes>>> {
        Ok(py.detach(|| self.ks.get(key)).map_err(err)?.map(|v| bytes(py, &v)))
    }

    fn remove(&self, py: Python<'_>, key: &[u8]) -> PyResult<()> {
        py.detach(|| self.ks.remove(key)).map_err(err)
    }

    fn contains_key(&self, py: Python<'_>, key: &[u8]) -> PyResult<bool> {
        py.detach(|| self.ks.contains_key(key)).map_err(err)
    }

    fn size_of(&self, py: Python<'_>, key: &[u8]) -> PyResult<Option<u32>> {
        py.detach(|| self.ks.size_of(key)).map_err(err)
    }

    /// O(n): scans the whole keyspace. Prefer `approximate_len()` or `is_empty()`.
    fn len(&self, py: Python<'_>) -> PyResult<usize> {
        py.detach(|| self.ks.len()).map_err(err)
    }

    fn is_empty(&self, py: Python<'_>) -> PyResult<bool> {
        py.detach(|| self.ks.is_empty()).map_err(err)
    }

    fn approximate_len(&self) -> usize {
        self.ks.approximate_len()
    }

    fn disk_space(&self) -> u64 {
        self.ks.disk_space()
    }

    fn clear(&self) -> PyResult<()> {
        self.ks.clear().map_err(err)
    }

    fn first_key_value<'py>(&self, py: Python<'py>) -> PyResult<Option<(Bound<'py, PyBytes>, Bound<'py, PyBytes>)>> {
        kv(py, self.ks.first_key_value())
    }

    fn last_key_value<'py>(&self, py: Python<'py>) -> PyResult<Option<(Bound<'py, PyBytes>, Bound<'py, PyBytes>)>> {
        kv(py, self.ks.last_key_value())
    }

    #[pyo3(signature = (*, reverse=false, keys_only=false))]
    fn iter(&self, reverse: bool, keys_only: bool) -> Iter {
        Iter::new(self.ks.iter(), reverse, keys_only)
    }

    #[pyo3(signature = (prefix, *, reverse=false, keys_only=false))]
    fn prefix(&self, prefix: &[u8], reverse: bool, keys_only: bool) -> Iter {
        Iter::new(self.ks.prefix(prefix), reverse, keys_only)
    }

    /// `start` is inclusive, `end` is exclusive (Python slice semantics); `None` = unbounded.
    #[pyo3(signature = (start=None, end=None, *, reverse=false, keys_only=false))]
    fn range(&self, start: Option<&[u8]>, end: Option<&[u8]>, reverse: bool, keys_only: bool) -> Iter {
        let r = (start.map_or(B::Unbounded, B::Included), end.map_or(B::Unbounded, B::Excluded));
        Iter::new(self.ks.range::<&[u8], _>(r), reverse, keys_only)
    }

    fn __getitem__<'py>(&self, py: Python<'py>, key: &[u8]) -> PyResult<Bound<'py, PyBytes>> {
        self.get(py, key)?.ok_or_else(|| PyKeyError::new_err(key.to_vec()))
    }

    fn __setitem__(&self, py: Python<'_>, key: &[u8], value: &[u8]) -> PyResult<()> {
        self.insert(py, key, value)
    }

    fn __delitem__(&self, py: Python<'_>, key: &[u8]) -> PyResult<()> {
        self.remove(py, key)
    }

    fn __contains__(&self, py: Python<'_>, key: &[u8]) -> PyResult<bool> {
        self.contains_key(py, key)
    }
}

fn kv<'py>(py: Python<'py>, g: Option<fjall::Guard>) -> PyResult<Option<(Bound<'py, PyBytes>, Bound<'py, PyBytes>)>> {
    match g {
        None => Ok(None),
        Some(g) => {
            let (k, v) = g.into_inner().map_err(err)?;
            Ok(Some((bytes(py, &k), bytes(py, &v))))
        }
    }
}

/// Iterator yielding `(key, value)` tuples, or just keys when created with `keys_only=True`.
#[pyclass(module = "pyfjall")]
struct Iter {
    it: Mutex<fjall::Iter>,
    reverse: bool,
    keys_only: bool,
}

impl Iter {
    fn new(it: fjall::Iter, reverse: bool, keys_only: bool) -> Self {
        Self { it: Mutex::new(it), reverse, keys_only }
    }
}

#[pymethods]
impl Iter {
    fn __iter__<'py>(slf: Bound<'py, Self>) -> Bound<'py, Self> {
        slf
    }

    fn __next__(&mut self, py: Python<'_>) -> PyResult<Option<Py<PyAny>>> {
        let (reverse, keys_only) = (self.reverse, self.keys_only);
        let it = self.it.get_mut().expect("iterator mutex poisoned");
        let item = py.detach(|| {
            let g = if reverse { it.next_back() } else { it.next() }?;
            Some(if keys_only {
                g.key().map(|k| (k, None))
            } else {
                g.into_inner().map(|(k, v)| (k, Some(v)))
            })
        });
        match item {
            None => Ok(None),
            Some(Err(e)) => Err(err(e)),
            Some(Ok((k, None))) => Ok(Some(bytes(py, &k).into_py_any(py)?)),
            Some(Ok((k, Some(v)))) => Ok(Some((bytes(py, &k), bytes(py, &v)).into_py_any(py)?)),
        }
    }
}

/// Atomic write batch across keyspaces. Usable as a context manager (commits on clean exit).
#[pyclass(module = "pyfjall")]
struct Batch {
    b: Option<fjall::OwnedWriteBatch>,
}

impl Batch {
    fn inner(&mut self) -> PyResult<&mut fjall::OwnedWriteBatch> {
        self.b.as_mut().ok_or_else(|| Error::new_err("batch already committed"))
    }
}

#[pymethods]
impl Batch {
    fn insert(&mut self, keyspace: &Keyspace, key: &[u8], value: &[u8]) -> PyResult<()> {
        self.inner()?.insert(&keyspace.ks, key, value);
        Ok(())
    }

    fn remove(&mut self, keyspace: &Keyspace, key: &[u8]) -> PyResult<()> {
        self.inner()?.remove(&keyspace.ks, key);
        Ok(())
    }

    fn commit(&mut self, py: Python<'_>) -> PyResult<()> {
        let b = self.b.take().ok_or_else(|| Error::new_err("batch already committed"))?;
        py.detach(|| b.commit()).map_err(err)
    }

    fn __len__(&self) -> usize {
        self.b.as_ref().map_or(0, |b| b.len())
    }

    fn __enter__<'py>(slf: Bound<'py, Self>) -> Bound<'py, Self> {
        slf
    }

    fn __exit__(&mut self, py: Python<'_>, exc_type: Bound<'_, PyAny>, _v: Bound<'_, PyAny>, _tb: Bound<'_, PyAny>) -> PyResult<()> {
        if exc_type.is_none() {
            self.commit(py)
        } else {
            Ok(())
        }
    }
}

#[pymodule]
fn _pyfjall(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Database>()?;
    m.add_class::<Keyspace>()?;
    m.add_class::<Iter>()?;
    m.add_class::<Batch>()?;
    m.add("Error", m.py().get_type::<Error>())
}
