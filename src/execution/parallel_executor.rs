use crate::core::transaction::{Transaction, TxPayload, Address};
use crate::state::state_manager::StateManager;
use crate::state::state_tree::Account;
use crate::contracts::contract_storage::ContractStorage;
use crate::contracts::contract_registry::ContractRegistry;
use crate::vm::gas_meter::GasMeter;
use crate::vm::wasm_runtime::{WasmRuntime, RuntimeState, Storage};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// A transaction identifier - we use the hash of the transaction.
pub type TxId = [u8; 32];

/// A key in the global state that transactions may read or write.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum StateKey {
    Account(Address),                // whole account (balance, nonce)
    ContractStorage(Address, Vec<u8>), // contract address + storage key; empty Vec means wildcard
    ContractRegistry(Address),       // contract registry entry for a given address
}

/// The read/write sets for a single transaction.
#[derive(Debug, Clone)]
pub struct TxAccessSet {
    pub reads: HashSet<StateKey>,
    pub writes: HashSet<StateKey>,
}

/// Compute the access set for a transaction.  This is necessarily
/// conservative; contract calls assume they may touch the entire
/// contract account and storage.
pub fn analyze_access(tx: &Transaction) -> TxAccessSet {
    let mut reads = HashSet::new();
    let mut writes = HashSet::new();

    // sender is always read and written (nonce, balance)
    reads.insert(StateKey::Account(tx.from));
    writes.insert(StateKey::Account(tx.from));

    match &tx.payload {
        TxPayload::Transfer { to, .. } => {
            reads.insert(StateKey::Account(*to));
            writes.insert(StateKey::Account(*to));
        }
        TxPayload::ContractDeploy { wasm_code, .. } => {
            // address computed deterministically from code
            use sha2::{Digest, Sha256};
            let hash = Sha256::digest(&wasm_code);
            let mut address = [0u8; 20];
            address.copy_from_slice(&hash[0..20]);
            writes.insert(StateKey::Account(address));
            writes.insert(StateKey::ContractStorage(address, Vec::new()));
            writes.insert(StateKey::ContractRegistry(address));
        }
        TxPayload::ContractCall { contract_address, .. } => {
            reads.insert(StateKey::Account(*contract_address));
            writes.insert(StateKey::Account(*contract_address));
            // wildcard storage access for this contract
            reads.insert(StateKey::ContractStorage(*contract_address, Vec::new()));
            writes.insert(StateKey::ContractStorage(*contract_address, Vec::new()));
            // reading from contract registry (must happen after deploy)
            reads.insert(StateKey::ContractRegistry(*contract_address));
        }
    }

    TxAccessSet { reads, writes }
}

/// Do two access sets conflict?
pub fn transactions_conflict(a: &TxAccessSet, b: &TxAccessSet) -> bool {
    // write/write conflicts
    if !a.writes.is_disjoint(&b.writes) {
        return true;
    }
    // write/read conflicts
    if !a.writes.is_disjoint(&b.reads) {
        return true;
    }
    if !a.reads.is_disjoint(&b.writes) {
        return true;
    }
    false
}

/// Graph of conflicts keyed by transaction id.
#[derive(Debug, Clone)]
pub struct TxConflictGraph {
    pub edges: HashMap<TxId, HashSet<TxId>>,
}

/// Build a deterministic conflict graph for a list of transactions.
pub fn build_conflict_graph(txs: &[Transaction]) -> TxConflictGraph {
    let mut edges: HashMap<TxId, HashSet<TxId>> = HashMap::new();
    let access_sets: Vec<TxAccessSet> = txs.iter().map(analyze_access).collect();
    let ids: Vec<TxId> = txs.iter().map(|tx| tx.hash()).collect();

    for i in 0..txs.len() {
        for j in (i + 1)..txs.len() {
            if transactions_conflict(&access_sets[i], &access_sets[j]) {
                edges.entry(ids[i]).or_default().insert(ids[j]);
                edges.entry(ids[j]).or_default().insert(ids[i]);
            }
        }
    }

    TxConflictGraph { edges }
}

