use serde::{Deserialize, Serialize};
use std::sync::Arc;
use rocksdb::{DB, Options};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ContractInfo {
    pub address: [u8;20],
    pub bytecode: Vec<u8>,
    pub metadata: Vec<u8>,
}

/// ContractRegistry dengan persistent storage di RocksDB
/// Setiap kontrak yang terdaftar disimpan secara permanen di disk
#[derive(Debug, Clone)]
pub struct ContractRegistry {
    /// RocksDB instance untuk persistent storage
    db: Arc<DB>,
}

impl ContractRegistry {
    /// Buat registry baru dengan backend RocksDB
    pub fn new(db_path: &str) -> Result<Self, String> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        
        let db = DB::open(&opts, db_path)
            .map_err(|e| format!("Failed to open contract registry DB: {}", e))?;
        
        Ok(ContractRegistry {
            db: Arc::new(db),
        })
    }

    /// Register kontrak baru dengan bytecode
    /// Kontrak disimpan secara permanen di RocksDB
    pub fn register_contract(&mut self, address: [u8;20], bytecode: Vec<u8>) -> Result<(), String> {
        // Check jika kontrak sudah terdaftar
        if self.get_contract(&address).is_some() {
            return Err("contract already exists".to_string());
        }

        let info = ContractInfo {
            address,
            bytecode,
            metadata: Vec::new(),
        };

        // Serialize dan simpan ke RocksDB
        let serialized = bincode::serialize(&info)
            .map_err(|e| format!("Failed to serialize contract info: {}", e))?;
        
        let key = Self::make_registry_key(&address);
        self.db.put(&key, &serialized)
            .map_err(|e| format!("Failed to write contract to DB: {}", e))?;

        tracing::debug!("Registered contract at {} in RocksDB", hex::encode(address));
        Ok(())
    }

    /// Tambahkan metadata ke kontrak yang sudah terdaftar
    pub fn add_metadata(&mut self, address: [u8;20], metadata: Vec<u8>) -> Result<(), String> {
        let key = Self::make_registry_key(&address);
        
        // Baca kontrak yang sudah ada
        let serialized = self.db.get(&key)
            .map_err(|e| format!("Failed to read from DB: {}", e))?
            .ok_or_else(|| "contract not found".to_string())?;
        
        let mut info: ContractInfo = bincode::deserialize(&serialized)
            .map_err(|e| format!("Failed to deserialize contract info: {}", e))?;

        // Update metadata
        info.metadata = metadata;

        // Simpan kembali
        let updated_serialized = bincode::serialize(&info)
            .map_err(|e| format!("Failed to serialize updated contract info: {}", e))?;
        
        self.db.put(&key, &updated_serialized)
            .map_err(|e| format!("Failed to update contract in DB: {}", e))?;

        tracing::debug!("Updated metadata for contract at {}", hex::encode(address));
        Ok(())
    }

    /// Ambil informasi kontrak dari registry
    /// Jika tidak ditemukan di memory, ambil dari RocksDB
    pub fn get_contract(&self, address: &[u8;20]) -> Option<ContractInfo> {
        let key = Self::make_registry_key(address);
        
        match self.db.get(&key) {
            Ok(Some(data)) => {
                match bincode::deserialize::<ContractInfo>(&data) {
                    Ok(info) => {
                        tracing::trace!("Retrieved contract from RocksDB: {}", hex::encode(address));
                        Some(info)
                    }
                    Err(e) => {
                        tracing::error!("Failed to deserialize contract info: {}", e);
                        None
                    }
                }
            }
            Ok(None) => None,
            Err(e) => {
                tracing::error!("Failed to read contract from DB: {}", e);
                None
            }
        }
    }

    /// Hapus kontrak dari registry
    pub fn delete_contract(&self, address: &[u8;20]) -> Result<(), String> {
        let key = Self::make_registry_key(address);
        self.db.delete(&key)
            .map_err(|e| format!("Failed to delete contract from DB: {}", e))?;
        
        tracing::debug!("Deleted contract at {} from RocksDB", hex::encode(address));
        Ok(())
    }

    /// Create a contract registry using the default path.
    ///
    /// This is a convenience wrapper for tests and simple usage.
    pub fn default() -> Self {
        ContractRegistry::new("./data/contracts.db")
            .expect("Failed to initialize default contract registry")
    }

    /// List semua kontrak terdaftar (iterate melalui DB)
    pub fn list_all_contracts(&self) -> Result<Vec<ContractInfo>, String> {
        let mut contracts = Vec::new();
        let iter = self.db.iterator(rocksdb::IteratorMode::From(&[b'c', b'o', b'n', b't'][..], rocksdb::Direction::Forward));

        for item in iter {
            let (key, value) = item.map_err(|e| format!("RocksDB iterator error: {}", e))?;
            // Filter keys yang merupakan kontrak registry entries
            if key.starts_with(b"contract:") {
                match bincode::deserialize::<ContractInfo>(&value) {
                    Ok(info) => contracts.push(info),
                    Err(e) => {
                        tracing::warn!("Failed to deserialize contract entry: {}", e);
                    }
                }
            }
        }

        Ok(contracts)
    }

    /// Helper function untuk membuat database key dari contract address
    fn make_registry_key(address: &[u8;20]) -> Vec<u8> {
        let mut key = Vec::with_capacity(25);
        key.extend_from_slice(b"contract:");
        key.extend_from_slice(address);
        key
    }

    /// Check jika kontrak sudah terdaftar
    pub fn contract_exists(&self, address: &[u8;20]) -> bool {
        let key = Self::make_registry_key(address);
        self.db.get(&key)
            .ok()
            .flatten()
            .is_some()
    }

    /// Dapatkan jumlah kontrak yang terdaftar
    pub fn contract_count(&self) -> Result<usize, String> {
        let contracts = self.list_all_contracts()?;
        Ok(contracts.len())
    }
}

