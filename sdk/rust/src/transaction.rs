use cytah_core::{Transaction, Address};
use cytah_core::core::transaction::TxPayload;
use crate::errors::SdkError;

/// Builder for creating Cytah-Speed transactions.
#[derive(Debug, Default)]
pub struct TransactionBuilder {
    from: Option<Address>,
    payload: Option<TxPayload>,
    nonce: Option<u64>,
    gas_limit: Option<u64>,
    gas_price: Option<u64>,
}

impl TransactionBuilder {
    /// Create a new builder instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the sender address
    pub fn from(mut self, from: Address) -> Self {
        self.from = Some(from);
        self
    }

    /// Set transfer payload
    pub fn transfer(mut self, to: Address, amount: u64) -> Self {
        self.payload = Some(TxPayload::Transfer { to, amount });
        self
    }

    /// Set contract deployment payload
    pub fn deploy_contract(mut self, wasm_code: Vec<u8>, init_args: Vec<u8>) -> Self {
        self.payload = Some(TxPayload::ContractDeploy { wasm_code, init_args });
        self
    }

    /// Set contract call payload
    pub fn call_contract(mut self, contract_address: Address, method: String, args: Vec<u8>) -> Self {
        self.payload = Some(TxPayload::ContractCall { contract_address, method, args });
        self
    }

    /// Set nonce
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = Some(nonce);
        self
    }

    /// Set gas limit
    pub fn gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = Some(gas_limit);
        self
    }

    /// Set gas price
    pub fn gas_price(mut self, gas_price: u64) -> Self {
        self.gas_price = Some(gas_price);
        self
    }

    /// Build the transaction
    pub fn build(self) -> Result<Transaction, SdkError> {
        let from = self.from.ok_or_else(|| SdkError::TransactionError("Missing from address".into()))?;
        let payload = self.payload.ok_or_else(|| SdkError::TransactionError("Missing payload".into()))?;
        let nonce = self.nonce.unwrap_or(0);
        let gas_limit = self.gas_limit.unwrap_or(21000);
        let gas_price = self.gas_price.unwrap_or(1);

        let tx = match payload {
            TxPayload::Transfer { to, amount } => Transaction::new_transfer(from, to, amount, nonce, gas_limit, gas_price),
            TxPayload::ContractDeploy { wasm_code, init_args } => Transaction::new_deploy(from, wasm_code, init_args, nonce, gas_limit, gas_price),
            TxPayload::ContractCall { contract_address, method, args } => Transaction::new_call(from, contract_address, method, args, nonce, gas_limit, gas_price),
        };

        Ok(tx)
    }

    /// Build and sign the transaction using the provided wallet.
    pub fn build_and_sign(self, wallet: &crate::wallet::Wallet) -> Result<Transaction, SdkError> {
        let mut tx = self.build()?;
        wallet.sign_transaction(&mut tx)?;
        Ok(tx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transaction_builder_transfer() {
        let from = [1u8; 20];
        let to = [2u8; 20];
        let tx = TransactionBuilder::new()
            .from(from)
            .transfer(to, 100)
            .nonce(1)
            .gas_limit(21000)
            .gas_price(1)
            .build()
            .expect("build tx");

        assert_eq!(tx.from, from);
        if let TxPayload::Transfer { to: t, amount } = tx.payload {
            assert_eq!(t, to);
            assert_eq!(amount, 100);
        } else {
            panic!("expected transfer payload");
        }
    }

    #[test]
    fn test_transaction_builder_missing_field() {
        let res = TransactionBuilder::new().build();
        assert!(res.is_err());
    }
}
