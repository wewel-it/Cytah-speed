use crate::core::transaction::Transaction;
use crate::state::state_manager::StateManager;
use crate::vm::wasm_runtime::{WasmRuntime, RuntimeState};
use crate::vm::gas_meter::GasMeter;
use crate::contracts::contract_registry::ContractRegistry;
use crate::contracts::contract_storage::ContractStorage;
use anyhow::Result;

/// Executor responsible for handling contract transactions and integrating
/// with the existing state manager.
pub struct ContractExecutor {
    pub state_manager: StateManager,
    pub registry: ContractRegistry,
    pub storage: ContractStorage,
    pub runtime: WasmRuntime,
}

impl ContractExecutor {
    pub fn new(state_manager: StateManager) -> Self {
        Self {
            registry: ContractRegistry::new(),
            storage: ContractStorage::new("contract_storage"),
            runtime: WasmRuntime::new(),
            state_manager,
        }
    }

    /// Handle a transaction of any supported type.
    pub fn execute_transaction(&mut self, tx: &Transaction) -> Result<(), String> {
        match &tx.payload {
            crate::core::transaction::TxPayload::Transfer { .. } => {
                self.state_manager.apply_transaction(tx)
            }
            crate::core::transaction::TxPayload::ContractDeploy { wasm_code, init_args } => {
                self.deploy_contract(tx, wasm_code.clone(), init_args.clone())
            }
            crate::core::transaction::TxPayload::ContractCall { contract_address, method, args } => {
                self.call_contract(tx, *contract_address, method.clone(), args.clone())
            }
        }
    }

    fn deploy_contract(&mut self, tx: &Transaction, wasm_code: Vec<u8>, _init_args: Vec<u8>) -> Result<(), String> {
        // derive deterministic address
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(&wasm_code);
        let mut address = [0u8; 20];
        address.copy_from_slice(&hash[0..20]);

        // store in registry
        self.registry.register_contract(address, wasm_code.clone())?;

        // initialize contract storage
        self.storage.initialize_contract(address)?;

        // Optionally call an `init` function if present (not implemented args parsing yet)
        let state = RuntimeState {
            contract_address: address,
            caller: tx.from,
            block_height: 0,
            timestamp: 0,
            storage: self.storage.clone(),
        };
        let (mut store, instance) = self.runtime.instantiate_contract(&wasm_code, state)
            .map_err(|e| format!("WASM instantiate error: {}", e))?;

        if let Some(init_func) = instance.get_func(&mut store, "init") {
            init_func.call(&mut store, &[], &mut [])
                .map_err(|e| format!("init call failed: {}", e))?;
        }

        Ok(())
    }

    fn call_contract(&mut self, tx: &Transaction, contract_address: [u8; 20], method: String, _args: Vec<u8>) -> Result<(), String> {
        let contract = self.registry.get_contract(&contract_address)
            .ok_or_else(|| "Contract not found".to_string())?;
        let bytecode = &contract.bytecode;

        let state = RuntimeState {
            contract_address,
            caller: tx.from,
            block_height: 0,
            timestamp: 0,
            storage: self.storage.clone(),
        };
        let (mut store, instance) = self.runtime.instantiate_contract(bytecode, state)
            .map_err(|e| format!("WASM instantiate error: {}", e))?;

        let _ = self.runtime.call_function(&mut store, &instance, &method, &[])
            .map_err(|e| format!("Contract call failed: {}", e))?;

        Ok(())
    }
}

