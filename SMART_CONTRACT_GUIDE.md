# Smart Contract WASM Runtime - Quick Reference Guide

## Architecture at a Glance

```
┌─────────────────────────────────────────────────────────┐
│                    Cytah-Speed Node                      │
├─────────────────────────────────────────────────────────┤
│                                                           │
│  Transactions (3 types)                                 │
│  ├─ Transfer (UTXO)                                     │
│  ├─ ContractDeploy                                      │
│  └─ ContractCall                                        │
│           ↓                                              │
│  TransactionExecutor                                    │
│           ↓                                              │
│  ContractExecutor ───→ StateManager (UTXO)             │
│  ├─ WasmRuntime                                         │
│  ├─ ContractRegistry                                    │
│  ├─ ContractStorage (RocksDB)                          │
│  └─ GasMeter                                            │
│                                                           │
│  RPC Server                                             │
│  ├─ /contract/deploy                                    │
│  └─ /contract/call                                      │
│                                                           │
│  CLI                                                    │
│  ├─ cyt contract deploy                                 │
│  └─ cyt contract call                                   │
│                                                           │
└─────────────────────────────────────────────────────────┘
```

## Execution Flow

### 1. Deploy Contract
```
User/CLI
  ↓
POST /contract/deploy (hex WASM code)
  ↓
TransactionExecutor::execute_transaction()
  ↓
ContractExecutor::deploy_contract()
  ├─ Hash WASM → address
  ├─ Store in ContractRegistry
  ├─ Create ContractStorage space
  └─ Call init() if exists
  ↓
ContractStorage (RocksDB) ← Contract State
```

### 2. Call Contract
```
User/CLI
  ↓
POST /contract/call (address, method, args)
  ↓
TransactionExecutor::execute_transaction()
  ↓
ContractExecutor::call_contract()
  ├─ Lookup in ContractRegistry
  ├─ Instantiate WASM in WasmRuntime
  ├─ Call method with args
  └─ Access storage via host functions
  ↓
ContractStorage (RocksDB) ← Contract State
```

## Key Components

### Transactions (src/core/transaction.rs)

```rust
pub enum TxPayload {
    Transfer { to: Address, amount: u64 },
    ContractDeploy { wasm_code: Vec<u8>, init_args: Vec<u8> },
    ContractCall { contract_address: Address, method: String, args: Vec<u8> },
}

pub struct Transaction {
    pub from: Address,
    pub payload: TxPayload,
    pub nonce: u64,
    pub gas_limit: u64,
    pub signature: Vec<u8>,
}
```

### WASM Runtime (src/vm/wasm_runtime.rs)

```rust
pub struct RuntimeState {
    pub contract_address: [u8;20],
    pub caller: [u8;20],
    pub block_height: u64,
    pub timestamp: u64,
    pub storage: ContractStorage,
}

pub struct WasmRuntime {
    engine: Engine,
    linker: Linker<RuntimeState>,
}

impl WasmRuntime {
    pub fn instantiate_contract(&self, bytecode: &[u8], state: RuntimeState) 
        -> Result<(Store<RuntimeState>, Instance)>
    
    pub fn call_function(&self, store: &mut Store<RuntimeState>, 
        instance: &Instance, func_name: &str, args: &[Val]) 
        -> Result<Box<[Val]>>
}
```

### Contract Executor (src/vm/contract_executor.rs)

```rust
pub struct ContractExecutor {
    pub state_manager: StateManager,
    pub registry: ContractRegistry,
    pub storage: ContractStorage,
    pub runtime: WasmRuntime,
}

impl ContractExecutor {
    pub fn execute_transaction(&mut self, tx: &Transaction) -> Result<(), String>
    pub fn deploy_contract(&mut self, tx: &Transaction, wasm_code: Vec<u8>, 
        init_args: Vec<u8>) -> Result<(), String>
    pub fn call_contract(&mut self, tx: &Transaction, contract_address: [u8; 20], 
        method: String, args: Vec<u8>) -> Result<(), String>
}
```

### Contract Storage (src/contracts/contract_storage.rs)

```rust
pub struct ContractStorage {
    db: Arc<DB>, // Thread-safe RocksDB
}

impl ContractStorage {
    pub fn write(&self, key: &[u8], value: &[u8])
    pub fn read(&self, key: &[u8]) -> Option<Vec<u8>>
}
```

### Contract Registry (src/contracts/contract_registry.rs)

```rust
pub struct ContractRegistry {
    contracts: HashMap<[u8;20], ContractInfo>,
}

impl ContractRegistry {
    pub fn register_contract(&mut self, address: [u8;20], bytecode: Vec<u8>) 
        -> Result<(), String>
    pub fn get_contract(&self, address: &[u8;20]) -> Option<&ContractInfo>
}
```

