use crate::wallet::Wallet;
use crate::transaction::TransactionBuilder;
use crate::errors::SdkError;
use cytah_core::{Address, Transaction as CoreTransaction};

/// Mobile-optimized wallet with lightweight operations
pub struct MobileWallet {
    wallet: Wallet,
    nonce_cache: std::sync::Mutex<std::collections::HashMap<Address, u64>>,
}

impl MobileWallet {
    /// Create a new mobile wallet
    pub fn create() -> Result<Self, SdkError> {
        let wallet = Wallet::create_wallet()?;
        Ok(MobileWallet {
            wallet,
            nonce_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// Import wallet from private key
    pub fn from_private_key(private_key: &[u8]) -> Result<Self, SdkError> {
        let wallet = Wallet::import_private_key(private_key)?;
        Ok(MobileWallet {
            wallet,
            nonce_cache: std::sync::Mutex::new(std::collections::HashMap::new()),
        })
    }

    /// Get wallet address
    pub fn address(&self) -> &Address {
        &self.wallet.address
    }

    /// Get balance (requires mobile client)
    pub async fn get_balance(&self, client: &crate::mobile::mobile_client::MobileClient) -> Result<crate::client::Balance, SdkError> {
        client.get_balance(self.wallet.address).await
    }

    /// Send tokens (convenience method)
    pub async fn send_tokens(
        &self,
        client: &crate::mobile::mobile_client::MobileClient,
        to: Address,
        amount: u64,
        gas_price: u64,
    ) -> Result<String, SdkError> {
        // Ensure nonce is current from the network
        self.sync_nonce(client).await?;
        let nonce = self.get_next_nonce(self.address());

        let tx = TransactionBuilder::new()
            .from(*self.address())
            .transfer(to, amount)
            .nonce(nonce)
            .gas_limit(21000)
            .gas_price(gas_price)
            .build_and_sign(&self.wallet)?;

        let tx_hash = client.send_transaction(&tx).await?;

        // Update nonce cache
        self.update_nonce(*self.address(), nonce + 1);

        Ok(tx_hash)
    }

    /// Call contract (convenience method)
    pub async fn call_contract(
        &self,
        client: &crate::mobile::mobile_client::MobileClient,
        contract_address: Address,
        method: String,
        args: Vec<u8>,
        gas_limit: u64,
        gas_price: u64,
    ) -> Result<String, SdkError> {
        // Ensure nonce is current from the network
        self.sync_nonce(client).await?;
        let nonce = self.get_next_nonce(self.address());

        let tx = TransactionBuilder::new()
            .from(*self.address())
            .call_contract(contract_address, method, args)
            .nonce(nonce)
            .gas_limit(gas_limit)
            .gas_price(gas_price)
            .build_and_sign(&self.wallet)?;

        let tx_hash = client.send_transaction(&tx).await?;

        // Update nonce cache
        self.update_nonce(*self.address(), nonce + 1);

        Ok(tx_hash)
    }

    /// Deploy contract (convenience method)
    pub async fn deploy_contract(
        &self,
        client: &crate::mobile::mobile_client::MobileClient,
        wasm_code: Vec<u8>,
        init_args: Vec<u8>,
        gas_limit: u64,
        gas_price: u64,
    ) -> Result<String, SdkError> {
        // Ensure nonce is current from the network
        self.sync_nonce(client).await?;
        let nonce = self.get_next_nonce(self.address());

        let tx = TransactionBuilder::new()
            .from(*self.address())
            .deploy_contract(wasm_code, init_args)
            .nonce(nonce)
            .gas_limit(gas_limit)
            .gas_price(gas_price)
            .build_and_sign(&self.wallet)?;

        let tx_hash = client.send_transaction(&tx).await?;

        // Update nonce cache
        self.update_nonce(*self.address(), nonce + 1);

        Ok(tx_hash)
    }

    /// Export private key (use with caution)
    pub fn export_private_key(&self) -> String {
        self.wallet.export_private_key_hex()
    }

    /// Get next nonce for address (from cache)
    fn get_next_nonce(&self, address: &Address) -> u64 {
        *self.nonce_cache.lock().unwrap().get(address).unwrap_or(&0)
    }

    /// Update nonce cache
    fn update_nonce(&self, address: Address, nonce: u64) {
        self.nonce_cache.lock().unwrap().insert(address, nonce);
    }

    /// Sync nonce from network (should be called periodically)
    pub async fn sync_nonce(
        &self,
        client: &crate::mobile::mobile_client::MobileClient,
    ) -> Result<(), SdkError> {
        let balance = client.get_balance(self.address()).await?;
        self.update_nonce(*self.address(), balance.nonce);
        Ok(())
    }

    /// Clear nonce cache
    pub fn clear_nonce_cache(&self) {
        self.nonce_cache.lock().unwrap().clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mobile_wallet_creation() {
        let wallet = MobileWallet::create().unwrap();
        assert_eq!(wallet.address().len(), 20);
    }

    #[test]
    fn test_nonce_cache() {
        let wallet = MobileWallet::create().unwrap();
        let address = *wallet.address();

        // Initially 0
        assert_eq!(wallet.get_next_nonce(&address), 0);

        // Update nonce
        wallet.update_nonce(address, 5);
        assert_eq!(wallet.get_next_nonce(&address), 5);

        // Clear cache
        wallet.clear_nonce_cache();
        assert_eq!(wallet.get_next_nonce(&address), 0);
    }
}