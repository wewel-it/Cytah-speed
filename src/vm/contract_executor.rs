use crate::core::transaction::Transaction;
use crate::state::state_manager::StateManager;
use crate::vm::wasm_runtime::{WasmRuntime, RuntimeState};
use crate::vm::gas_meter::GasMeter;
use crate::contracts::contract_registry::ContractRegistry;
use crate::contracts::contract_storage::ContractStorage;
use anyhow::Result;

/// Executor responsible for handling contract transactions and integrating
/// with the existing state manager.
#[derive(Debug)]
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
    /// Execute a transaction and return the amount of gas actually used.  The
    /// caller (e.g. `TransactionExecutor`) can then calculate the fee and
    /// credit it appropriately.
    pub fn execute_transaction(&mut self, tx: &Transaction) -> Result<u64, String> {
        match &tx.payload {
            crate::core::transaction::TxPayload::Transfer { .. } => {
                // For simple transfers we assume the sender uses the entire gas
                // limit.  The fee deduction is handled inside `apply_transaction`.
                self.state_manager.apply_transaction(tx)?;
                Ok(tx.gas_limit)
            }
            crate::core::transaction::TxPayload::ContractDeploy { wasm_code, init_args } => {
                self.deploy_contract(tx, wasm_code.clone(), init_args.clone())
            }
            crate::core::transaction::TxPayload::ContractCall { contract_address, method, args } => {
                self.call_contract(tx, *contract_address, method.clone(), args.clone())
            }
        }
    }

    fn deploy_contract(&mut self, tx: &Transaction, wasm_code: Vec<u8>, _init_args: Vec<u8>) -> Result<u64, String> {
        // derive deterministic address
        use sha2::{Digest, Sha256};
        let hash = Sha256::digest(&wasm_code);
        let mut address = [0u8; 20];
        address.copy_from_slice(&hash[0..20]);

        let gas_limit = tx.gas_limit;
        let state = RuntimeState {
            contract_address: address,
            caller: tx.from,
            block_height: 0,
            timestamp: 0,
            storage: Box::new(self.storage.clone()),
            gas_meter: GasMeter::new(gas_limit),
            memory_limiter: None,
        };
        let (mut store, instance) = self.runtime.instantiate_contract(&wasm_code, state, gas_limit)
            .map_err(|e| format!("WASM instantiate error: {}", e))?;

        let mut success = true;
        if let Some(init_func) = instance.get_func(&mut store, "init") {
            if let Err(_e) = init_func.call(&mut store, &[], &mut []) {
                success = false;
            }
        }

        let used = store.data().gas_meter.used;
        // charge fee
        let fee = used.saturating_mul(tx.gas_price);
        self.state_manager.deduct_fee(tx.from, fee)?;

        if success {
            // commit registry and storage updates
            self.registry.register_contract(address, wasm_code.clone())?;
            self.storage.initialize_contract(address)?;
            Ok(used)
        } else {
            Err("contract init failed".to_string())
        }
    }

    fn call_contract(&mut self, tx: &Transaction, contract_address: [u8; 20], method: String, _args: Vec<u8>) -> Result<u64, String> {
        let contract = self.registry.get_contract(&contract_address)
            .ok_or_else(|| "Contract not found".to_string())?;
        let bytecode = &contract.bytecode;

        let gas_limit = tx.gas_limit;
        let state = RuntimeState {
            contract_address,
            caller: tx.from,
            block_height: 0,
            timestamp: 0,
            storage: Box::new(self.storage.clone()),
            gas_meter: GasMeter::new(gas_limit),
            memory_limiter: None,
        };
        let (mut store, instance) = self.runtime.instantiate_contract(bytecode, state, gas_limit)
            .map_err(|e| format!("WASM instantiate error: {}", e))?;

        let result = self.runtime.call_function(&mut store, &instance, &method, &[]);
        let used = store.data().gas_meter.used;
        let fee = used.saturating_mul(tx.gas_price);
        self.state_manager.deduct_fee(tx.from, fee)?;

        result.map(|_| used).map_err(|e| format!("Contract call failed: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transaction::{Transaction, Address};
    use secp256k1::{Secp256k1, SecretKey};
    use rand::{Rng, thread_rng};

    fn create_signed_tx(from: Address, to: Address, amount: u64, nonce: u64, gas_limit: u64, gas_price: u64, privk: &SecretKey) -> Transaction {
        let mut tx = Transaction::new_transfer(from, to, amount, nonce, gas_limit, gas_price);
        tx.sign(privk).unwrap();
        tx
    }

    #[test]
    fn test_transfer_charges_full_gas() {
        let mut rng = thread_rng();
        let secp = Secp256k1::new();
        let mut priv_bytes = [0u8; 32];
        rng.fill(&mut priv_bytes);
        let privk = SecretKey::from_slice(&priv_bytes).unwrap();
        let pubk = privk.public_key(&secp);
        let pubhash = sha2::Sha256::digest(&pubk.serialize()[1..]);
        let from: Address = pubhash[12..32].try_into().unwrap();
        let to: Address = [2;20];

        let mut executor = ContractExecutor::new(StateManager::new());
        executor.state_manager.state_tree.update_account(from, crate::state::state_tree::Account::new(10000,0));

        let tx = create_signed_tx(from, to, 100, 0, 21000, 1, &privk);
        let used = executor.execute_transaction(&tx).expect("tx should succeed");
        assert_eq!(used, 21000);
        // fee = 21000*1
        let sender_acc = executor.state_manager.get_account(&from).unwrap();
        assert_eq!(sender_acc.balance, 10000 - 100 - 21000);
        let recv_acc = executor.state_manager.get_account(&to).unwrap();
        assert_eq!(recv_acc.balance, 100);
    }
}