/// A batch of transactions that can be executed concurrently.
#[derive(Debug, Clone)]
pub struct ParallelBatch {
    pub transactions: Vec<Transaction>,
}

/// Schedule transactions in parallel batches using greedy coloring.
pub fn schedule_parallel_batches(txs: Vec<Transaction>) -> Vec<ParallelBatch> {
    let graph = build_conflict_graph(&txs);
    let mut colors: Vec<usize> = vec![0; txs.len()];

    // Keep a map of tx hash -> original index to preserve ordering
    let mut index_map: HashMap<TxId, usize> = HashMap::new();
    for (i, tx) in txs.iter().enumerate() {
        index_map.insert(tx.hash(), i);
    }

    for i in 0..txs.len() {
        let mut forbidden = HashSet::new();
        let id_i = txs[i].hash();
        if let Some(neighbors) = graph.edges.get(&id_i) {
            for nid in neighbors {
                if let Some(pos) = index_map.get(nid) {
                    forbidden.insert(colors[*pos]);
                }
            }
        }
        let mut color = 0;
        while forbidden.contains(&color) {
            color += 1;
        }
        colors[i] = color;
    }

    let mut batches_map: HashMap<usize, ParallelBatch> = HashMap::new();
    for (i, color) in colors.into_iter().enumerate() {
        batches_map
            .entry(color)
            .or_insert_with(|| ParallelBatch { transactions: Vec::new() })
            .transactions
            .push(txs[i].clone());
    }
    let mut batches: Vec<ParallelBatch> = batches_map.into_iter().map(|(_, b)| b).collect();

    // sort batches by the earliest original transaction index to maintain deterministic
    // order and ensure sequential semantics for conflicting transactions
    batches.sort_by_key(|batch| {
        batch
            .transactions
            .iter()
            .map(|tx| index_map.get(&tx.hash()).copied().unwrap_or(usize::MAX))
            .min()
            .unwrap_or(usize::MAX)
    });

    batches
}

/// Overlay for account and storage modifications during parallel execution.
#[derive(Debug, Clone)]
pub struct StateOverlay {
    pub modified_accounts: HashMap<Address, Account>,
}

impl StateOverlay {
    pub fn new() -> Self {
        StateOverlay { modified_accounts: HashMap::new() }
    }

    pub fn read_account(&self, address: &Address, base: &StateManager) -> Account {
        if let Some(a) = self.modified_accounts.get(address) {
            a.clone()
        } else {
            base.get_account(address).cloned().unwrap_or_else(|| Account::new(0, 0))
        }
    }

    pub fn write_account(&mut self, address: Address, account: Account) {
        self.modified_accounts.insert(address, account);
    }
}

/// Overlay for contract storage writes.
#[derive(Debug, Clone)]
pub struct ContractStorageOverlay {
    inner: Arc<std::sync::Mutex<ContractStorageOverlayInner>>,
}

#[derive(Debug)]
pub struct ContractStorageOverlayInner {
    base: Arc<ContractStorage>,
    modified: HashMap<Vec<u8>, Vec<u8>>,
}

impl ContractStorageOverlay {
    pub fn new(base: Arc<ContractStorage>) -> Self {
        ContractStorageOverlay {
            inner: Arc::new(std::sync::Mutex::new(ContractStorageOverlayInner { base, modified: HashMap::new() })),
        }
    }
}

