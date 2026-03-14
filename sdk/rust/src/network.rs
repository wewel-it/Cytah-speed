use crate::client::{Client, DagInfo, NodeInfo};
use crate::errors::SdkError;

/// High-level network utilities.
///
/// This module re-exports `Client` helpers for retrieving node / DAG information.
pub struct Network {
    client: Client,
}

impl Network {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    /// Returns basic node runtime information (peers, mempool size, DAG height).
    pub async fn node_info(&self) -> Result<NodeInfo, SdkError> {
        self.client.get_node_info().await
    }

    /// Returns DAG information such as tip hashes and block count.
    pub async fn dag_info(&self) -> Result<DagInfo, SdkError> {
        self.client.get_dag_info().await
    }

    /// Waits until the node reports at least `target_height` blocks (or times out).
    pub async fn wait_for_height(
        &self,
        target_height: usize,
        max_attempts: usize,
        delay_ms: u64,
    ) -> Result<(), SdkError> {
        for _ in 0..max_attempts {
            let info = self.client.get_node_info().await?;
            if info.dag_height >= target_height {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        }
        Err(SdkError::RpcError(format!(
            "Node did not reach target height {} after {} attempts",
            target_height, max_attempts
        )))
    }
}
