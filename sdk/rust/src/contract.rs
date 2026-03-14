use cytah_core::Address;
use crate::client::{Client, DeployContractResult, CallContractResult};
use crate::errors::SdkError;

/// High-level contract helper that speaks to a Cytah-Speed node.
///
/// This wrapper provides a small convenience layer around the RPC contract
/// endpoints (deploy/call).
#[derive(Clone, Debug)]
pub struct ContractClient {
    inner: Client,
}

impl ContractClient {
    pub fn new(node_url: impl Into<String>) -> Self {
        Self {
            inner: Client::new(node_url),
        }
    }

    /// Deploy a WASM contract.
    ///
    /// Returns the contract address and the transaction hash of the deployment.
    pub async fn deploy(
        &self,
        from: Address,
        nonce: u64,
        wasm_code: Vec<u8>,
        init_args: Option<Vec<u8>>,
    ) -> Result<DeployContractResult, SdkError> {
        self.inner
            .deploy_contract(from, nonce, wasm_code, init_args)
            .await
    }

    /// Call a contract method.
    pub async fn call(
        &self,
        from: Address,
        nonce: u64,
        contract_address: String,
        method: String,
        args: Option<Vec<u8>>,
    ) -> Result<CallContractResult, SdkError> {
        self.inner
            .call_contract(from, nonce, contract_address, method, args)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_contract_client_new() {
        let client = ContractClient::new("http://127.0.0.1:0");
        // No real server; just ensure constructor works and can clone the inner client.
        let _ = client.inner.clone();
    }
}