impl Default for ContractRegistry {
    fn default() -> Self {
        ContractRegistry::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn create_test_registry(path: &str) -> ContractRegistry {
        // Clean up test DB if exists
        let _ = fs::remove_dir_all(path);
        ContractRegistry::new(path).expect("Should create registry")
    }

    #[test]
    fn test_register_and_retrieve_contract() {
        let registry = create_test_registry("./test_registry1.db");
        let mut reg = registry.clone();
        
        let address = [1u8; 20];
        let bytecode = vec![0x60, 0x01, 0x60, 0x02]; // Simple WASM stub
        
        reg.register_contract(address, bytecode.clone()).expect("Should register");
        
        let contract = registry.get_contract(&address);
        assert!(contract.is_some());
        assert_eq!(contract.unwrap().bytecode, bytecode);
        
        let _ = fs::remove_dir_all("./test_registry1.db");
    }

    #[test]
    fn test_persistence_after_restart() {
        let db_path = "./test_registry_restart.db";
        let address = [2u8; 20];
        let bytecode = vec![0x61, 0x01, 0x61, 0x02, 0x61, 0x03];
        let metadata = vec![0x00, 0x01, 0x02];
        
        // First session: register contract
        {
            let mut registry = create_test_registry(db_path);
            registry.register_contract(address, bytecode.clone())
                .expect("Should register contract");
            registry.add_metadata(address, metadata.clone())
                .expect("Should add metadata");
            
            // Verify in first session
            let contract = registry.get_contract(&address);
            assert!(contract.is_some());
            let info = contract.unwrap();
            assert_eq!(info.bytecode, bytecode);
            assert_eq!(info.metadata, metadata);
            
            // Drop registry to ensure DB is flushed
            drop(registry);
        }
        
        // Simulate restart: Create new registry instance pointing to same DB
        {
            let registry = ContractRegistry::new(db_path)
                .expect("Should open existing DB");
            
            // Verify data persisted
            let contract = registry.get_contract(&address);
            assert!(contract.is_some(), "Contract should persist after restart");
            
            let info = contract.unwrap();
            assert_eq!(info.bytecode, bytecode, "Bytecode should match after restart");
            assert_eq!(info.metadata, metadata, "Metadata should match after restart");
            assert_eq!(info.address, address, "Address should match after restart");
        }
        
        let _ = fs::remove_dir_all(db_path);
    }

    #[test]
    fn test_multiple_contracts_persistence() {
        let db_path = "./test_registry_multi.db";
        let _ = fs::remove_dir_all(db_path);
        
        // Register multiple contracts
        let contracts = vec![
            ([3u8; 20], vec![0xaa, 0xbb]),
            ([4u8; 20], vec![0xcc, 0xdd, 0xee]),
            ([5u8; 20], vec![0xff, 0x00]),
        ];
        
        {
            let mut registry = ContractRegistry::new(db_path).expect("Should create registry");
            
            for (addr, bytecode) in contracts.iter() {
                registry.register_contract(*addr, bytecode.clone())
                    .expect("Should register contract");
            }
            
            drop(registry);
        }
        
        // Verify after restart
        {
            let registry = ContractRegistry::new(db_path)
                .expect("Should open existing DB");
            
            let count = registry.contract_count()
                .expect("Should get count");
            assert_eq!(count, 3, "Should have 3 contracts after restart");
            
            for (addr, bytecode) in contracts.iter() {
                let contract = registry.get_contract(addr);
                assert!(contract.is_some(), "Contract should exist");
                assert_eq!(contract.unwrap().bytecode, *bytecode);
            }
        }
        
        let _ = fs::remove_dir_all(db_path);
    }

    #[test]
    fn test_contract_existence_check() {
        let registry = create_test_registry("./test_registry_exists.db");
        let mut reg = registry.clone();
        
        let address = [6u8; 20];
        assert!(!registry.contract_exists(&address), "Contract should not exist initially");
        
        reg.register_contract(address, vec![0x11, 0x22])
            .expect("Should register");
        
        assert!(registry.contract_exists(&address), "Contract should exist after register");
        
        let _ = fs::remove_dir_all("./test_registry_exists.db");
    }

    #[test]
    fn test_metadata_update_persistence() {
        let db_path = "./test_registry_metadata.db";
        let _ = fs::remove_dir_all(db_path);
        
        let address = [7u8; 20];
        let bytecode = vec![0x44, 0x55, 0x66];
        let metadata1 = vec![0xaa];
        let metadata2 = vec![0xbb, 0xcc];
        
        {
            let mut registry = ContractRegistry::new(db_path).expect("Should create registry");
            
            registry.register_contract(address, bytecode.clone())
                .expect("Should register");
            registry.add_metadata(address, metadata1.clone())
                .expect("Should add metadata");
            
            drop(registry);
        }
        
        // Verify first metadata
        {
            let registry = ContractRegistry::new(db_path)
                .expect("Should open existing DB");
            
            let contract = registry.get_contract(&address).unwrap();
            assert_eq!(contract.metadata, metadata1);
        }
        
        // Update metadata
        {
            let mut registry = ContractRegistry::new(db_path)
                .expect("Should open existing DB");
            
            registry.add_metadata(address, metadata2.clone())
                .expect("Should update metadata");
            
            drop(registry);
        }
        
        // Verify updated metadata persists
        {
            let registry = ContractRegistry::new(db_path)
                .expect("Should open existing DB");
            
            let contract = registry.get_contract(&address).unwrap();
            assert_eq!(contract.metadata, metadata2, "Updated metadata should persist");
        }
        
        let _ = fs::remove_dir_all(db_path);
    }

    #[test]
    fn test_duplicate_registration_rejection() {
        let registry = create_test_registry("./test_registry_dup.db");
        let mut reg = registry.clone();
        
        let address = [8u8; 20];
        let bytecode1 = vec![0x11];
        let bytecode2 = vec![0x22];
        
        reg.register_contract(address, bytecode1)
            .expect("First registration should succeed");
        
        let result = reg.register_contract(address, bytecode2);
        assert!(result.is_err(), "Duplicate registration should fail");
        assert_eq!(result.unwrap_err(), "contract already exists");
        
        let _ = fs::remove_dir_all("./test_registry_dup.db");
    }

    #[test]
    fn test_list_all_contracts() {
        let db_path = "./test_registry_list.db";
        let _ = fs::remove_dir_all(db_path);
        
        let contracts = vec![
            ([10u8; 20], vec![0x10]),
            ([11u8; 20], vec![0x11]),
            ([12u8; 20], vec![0x12]),
        ];
        
        {
            let mut registry = ContractRegistry::new(db_path).expect("Should create registry");
            
            for (addr, bytecode) in contracts.iter() {
                registry.register_contract(*addr, bytecode.clone())
                    .expect("Should register");
            }
            
            drop(registry);
        }
        
        {
            let registry = ContractRegistry::new(db_path)
                .expect("Should open existing DB");
            
            let listed = registry.list_all_contracts()
                .expect("Should list contracts");
            
            assert_eq!(listed.len(), 3, "Should list all 3 contracts");
            
            for (addr, bytecode) in contracts.iter() {
                let found = listed.iter()
                    .find(|c| c.address == *addr)
                    .expect("Contract should be in list");
                assert_eq!(&found.bytecode, bytecode);
            }
        }
        
        let _ = fs::remove_dir_all(db_path);
    }

    #[test]
    fn test_delete_contract() {
        let registry = create_test_registry("./test_registry_delete.db");
        let mut reg = registry.clone();
        
        let address = [13u8; 20];
        
        reg.register_contract(address, vec![0x33])
            .expect("Should register");
        
        assert!(registry.contract_exists(&address), "Should exist");
        
        registry.delete_contract(&address)
            .expect("Should delete");
        
        assert!(!registry.contract_exists(&address), "Should not exist after delete");
        
        let contract = registry.get_contract(&address);
        assert!(contract.is_none(), "Should return None after delete");
        
        let _ = fs::remove_dir_all("./test_registry_delete.db");
    }
}
