# Cytah-Speed SDK Guide

This guide shows how to use the built-in **Cytah-Speed SDK** to interact with a running node over the RPC API.

> The SDK is part of the `cytah-core` crate and is available via `cytah_core::sdk` once you add `cytah-core` as a dependency.

---

## 1) Setup

Add `cytah-core` to your `Cargo.toml`:

```toml
[dependencies]
cytah-core = { path = "../" } # or a published version
```

Then in your code:

```rust
use cytah_core::sdk::{Client, Wallet, TransactionBuilder};
use cytah_core::core::Address;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let client = Client::new("http://127.0.0.1:8080");
    Ok(())
}
```

---

## 2) Wallet + Keys

### Create a new wallet

```rust
let wallet = Wallet::create_wallet()?;
println!("Address: cyt{}", hex::encode(wallet.address));
```

### Import from an existing private key

```rust
let secret_bytes = hex::decode("...")?;
let wallet = Wallet::import_private_key(&secret_bytes)?;
```

### Sign a transaction

```rust
let mut tx = TransactionBuilder::new()
    .from(wallet.address)
    .transfer(recipient, 1_000)
    .nonce(1)
    .gas_limit(21000)
    .gas_price(1)
    .build()?;

wallet.sign_transaction(&mut tx)?;
```

---

## 3) Sending Transactions

```rust
let client = Client::new("http://127.0.0.1:8080");
client.send_transaction(&tx).await?;
```

---

## 4) Querying Node State

```rust
let balance = client.get_balance(wallet.address).await?;
println!("balance={} nonce={}", balance.balance, balance.nonce);

let dag = client.get_dag_info().await?;
let node = client.get_node_info().await?;
```

---

## 5) Contracts

### Deploy

```rust
let deploy_result = client
    .deploy_contract(wallet.address, 1, wasm_bytes, None)
    .await?;
println!("contract_address = {}", deploy_result.contract_address);
```

### Call

```rust
let call_result = client
    .call_contract(
        wallet.address,
        2,
        deploy_result.contract_address.clone(),
        "my_method".to_string(),
        Some(args_bytes),
    )
    .await?;
println!("call status = {}", call_result.status);
```

---

## 6) Network Helpers

Use `cytah_core::sdk::network::Network` when you need higher-level helpers like waiting for a tip height.

```rust
use cytah_core::sdk::network::Network;

let client = Client::new("http://127.0.0.1:8080");
let net = Network::new(client);
net.wait_for_height(10, 20, 500).await?;
```

---

## Notes

- The SDK uses the node's HTTP RPC API; make sure the node's RPC server is running (`rpc_addr` when starting the node).
- Address encoding uses the `cyt<hex>` prefix that the node expects.