## Host Functions

WASM contracts can import these functions:

```c
// Storage access
i32 storage_read(i32 key_ptr, i32 key_len, i32 out_ptr);
i32 storage_write(i32 key_ptr, i32 key_len, i32 val_ptr, i32 val_len);

// Context access
i32 get_caller(i32 ptr);  // Writes 20-byte address to memory
i64 get_block_height();
i64 get_timestamp();
```

## Binary Execution

### Transaction Creation (Transfer)
```rust
let mut tx = Transaction::new_transfer(from, to, amount, nonce, gas_limit);
tx.sign(&private_key)?;
```

### Transaction Creation (Deploy)
```rust
let mut tx = Transaction::new_deploy(from, wasm_code, init_args, nonce, gas_limit);
tx.sign(&private_key)?;
```

### Transaction Creation (Call)
```rust
let mut tx = Transaction::new_call(from, contract_addr, method, args, nonce, gas_limit);
tx.sign(&private_key)?;
```

## RPC API

### Deploy Contract
```bash
curl -X POST http://localhost:3000/contract/deploy \
  -H "Content-Type: application/json" \
  -d '{
    "wasm_code": "0x...",  // hex-encoded WASM bytecode
    "init_args": "0x..."   // optional hex-encoded init args
  }'

Response:
{
  "contract_address": "cyt...",
  "tx_hash": "0x..."
}
```

### Call Contract
```bash
curl -X POST http://localhost:3000/contract/call \
  -H "Content-Type: application/json" \
  -d '{
    "contract_address": "cyt...",
    "method": "transfer",
    "args": "0x..."  // optional hex-encoded args
  }'

Response:
{
  "status": "success",
  "result": null
}
```

## CLI Usage

### Deploy
```bash
cyt contract deploy \
  --wasm /path/to/contract.wasm \
  --wallet /path/to/wallet.json \
  --rpc-url http://localhost:3000
```

### Call
```bash
cyt contract call \
  --contract cytXXXXXXXXXXXXXXXXXXXXXXXXXX \
  --method transfer \
  --args "0x..." \
  --rpc-url http://localhost:3000
```

## Smart Contract Example (Rust)

```rust
#[no_mangle]
pub extern "C" fn init() {
    // Called during contract deployment
}

#[no_mangle]
pub extern "C" fn transfer(to_ptr: i32, to_len: i32, amount: i64) -> i32 {
    // Called when contract is invoked
    // Access host functions via imported functions
    0 // Return code
}
```

## State Management

### UTXO (Transfers)
- Handled by StateManager
- Uses SparseMerkleTree
- Tracks account balance and nonce

### Contract State
- Handled by ContractStorage
- Uses RocksDB backend
- Key-value storage per contract
- Addressable via contract address

## Gas Metering

```rust
pub struct GasMeter {
    pub limit: u64,
    pub used: u64,
}

impl GasMeter {
    pub fn charge(&mut self, amount: u64) -> Result<(), String>
}
```

Gas is tracked per transaction via gas_limit field.

## Data Persistence

- **Block data**: BlockDAG + BlockStore
- **Account state**: StateManager's SparseMerkleTree
- **Contract state**: ContractStorage (RocksDB)
- **Contract code**: ContractRegistry (in-memory)

## Thread Safety

- StateManager: Arc<Mutex<StateManager>>
- ContractStorage: Arc<DB> (RocksDB is thread-safe)
- BlockDAG: Arc<RwLock<BlockDAG>>
- WasmRuntime: Interior mutability via Engine

## Error Handling

All operations return Result<T, String> or Result<T, E>:
- Invalid transactions are rejected
- Contract execution errors are caught
- Out-of-gas is detected
- Missing contracts are reported
- Storage errors are propagated

## Performance Considerations

1. **Bytecode caching**: Compiled WASM cached in ContractRegistry
2. **Storage efficiency**: RocksDB provides efficient key-value access
3. **State isolation**: Each contract has separate storage
4. **Gas limits**: Prevent runaway execution
5. **Parallel execution**: Each contract instance is independent

## Security Notes

- Contract addresses derived from bytecode hash (deterministic)
- Storage access isolated per contract
- Caller identity available to contracts
- Block height/timestamp accessible for time-based logic
- Signature validation before execution

## Integration Points

1. **Block execution**: TransactionExecutor → ContractExecutor
2. **State updates**: ContractExecutor → StateManager
3. **Storage**: ContractExecutor → ContractStorage
4. **Mempool**: Transactions queued normally
5. **Finality**: Contracts finalized with blocks

---

**For full implementation details, see WASM_IMPLEMENTATION.md and SMART_CONTRACT_COMPLETION.md**
