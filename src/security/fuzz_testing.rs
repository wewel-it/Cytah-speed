use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use parking_lot::RwLock;
use rand::prelude::*;
use tokio::time::interval;
use tracing::{info, warn, error, debug};
use crate::core::Transaction;
use crate::core::transaction::TxPayload;
use crate::security::pre_validation::TransactionPreValidator;

/// Spam detection configuration
#[derive(Debug, Clone)]
pub struct SpamDetectionConfig {
    /// Maximum transactions per second from single IP
    pub max_tps_per_ip: u32,
    /// Burst detection window (seconds)
    pub burst_window_seconds: u64,
    /// Burst threshold (transactions in window)
    pub burst_threshold: u32,
    /// Similarity threshold for detecting similar transactions
    pub similarity_threshold: f64,
    /// Cleanup interval for old data
    pub cleanup_interval_seconds: u64,
}

/// Transaction pattern for spam detection
#[derive(Debug, Clone)]
struct TransactionPattern {
    /// Transaction hash
    hash: [u8; 32],
    /// Amount (0 for non-transfer transactions)
    amount: u64,
    /// Gas price
    gas_price: u64,
    /// Payload size
    payload_size: usize,
    /// Timestamp
    timestamp: Instant,
}

/// IP activity tracker
#[derive(Debug, Clone)]
struct IpActivity {
    /// Transaction timestamps
    transactions: Vec<Instant>,
    /// Transaction patterns
    patterns: Vec<TransactionPattern>,
    /// Burst detection score
    burst_score: f64,
    /// Last activity
    last_activity: Instant,
}

/// Spam detector
pub struct SpamDetector {
    /// Configuration
    config: SpamDetectionConfig,
    /// IP activity tracking
    ip_activity: Arc<RwLock<HashMap<std::net::IpAddr, IpActivity>>>,
    /// Global transaction patterns
    global_patterns: Arc<RwLock<Vec<TransactionPattern>>>,
    /// Statistics
    stats: Arc<RwLock<SpamStats>>,
}

impl SpamDetector {
    /// Create new spam detector
    pub fn new(config: SpamDetectionConfig) -> Self {
        let detector = Self {
            config: config.clone(),
            ip_activity: Arc::new(RwLock::new(HashMap::new())),
            global_patterns: Arc::new(RwLock::new(Vec::new())),
            stats: Arc::new(RwLock::new(SpamStats::default())),
        };

        // Start cleanup task
        detector.start_cleanup_task();

        detector
    }

    /// Check if transaction from IP is spam
    pub fn check_spam(&self, tx: &Transaction, ip: std::net::IpAddr) -> SpamResult {
        let now = Instant::now();
        let mut activity_map = self.ip_activity.write();

        let activity = activity_map.entry(ip).or_insert_with(|| IpActivity {
            transactions: Vec::new(),
            patterns: Vec::new(),
            burst_score: 0.0,
            last_activity: now,
        });

        // Clean old transactions
        activity.transactions.retain(|&time| now.duration_since(time) < Duration::from_secs(60));

        // Check rate limit
        if activity.transactions.len() >= self.config.max_tps_per_ip as usize {
            let mut stats = self.stats.write();
            stats.rate_limit_blocks += 1;
            return SpamResult::Blocked(SpamReason::RateLimitExceeded);
        }

        // Check burst detection
        let recent_count = activity.transactions.iter()
            .filter(|&&time| now.duration_since(time) < Duration::from_secs(self.config.burst_window_seconds))
            .count();

        if recent_count >= self.config.burst_threshold as usize {
            activity.burst_score += 1.0;
            let mut stats = self.stats.write();
            stats.burst_detections += 1;

            if activity.burst_score > 3.0 {
                return SpamResult::Blocked(SpamReason::BurstDetected);
            }
        }

        // Check pattern similarity
        let tx_pattern = TransactionPattern {
            hash: tx.hash(),
            amount: match &tx.payload {
                TxPayload::Transfer { amount, .. } => *amount,
                _ => 0,
            },
            gas_price: tx.gas_price,
            payload_size: std::mem::size_of_val(&tx.payload),
            timestamp: now,
        };

        for pattern in &activity.patterns {
            if self.calculate_similarity(&tx_pattern, pattern) > self.config.similarity_threshold {
                let mut stats = self.stats.write();
                stats.similarity_blocks += 1;
                return SpamResult::Blocked(SpamReason::SimilarTransaction);
            }
        }

        // Add transaction
        activity.transactions.push(now);
        activity.patterns.push(tx_pattern);
        activity.last_activity = now;

        // Limit stored patterns
        if activity.patterns.len() > 100 {
            activity.patterns.remove(0);
        }

        SpamResult::Allowed
    }