impl Storage for ContractStorageOverlay {
    fn read(&self, key: &[u8]) -> Option<Vec<u8>> {
        let inner = self.inner.lock().unwrap();
        if let Some(v) = inner.modified.get(key) {
            Some(v.clone())
        } else {
            inner.base.read(key)
        }
    }
    fn write(&self, key: &[u8], value: &[u8]) {
        let mut inner = self.inner.lock().unwrap();
        inner.modified.insert(key.to_vec(), value.to_vec());
    }
    fn box_clone(&self) -> Box<dyn Storage> {
        Box::new(self.clone())
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

// additional helper for convenience when mutably holding overlay
impl ContractStorageOverlay {
    pub fn take_modified(&self) -> HashMap<Vec<u8>, Vec<u8>> {
        let inner = self.inner.lock().unwrap();
        inner.modified.clone()
    }
}

impl ContractStorageOverlayInner {
    // helper if needed
}

/// Combine all overlay pieces (state, storage, registry updates)
#[derive(Debug, Clone)]
pub struct Overlay {
    pub state: StateOverlay,
    pub storage: ContractStorageOverlay,
    pub registry_updates: Vec<(Address, Vec<u8>)>,
}

impl Overlay {
    pub fn new(storage_base: Arc<ContractStorage>) -> Self {
        Overlay {
            state: StateOverlay::new(),
            storage: ContractStorageOverlay::new(storage_base),
            registry_updates: Vec::new(),
        }
    }
}

/// Receipt returned after executing a single transaction.
#[derive(Debug, Clone)]
pub struct TransactionReceipt {
    pub tx_hash: TxId,
    pub success: bool,
    pub gas_used: u64,
    pub error: Option<String>,
}

/// Executes a single transaction using a provided overlay.  The global
/// state and storage are passed in as read-only references for fallback
/// reads; writes go to the overlay.  The gas meter is consumed locally.
fn execute_transaction(
    tx: &Transaction,
    overlay: &mut Overlay,
    base_state: &StateManager,
    base_registry: &ContractRegistry,
) -> TransactionReceipt {
    let mut gas_meter = GasMeter::new(tx.gas_limit);
    let mut receipt = TransactionReceipt {
        tx_hash: tx.hash(),
        success: false,
        gas_used: 0,
        error: None,
    };

    // basic validation
    if let Err(e) = tx.validate_basic() {
        receipt.error = Some(e);
        return receipt;
    }

    let from = tx.from;
    let mut sender_acc = overlay.state.read_account(&from, base_state);

    // nonce check
    if sender_acc.nonce != tx.nonce {
        receipt.error = Some(format!("Invalid nonce: expected {} got {}", sender_acc.nonce, tx.nonce));
        return receipt;
    }

    // helper for deducting fee after we know gas used
    let charge_fee = |acc: &mut Account, used: u64| -> Result<(), String> {
        let fee = used.saturating_mul(tx.gas_price);
        if acc.balance < fee {
            Err("Insufficient balance for fee".to_string())
        } else {
            acc.balance = acc.balance.saturating_sub(fee);
            Ok(())
        }
    };

    match &tx.payload {
        TxPayload::Transfer { to, amount } => {
            // check funds (amount + gas limit*price) pessimistically
            let total = amount.saturating_add(tx.gas_limit.saturating_mul(tx.gas_price));
            if sender_acc.balance < total {
                receipt.error = Some("Insufficient balance for transfer".to_string());
                return receipt;
            }
            // update sender
            sender_acc.balance = sender_acc.balance.saturating_sub(*amount);
            sender_acc.nonce += 1;
            overlay.state.write_account(from, sender_acc.clone());
            // credit receiver
            let mut recv_acc = overlay.state.read_account(to, base_state);
            recv_acc.balance = recv_acc.balance.saturating_add(*amount);
            overlay.state.write_account(*to, recv_acc);
            // consume all gas
            gas_meter.used = tx.gas_limit;
            // charge fee (deduct from sender balance)
            if let Err(e) = charge_fee(&mut sender_acc, gas_meter.used) {
                receipt.error = Some(e);
                return receipt;
            }
            overlay.state.write_account(from, sender_acc);
            receipt.success = true;
            receipt.gas_used = gas_meter.used;
        }
        TxPayload::ContractDeploy { wasm_code, init_args: _init_args } => {
            // compute address deterministically
            use sha2::{Digest, Sha256};
            let hash = Sha256::digest(&wasm_code);
            let mut address = [0u8; 20];
            address.copy_from_slice(&hash[0..20]);

            // prepare runtime state using overlay storage
            let runtime_state = RuntimeState {
                contract_address: address,
                caller: tx.from,
                block_height: 0,
                timestamp: 0,
                storage: Box::new(overlay.storage.clone()),
                gas_meter: GasMeter::new(tx.gas_limit),
                memory_limiter: None,
            };
            let (mut store, instance) = match WasmRuntime::new()
                .instantiate_contract(&wasm_code, runtime_state, tx.gas_limit)
            {
                Ok(pair) => pair,
                Err(e) => {
                    receipt.error = Some(format!("WASM instantiate error: {}", e));
                    return receipt;
                }
            };

            let mut success = true;
            if let Some(init_func) = instance.get_func(&mut store, "init") {
                if let Err(_) = init_func.call(&mut store, &[], &mut []) {
                    success = false;
                }
            }
            let used = store.data().gas_meter.used;
            // charge fee
            if let Err(e) = charge_fee(&mut sender_acc, used) {
                receipt.error = Some(e);
                return receipt;
            }
            // increment sender nonce for transaction ordering
            sender_acc.nonce += 1;
            overlay.state.write_account(from, sender_acc.clone());

            if success {
                // record registry update and mark new account as zero
                overlay.registry_updates.push((address, wasm_code.clone()));
                let mut new_acc = overlay.state.read_account(&address, base_state);
                new_acc.balance = 0;
                new_acc.nonce = 0;
                overlay.state.write_account(address, new_acc);
                receipt.success = true;
                receipt.gas_used = used;
            } else {
                receipt.error = Some("contract init failed".to_string());
            }
        }
        TxPayload::ContractCall { contract_address, method, args: _args } => {
            // verify contract exists
            if base_registry.get_contract(contract_address).is_none() {
                receipt.error = Some("Contract not found".to_string());
                return receipt;
            }
            let contract = base_registry.get_contract(contract_address).unwrap();
            let bytecode = &contract.bytecode;

            // run wasm with overlay storage
            let runtime_state = RuntimeState {
                contract_address: *contract_address,
                caller: tx.from,
                block_height: 0,
                timestamp: 0,
                storage: Box::new(overlay.storage.clone()),
                gas_meter: GasMeter::new(tx.gas_limit),
                memory_limiter: None,
            };
            let (mut store, instance) = match WasmRuntime::new()
                .instantiate_contract(bytecode, runtime_state, tx.gas_limit)
            {
                Ok(pair) => pair,
                Err(e) => {
                    receipt.error = Some(format!("WASM instantiate error: {}", e));
                    return receipt;
                }
            };
            let result = WasmRuntime::new().call_function(&mut store, &instance, method, &[]);
            let used = store.data().gas_meter.used;
            if let Err(e) = charge_fee(&mut sender_acc, used) {
                receipt.error = Some(e);
                return receipt;
            }
            // increment nonce for a successful contract call
            sender_acc.nonce += 1;
            overlay.state.write_account(from, sender_acc.clone());
            match result {
                Ok(_) => {
                    receipt.success = true;
                    receipt.gas_used = used;
                }
                Err(e) => {
                    receipt.error = Some(format!("Contract call failed: {}", e));
                }
            }
        }
    }

    receipt
}

/// Merge a single overlay into the global state and storage.
/// Deterministic ordering is guaranteed by the caller sorting by tx hash.
pub fn merge_overlay(
    global: &mut StateManager,
    storage: &ContractStorage,
    registry: &mut ContractRegistry,
    overlay: Overlay,
) {
    // apply account updates in sorted order
    let mut accounts: Vec<_> = overlay.state.modified_accounts.into_iter().collect();
    accounts.sort_by(|a, b| a.0.cmp(&b.0));
    for (addr, acct) in accounts {
        global.state_tree.update_account(addr, acct);
    }
    global.current_state_root = global.state_tree.calculate_root();

    // apply storage updates
    let mut entries: Vec<_> = overlay.storage.take_modified().into_iter().collect();
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    for (key, value) in entries {
        storage.write(&key, &value);
    }

    // apply registry updates
    let mut regs = overlay.registry_updates;
    regs.sort_by(|a, b| a.0.cmp(&b.0));
    for (addr, bytecode) in regs {
        // ignore errors if already registered (shouldn't happen)
        let _ = registry.register_contract(addr, bytecode);
    }
}

/// Entry point for parallel transaction execution.  Takes ownership of the
/// transactions vector, mutably borrows the state manager and contract registry,
/// and returns receipts in the original transaction order.
pub fn execute_block_transactions_parallel(
    txs: Vec<Transaction>,
    state: &mut StateManager,
    registry: &mut ContractRegistry,
    storage: Arc<ContractStorage>,
) -> Vec<TransactionReceipt> {
    let batches = schedule_parallel_batches(txs.clone());
    let mut receipts: Vec<TransactionReceipt> = Vec::new();

    for batch in batches {
        // run transactions in batch in parallel
        let results: Vec<(TxId, Overlay, TransactionReceipt)> = batch
            .transactions
            .par_iter()
            .map(|tx| {
                let mut overlay = Overlay::new(storage.clone());
                let receipt = execute_transaction(tx, &mut overlay, state, registry);
                (tx.hash(), overlay, receipt)
            })
            .collect();

        // sort results by tx hash to ensure deterministic merge order
        let mut sorted = results;
        sorted.sort_by(|a, b| a.0.cmp(&b.0));
        for (_id, overlay, receipt) in sorted {
            if receipt.success {
                merge_overlay(state, &storage, registry, overlay);
            }
            receipts.push(receipt);
        }
    }

    receipts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::transaction::{Transaction, Address};
    use secp256k1::{Secp256k1, SecretKey};
    use sha2::{Digest, Sha256};
    use rand::{rngs::OsRng, RngCore};
    use wat::parse_str;

    fn random_address() -> Address {
        let mut buf = [0u8; 20];
        OsRng.fill_bytes(&mut buf);
        buf
    }

    fn funded_account(state: &mut StateManager, addr: Address, amount: u64) {
        state.state_tree.update_account(addr, Account::new(amount, 0));
        state.current_state_root = state.state_tree.calculate_root();
    }

    fn sign_tx(mut tx: Transaction, privk: &SecretKey) -> Transaction {
        tx.sign(privk).unwrap();
        tx
    }

    #[test]
    fn independent_transfers_parallel() {
        let mut state = StateManager::new();
        let mut registry = ContractRegistry::new("./data/test_contract_registry_parallel.db")
            .expect("Should create registry");
        let storage = Arc::new(ContractStorage::new("test_storage_parallel"));

        let secp = Secp256k1::new();
        let privk1 = SecretKey::new(&mut OsRng);
        let pubk1 = privk1.public_key(&secp);
        let addr1: Address = Sha256::digest(&pubk1.serialize()[1..])[12..32].try_into().unwrap();

        let privk2 = SecretKey::new(&mut OsRng);
        let pubk2 = privk2.public_key(&secp);
        let addr2: Address = Sha256::digest(&pubk2.serialize()[1..])[12..32].try_into().unwrap();

        let rec1 = random_address();
        let rec2 = random_address();

        funded_account(&mut state, addr1, 1_000_000);
        funded_account(&mut state, addr2, 1_000_000);

        let tx1 = sign_tx(Transaction::new_transfer(addr1, rec1, 100, 0, 21000, 1), &privk1);
        let tx2 = sign_tx(Transaction::new_transfer(addr2, rec2, 200, 0, 21000, 1), &privk2);
        let receipts = execute_block_transactions_parallel(vec![tx1.clone(), tx2.clone()], &mut state, &mut registry, storage.clone());

        assert_eq!(receipts.len(), 2);
        assert!(receipts.iter().all(|r| r.success));
        assert_ne!(state.get_account(&rec1).unwrap().balance, 0);
        assert_ne!(state.get_account(&rec2).unwrap().balance, 0);
    }

    #[test]
    fn conflicting_transfers_sequential() {
        let mut state = StateManager::new();
        let mut registry = ContractRegistry::new("./data/test_contract_registry_conflict.db")
            .expect("Should create registry");
        let storage = Arc::new(ContractStorage::new("test_storage_conflict"));

        let secp = Secp256k1::new();
        let privk = SecretKey::new(&mut OsRng);
        let pubk = privk.public_key(&secp);
        let addr: Address = Sha256::digest(&pubk.serialize()[1..])[12..32].try_into().unwrap();
        let rec_a = random_address();
        let rec_b = random_address();

        funded_account(&mut state, addr, 1_000_000);

        let tx1 = sign_tx(Transaction::new_transfer(addr, rec_a, 100, 0, 21000, 1), &privk);
        // second tx should use the next nonce to be valid after tx1
        let tx2 = sign_tx(Transaction::new_transfer(addr, rec_b, 200, 1, 21000, 1), &privk);
        let receipts = execute_block_transactions_parallel(vec![tx1.clone(), tx2.clone()], &mut state, &mut registry, storage.clone());

        // both should succeed but one will run after the other because they conflict
        assert_eq!(receipts.len(), 2);
        assert!(receipts[0].success && receipts[1].success);
        // nonce and balances progressed sequentially
        let final_acc = state.get_account(&addr).unwrap();
        assert_eq!(final_acc.nonce, 2);
    }

    #[test]
    fn contract_call_executes() {
        let mut state = StateManager::new();
        let mut registry = ContractRegistry::new("./data/test_contract_registry_call.db")
            .expect("Should create registry");
        let storage = Arc::new(ContractStorage::new("test_storage_contract"));

        let secp = Secp256k1::new();
        let privk = SecretKey::new(&mut OsRng);
        let pubk = privk.public_key(&secp);
        let addr: Address = Sha256::digest(&pubk.serialize()[1..])[12..32].try_into().unwrap();
        funded_account(&mut state, addr, 1_000_000);

        // create a trivial wasm module that exports a function "foo" that does nothing
        let wat = r#"(module (func (export "foo") (nop)))"#;
        let wasm = parse_str(wat).unwrap();

        let deploy_tx = sign_tx(Transaction::new_deploy(addr, wasm.clone(), vec![], 0, 100000, 1), &privk);
        let call_tx = sign_tx(Transaction::new_call(addr, deploy_tx.hash()[0..20].try_into().unwrap(), "foo".to_string(), vec![], 1, 100000, 1), &privk);
        let receipts = execute_block_transactions_parallel(vec![deploy_tx.clone(), call_tx.clone()], &mut state, &mut registry, storage.clone());

        assert_eq!(receipts.len(), 2);
        assert!(receipts[0].success, "Deploy failed: {:?}", receipts[0].error);
        assert!(receipts[1].success, "Call failed: {:?}", receipts[1].error);
        assert!(receipts[0].gas_used > 0);
        assert!(receipts[1].gas_used > 0);
        // state root deterministic check
        let root1 = state.get_state_root();
        let root2 = state.get_state_root();
        assert_eq!(root1, root2);
    }

    #[test]
    fn gas_accounting_and_overlay() {
        let mut state = StateManager::new();
        let mut registry = ContractRegistry::new("./data/test_contract_registry_gas.db")
            .expect("Should create registry");
        let storage = Arc::new(ContractStorage::new("test_storage_gas"));

        let secp = Secp256k1::new();
        let privk = SecretKey::new(&mut OsRng);
        let pubk = privk.public_key(&secp);
        let addr: Address = Sha256::digest(&pubk.serialize()[1..])[12..32].try_into().unwrap();
        funded_account(&mut state, addr, 1_000_000);

        let tx = sign_tx(Transaction::new_transfer(addr, random_address(), 100, 0, 21000, 2), &privk);
        let receipts = execute_block_transactions_parallel(vec![tx.clone()], &mut state, &mut registry, storage.clone());
        assert_eq!(receipts.len(), 1);
        assert!(receipts[0].success);
        assert_eq!(receipts[0].gas_used, 21000);
        // overlay merge should have deducted fee correctly
        let final_acc = state.get_account(&addr).unwrap();
        assert!(final_acc.balance < 1_000_000 - 100);
    }
}
