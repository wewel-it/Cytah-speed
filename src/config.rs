/// Configuration loader untuk Cytah-Speed node
/// Mendukung loading dari config.toml dengan default fallback

use serde::{Deserialize, Serialize};
use std::path::Path;
use toml;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeConfig {
    pub network: NetworkConfig,
    pub sync: SyncConfig,
    pub contracts: ContractConfig,
    pub consensus: ConsensusConfig,
    pub mempool: MempoolConfig,
    pub storage: StorageConfig,
    pub logging: LoggingConfig,
    pub node: NodeSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    pub listen_addr: String,
    pub rpc_addr: String,
    pub enable_mdns: bool,
    pub enable_dht: bool,
    pub bootstrap_peers: Vec<String>,
    pub max_peers: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    pub periodic_check_interval: u64,
    pub max_pending_requests: usize,
    pub block_request_timeout: u64,
    pub max_batch_size: usize,
    pub state: StateSyncConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSyncConfig {
    pub enable_fast_sync: bool,
    pub snapshot_interval: u64,
    pub max_snapshot_size: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractConfig {
    pub db_path: String,
    pub max_bytecode_size: u64,
    pub enable_caching: bool,
    pub cache_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsensusConfig {
    pub block_interval: u64,
    pub confirmation_depth: usize,
    pub blue_set_size: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MempoolConfig {
    pub max_size: usize,
    pub min_gas_price: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub db_path: String,
    pub pruning: PruningConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PruningConfig {
    pub enabled: bool,
    pub prune_height: u64,
    pub window_size: u64,
    pub check_interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    pub level: String,
    pub structured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSettings {
    pub node_id: String,
    pub data_dir: String,
    pub node_name: String,
    pub enable_rpc: bool,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            network: NetworkConfig {
                listen_addr: "/ip4/0.0.0.0/tcp/30333".to_string(),
                rpc_addr: "127.0.0.1:8080".to_string(),
                enable_mdns: true,
                enable_dht: false,
                bootstrap_peers: vec![],
                max_peers: 100,
            },
            sync: SyncConfig {
                periodic_check_interval: 60,
                max_pending_requests: 1000,
                block_request_timeout: 30,
                max_batch_size: 100,
                state: StateSyncConfig {
                    enable_fast_sync: true,
                    snapshot_interval: 300,
                    max_snapshot_size: 500 * 1024 * 1024, // 500 MB
                },
            },
            contracts: ContractConfig {
                db_path: "./data/contracts.db".to_string(),
                max_bytecode_size: 1024 * 1024, // 1 MB
                enable_caching: true,
                cache_size: 1000,
            },
            consensus: ConsensusConfig {
                block_interval: 5000,
                confirmation_depth: 10,
                blue_set_size: 2,
            },
            mempool: MempoolConfig {
                max_size: 10000,
                min_gas_price: 1,
            },
            storage: StorageConfig {
                db_path: "./data/blocks.db".to_string(),
                pruning: PruningConfig {
                    enabled: true,
                    prune_height: 200000,
                    window_size: 100000,
                    check_interval: 3600,
                },
            },
            logging: LoggingConfig {
                level: "info".to_string(),
                structured: false,
            },
            node: NodeSettings {
                node_id: String::new(),
                data_dir: "./data".to_string(),
                node_name: "cytah-node".to_string(),
                enable_rpc: true,
            },
        }
    }
}

impl NodeConfig {
    /// Load configuration dari file TOML
    /// Fallback ke defaults jika file tidak ditemukan
    pub fn load_or_default(config_path: &str) -> Result<Self, String> {
        let path = Path::new(config_path);
        
        if !path.exists() {
            tracing::warn!(
                "Config file not found at {}, using defaults",
                config_path
            );
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read config file: {}", e))?;

        let config: Self = toml::from_str(&content)
            .map_err(|e| format!("Failed to parse config file: {}", e))?;

        tracing::info!("Configuration loaded from {}", config_path);
        Ok(config)
    }

    /// Load configuration dengan priority:
    /// 1. Command-line config path (if provided)
    /// 2. Default config.toml
    /// 3. Hardcoded defaults
    pub fn load_with_priority(cli_config: Option<&str>) -> Result<Self, String> {
        if let Some(path) = cli_config {
            Self::load_or_default(path)
        } else {
            Self::load_or_default("config.toml")
        }
    }

    /// Validate configuration untuk sane defaults
    pub fn validate(&self) -> Result<(), Vec<String>> {
        let mut errors = Vec::new();

        if self.consensus.block_interval == 0 {
            errors.push("block_interval must be > 0".to_string());
        }

        if self.mempool.max_size == 0 {
            errors.push("mempool.max_size must be > 0".to_string());
        }

        if self.consensus.confirmation_depth == 0 {
            errors.push("confirmation_depth must be > 0".to_string());
        }

        if self.network.max_peers == 0 {
            errors.push("network.max_peers must be > 0".to_string());
        }

        if self.sync.max_pending_requests == 0 {
            errors.push("sync.max_pending_requests must be > 0".to_string());
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors)
        }
    }

    /// Print configuration summary untuk debugging
    pub fn print_summary(&self) {
        println!("\n╔════════════════════════════════════════════╗");
        println!("║ Cytah-Speed Node Configuration             ║");
        println!("╚════════════════════════════════════════════╝");
        println!("\n[Network]");
        println!("  Listen Address: {}", self.network.listen_addr);
        println!("  RPC Address: {}", self.network.rpc_addr);
        println!("  mDNS Enabled: {}", self.network.enable_mdns);
        println!("  Bootstrap Peers: {}", self.network.bootstrap_peers.len());
        println!("  Max Peers: {}", self.network.max_peers);

        println!("\n[Consensus]");
        println!("  Block Interval: {} ms", self.consensus.block_interval);
        println!("  Confirmation Depth: {} blocks", self.consensus.confirmation_depth);
        println!("  Blue Set Size: {}", self.consensus.blue_set_size);

        println!("\n[Storage]");
        println!("  Data Directory: {}", self.node.data_dir);
        println!("  Block DB: {}", self.storage.db_path);
        println!("  Contract DB: {}", self.contracts.db_path);
        println!("  Pruning Enabled: {}", self.storage.pruning.enabled);

        println!("\n[Synchronization]");
        println!("  Periodic Check Interval: {} seconds", self.sync.periodic_check_interval);
        println!("  Fast State Sync: {}", self.sync.state.enable_fast_sync);
        println!("  Max Pending Requests: {}", self.sync.max_pending_requests);

        println!("\n[Mempool]");
        println!("  Max Size: {} transactions", self.mempool.max_size);
        println!("  Min Gas Price: {} wei", self.mempool.min_gas_price);

        println!("\n");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = NodeConfig::default();
        assert_eq!(config.consensus.block_interval, 5000);
        assert_eq!(config.network.max_peers, 100);
    }

    #[test]
    fn test_config_validation() {
        let config = NodeConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_fails_on_zero_block_interval() {
        let mut config = NodeConfig::default();
        config.consensus.block_interval = 0;
        assert!(config.validate().is_err());
    }
}
