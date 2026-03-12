use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct ContractInfo {
    pub address: [u8;20],
    pub bytecode: Vec<u8>,
    pub metadata: Vec<u8>,
}

#[derive(Default, Debug)]
pub struct ContractRegistry {
    contracts: HashMap<[u8;20], ContractInfo>,
}

impl ContractRegistry {
    pub fn new() -> Self {
        ContractRegistry { contracts: HashMap::new() }
    }

    pub fn register_contract(&mut self, address: [u8;20], bytecode: Vec<u8>) -> Result<(), String> {
        if self.contracts.contains_key(&address) {
            return Err("contract already exists".to_string());
        }
        let info = ContractInfo {
            address,
            bytecode,
            metadata: Vec::new(),
        };
        self.contracts.insert(address, info);
        Ok(())
    }

    pub fn add_metadata(&mut self, address: [u8;20], metadata: Vec<u8>) {
        if let Some(info) = self.contracts.get_mut(&address) {
            info.metadata = metadata;
        }
    }

    pub fn get_contract(&self, address: &[u8;20]) -> Option<&ContractInfo> {
        self.contracts.get(address)
    }
}