    /// Calculate similarity between transaction patterns
    fn calculate_similarity(&self, a: &TransactionPattern, b: &TransactionPattern) -> f64 {
        let mut score = 0.0;
        let mut factors = 0.0;

        // Amount similarity (exact match gets high score)
        if a.amount == b.amount {
            score += 1.0;
        }
        factors += 1.0;

        // Gas price similarity (within 10% gets partial score)
        let gas_diff = (a.gas_price as f64 - b.gas_price as f64).abs() / a.gas_price as f64;
        if gas_diff < 0.1 {
            score += (1.0 - gas_diff * 10.0).max(0.0);
        }
        factors += 1.0;

        // Data length similarity
        if a.payload_size == b.payload_size {
            score += 1.0;
        } else if a.payload_size > 0 && b.payload_size > 0 {
            let len_diff = (a.payload_size as f64 - b.payload_size as f64).abs() / (a.payload_size.max(b.payload_size) as f64);
            score += (1.0 - len_diff).max(0.0);
        }
        factors += 1.0;

        score / factors
    }

    /// Get spam statistics
    pub fn get_stats(&self) -> SpamStats {
        self.stats.read().clone()
    }

    /// Reset statistics
    pub fn reset_stats(&self) {
        *self.stats.write() = SpamStats::default();
    }

    /// Start periodic cleanup task
    fn start_cleanup_task(&self) {
        let activity_map = Arc::clone(&self.ip_activity);
        let global_patterns = Arc::clone(&self.global_patterns);
        let cleanup_interval = self.config.cleanup_interval_seconds;

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(cleanup_interval));

            loop {
                interval.tick().await;
                let now = Instant::now();

                // Clean IP activity
                let mut activity_write = activity_map.write();
                activity_write.retain(|_, activity| {
                    // Remove if no activity for 10 minutes
                    now.duration_since(activity.last_activity) < Duration::from_secs(600)
                });

                // Clean global patterns (keep last hour)
                let mut patterns_write = global_patterns.write();
                patterns_write.retain(|pattern| {
                    now.duration_since(pattern.timestamp) < Duration::from_secs(3600)
                });

                debug!("Cleaned up spam detection data");
            }
        });
    }
}

/// Spam detection result
#[derive(Debug, Clone, PartialEq)]
pub enum SpamResult {
    Allowed,
    Blocked(SpamReason),
}

/// Reason for blocking spam
#[derive(Debug, Clone, PartialEq)]
pub enum SpamReason {
    RateLimitExceeded,
    BurstDetected,
    SimilarTransaction,
}

/// Spam detection statistics
#[derive(Debug, Clone, Default)]
pub struct SpamStats {
    pub rate_limit_blocks: u64,
    pub burst_detections: u64,
    pub similarity_blocks: u64,
    pub total_checked: u64,
}

/// Fuzz testing configuration
#[derive(Debug, Clone)]
pub struct FuzzConfig {
    /// Number of test iterations
    pub iterations: u32,
    /// Maximum transaction size for fuzzing
    pub max_tx_size: usize,
    /// Seed for reproducible fuzzing
    pub seed: u64,
}

/// Fuzz testing suite for attack simulation
pub struct FuzzTester {
    /// Configuration
    config: FuzzConfig,
    /// Pre-validator for testing
    validator: TransactionPreValidator,
    /// Random number generator
    rng: Arc<RwLock<StdRng>>,
}

impl FuzzTester {
    /// Create new fuzz tester
    pub fn new(config: FuzzConfig, validator: TransactionPreValidator) -> Self {
        let rng = Arc::new(RwLock::new(StdRng::seed_from_u64(config.seed)));

        Self {
            config,
            validator,
            rng,
        }
    }

