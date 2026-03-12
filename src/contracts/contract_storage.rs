use rocksdb::{DB, Options};

// some helper utilities are currently unused but may be handy later
#[allow(dead_code)]
use std::sync::Arc;
use crate::vm::wasm_runtime::Storage;

/// Simple wrapper around RocksDB for contract storage
#[derive(Clone, Debug)]
pub struct ContractStorage {
    db: Arc<DB>,
}

impl Storage for ContractStorage {
    fn read(&self, key: &[u8]) -> Option<Vec<u8>> {
        match self.db.get(key) {
            Ok(Some(v)) => Some(v.to_vec()),
            _ => None,
        }
    }
    fn write(&self, key: &[u8], value: &[u8]) {
        self.db.put(key, value).unwrap();
    }
    fn box_clone(&self) -> Box<dyn Storage> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ContractStorage {
    pub fn new(path: &str) -> Self {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        let db = DB::open(&opts, path).expect("failed to open contract storage");
        ContractStorage { db: Arc::new(db) }
    }

    /// Combine contract address and key into a single DB key
    #[allow(dead_code)]
    fn make_key(contract: &[u8;20], key: &[u8]) -> Vec<u8> {
        let mut k = Vec::with_capacity(20 + key.len());
        k.extend_from_slice(contract);
        k.extend_from_slice(key);
        k
    }

    pub fn write(&self, key: &[u8], value: &[u8]) {
        // key is already combined by caller or we combine with contract address earlier
        self.db.put(key, value).unwrap();
    }

    pub fn read(&self, key: &[u8]) -> Option<Vec<u8>> {
        match self.db.get(key) {
            Ok(Some(v)) => Some(v.to_vec()),
            _ => None,
        }
    }

    pub fn initialize_contract(&self, _address: [u8;20]) -> Result<(), String> {
        // nothing special yet, but we could set up namespace or default values
        Ok(())
    }
}