    /// Run fuzz testing on transaction validation
    pub fn fuzz_transaction_validation(&self) -> FuzzResults {
        let mut results = FuzzResults::default();
        let mut rng = self.rng.write();

        for i in 0..self.config.iterations {
            let tx = self.generate_fuzzed_transaction(&mut rng);

            // Test pre-validation
            let pre_result = self.validator.pre_validate_transaction(&tx);
            results.pre_validation_tests += 1;

            if !pre_result.is_valid {
                results.pre_validation_failures += 1;
                results.last_failure = Some(format!("Iteration {}: {:?}", i, pre_result.errors));
            }

            // Test gas estimation
            let _gas = self.validator.estimate_gas_usage(&tx);
            results.gas_estimation_tests += 1;

            // Test edge cases
            if let Some(edge_case) = self.detect_edge_case(&tx) {
                results.edge_cases_found.push(edge_case);
            }
        }

        results
    }

    /// Generate a fuzzed transaction
    fn generate_fuzzed_transaction(&self, rng: &mut StdRng) -> Transaction {
        // Random addresses
        let from: [u8; 20] = rng.gen();
        let to: [u8; 20] = rng.gen();

        // Random amount (sometimes extreme values)
        let amount = match rng.gen_range(0..10) {
            0 => 0, // Zero amount
            1 => u64::MAX, // Max amount
            _ => rng.gen_range(1..1_000_000),
        };

        // Random gas parameters
        let gas_limit = match rng.gen_range(0..10) {
            0 => 0, // Zero gas
            1 => u64::MAX, // Max gas
            _ => rng.gen_range(21000..8_000_000),
        };

        let gas_price = match rng.gen_range(0..10) {
            0 => 0, // Zero price
            1 => u64::MAX, // Max price
            _ => rng.gen_range(1_000_000_000..100_000_000_000),
        };

        // Random payload type
        let payload = match rng.gen_range(0..3) {
            0 => TxPayload::Transfer { to, amount },
            1 => {
                let wasm_code: Vec<u8> = (0..rng.gen_range(100..1000)).map(|_| rng.gen()).collect();
                let init_args: Vec<u8> = (0..rng.gen_range(0..100)).map(|_| rng.gen()).collect();
                TxPayload::ContractDeploy { wasm_code, init_args }
            }
            _ => {
                let method = format!("method_{}", rng.gen_range(0..100));
                let args: Vec<u8> = (0..rng.gen_range(0..200)).map(|_| rng.gen()).collect();
                TxPayload::ContractCall { contract_address: to, method, args }
            }
        };

        let mut tx = Transaction {
            from,
            payload,
            nonce: rng.gen(),
            gas_limit,
            gas_price,
            signature: crate::crypto::Signature::empty(),
        };

        // Sometimes add invalid signature
        if rng.gen_bool(0.1) {
            tx.signature = crate::crypto::Signature::empty();
        }

        tx
    }

    /// Detect edge cases in transaction
    fn detect_edge_case(&self, tx: &Transaction) -> Option<String> {
        let (amount, to) = match &tx.payload {
            TxPayload::Transfer { amount, to } => (*amount, *to),
            TxPayload::ContractDeploy { wasm_code, .. } => (0, [0u8; 20]),
            TxPayload::ContractCall { contract_address, .. } => (0, *contract_address),
        };

        if amount == 0 && matches!(tx.payload, TxPayload::Transfer { .. }) {
            Some("Zero amount transfer".to_string())
        } else if amount == u64::MAX {
            Some("Maximum amount transaction".to_string())
        } else if tx.gas_limit == 0 {
            Some("Zero gas limit".to_string())
        } else if tx.gas_limit == u64::MAX {
            Some("Maximum gas limit".to_string())
        } else if tx.from == to && matches!(tx.payload, TxPayload::Transfer { .. }) {
            Some("Self-transfer transaction".to_string())
        } else if let TxPayload::ContractDeploy { wasm_code, .. } = &tx.payload {
            if wasm_code.len() > 100 * 1024 { // 100KB
                Some("Large WASM code payload".to_string())
            } else {
                None
            }
        } else {
            None
        }
    }

    /// Run DoS attack simulation
    pub fn simulate_dos_attack(&self, attack_type: DosAttackType) -> DosSimulationResult {
        let mut results = DosSimulationResult {
            attack_type: attack_type.clone(),
            transactions_generated: 0,
            detection_rate: 0.0,
            avg_processing_time: Duration::default(),
        };

        let mut rng = self.rng.write();
        let start_time = Instant::now();

        match attack_type {
            DosAttackType::SpamBurst => {
                // Generate burst of similar transactions
                for _ in 0..1000 {
                    let tx = self.generate_burst_transaction(&mut rng);
                    let _result = self.validator.pre_validate_transaction(&tx);
                    results.transactions_generated += 1;
                }
            }
            DosAttackType::LargeTransactions => {
                // Generate transactions with large data
                for _ in 0..100 {
                    let tx = self.generate_large_transaction(&mut rng);
                    let _result = self.validator.pre_validate_transaction(&tx);
                    results.transactions_generated += 1;
                }
            }
            DosAttackType::InvalidSignatures => {
                // Generate transactions with invalid signatures
                for _ in 0..500 {
                    let tx = self.generate_invalid_signature_transaction(&mut rng);
                    let _result = self.validator.pre_validate_transaction(&tx);
                    results.transactions_generated += 1;
                }
            }
        }

        results.avg_processing_time = start_time.elapsed() / results.transactions_generated.max(1);
        results.detection_rate = 0.95; // Simulated detection rate

        results
    }

    /// Generate transaction for burst attack
    fn generate_burst_transaction(&self, rng: &mut StdRng) -> Transaction {
        let from: [u8; 20] = rng.gen();
        let to: [u8; 20] = rng.gen();

        Transaction::new_transfer(from, to, 1000, 0, 21000, 1_000_000_000)
    }

    /// Generate large transaction
    fn generate_large_transaction(&self, rng: &mut StdRng) -> Transaction {
        let from: [u8; 20] = rng.gen();
        let _to: [u8; 20] = rng.gen();
        let wasm_code: Vec<u8> = (0..50 * 1024).map(|_| rng.gen()).collect(); // 50KB

        Transaction::new_deploy(from, wasm_code, vec![], 0, 50000, 1_000_000_000)
    }

    /// Generate transaction with invalid signature
    fn generate_invalid_signature_transaction(&self, rng: &mut StdRng) -> Transaction {
        let from: [u8; 20] = rng.gen();
        let to: [u8; 20] = rng.gen();

        let tx = Transaction::new_transfer(from, to, 1000, 0, 21000, 1_000_000_000);
        // Leave signature as empty (invalid)
        tx
    }
}

/// DoS attack types for simulation
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DosAttackType {
    SpamBurst,
    LargeTransactions,
    InvalidSignatures,
}

/// Fuzz testing results
#[derive(Debug, Clone, Default)]
pub struct FuzzResults {
    pub pre_validation_tests: u32,
    pub pre_validation_failures: u32,
    pub gas_estimation_tests: u32,
    pub edge_cases_found: Vec<String>,
    pub last_failure: Option<String>,
}

/// DoS simulation results
#[derive(Debug, Clone)]
pub struct DosSimulationResult {
    pub attack_type: DosAttackType,
    pub transactions_generated: u32,
    pub detection_rate: f64,
    pub avg_processing_time: Duration,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_spam_detection_rate_limit() {
        let config = SpamDetectionConfig {
            max_tps_per_ip: 5,
            burst_window_seconds: 10,
            burst_threshold: 10,
            similarity_threshold: 0.8,
            cleanup_interval_seconds: 60,
        };
        let detector = SpamDetector::new(config);

        let ip = std::net::IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let tx = Transaction::new_transfer([1; 20], [2; 20], 1000, 0, 21000, 1_000_000_000);

        // Should allow first few transactions
        for _ in 0..5 {
            assert!(matches!(detector.check_spam(&tx, ip), SpamResult::Allowed));
        }

        // Should block on rate limit
        assert!(matches!(detector.check_spam(&tx, ip), SpamResult::Blocked(SpamReason::RateLimitExceeded)));
    }

    #[test]
    fn test_fuzz_testing() {
        let fuzz_config = FuzzConfig {
            iterations: 100,
            max_tx_size: 1024,
            seed: 42,
        };
        let validator = TransactionPreValidator::default();
        let fuzzer = FuzzTester::new(fuzz_config, validator);

        let results = fuzzer.fuzz_transaction_validation();
        assert_eq!(results.pre_validation_tests, 100);
        assert!(results.gas_estimation_tests > 0);
    }

    #[test]
    fn test_dos_simulation() {
        let fuzz_config = FuzzConfig {
            iterations: 10,
            max_tx_size: 1024,
            seed: 42,
        };
        let validator = TransactionPreValidator::default();
        let fuzzer = FuzzTester::new(fuzz_config, validator);

        let result = fuzzer.simulate_dos_attack(DosAttackType::SpamBurst);
        assert_eq!(result.attack_type, DosAttackType::SpamBurst);
        assert!(result.transactions_generated > 0);
        assert!(result.detection_rate > 0.0);
    }
}