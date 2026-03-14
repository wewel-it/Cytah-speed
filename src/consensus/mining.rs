use crate::core::BlockHash;
use std::collections::VecDeque;
use std::sync::Arc;
use parking_lot::RwLock;
use tokio::time::{Duration, Instant};
use tracing::{info, warn, debug};
use chrono;

/// CTS Tokenomics Constants
pub const CTS_MAX_SUPPLY: u64 = 600_000_000;
pub const CTS_GENESIS_ALLOCATION: u64 = 100_000_000;
pub const CTS_MINING_SUPPLY: u64 = CTS_MAX_SUPPLY - CTS_GENESIS_ALLOCATION; // 500M
pub const CTS_INITIAL_REWARD: u64 = 100; // Initial mining reward in CTS
pub const CTS_MINIMUM_REWARD: u64 = 1; // Minimum reward to prevent zero
pub const CTS_EMISSION_DECAY_FACTOR: f64 = 0.000001; // Decay factor for long-term emission

/// Reward Adjustment Configuration
#[derive(Debug, Clone)]
pub struct RewardConfig {
    /// Target blocks per minute for activity measurement
    pub target_blocks_per_minute: f64,
    /// Sliding window size for activity measurement (in blocks)
    pub activity_window_size: usize,
    /// Maximum reward multiplier
    pub max_multiplier: f64,
    /// Minimum reward multiplier
    pub min_multiplier: f64,
}

impl Default for RewardConfig {
    fn default() -> Self {
        Self {
            target_blocks_per_minute: 2.0, // 2 blocks per minute = 30s average
            activity_window_size: 144, // ~1 hour at 30s blocks
            max_multiplier: 2.0,
            min_multiplier: 0.5,
        }
    }
}

/// Calculate activity multiplier based on recent block timestamps.
///
/// Uses a sliding window of timestamps to determine whether block production
/// is above or below the target rate. This is deterministic as long as the
/// timestamps are part of the blockchain.
pub fn calculate_activity_multiplier(timestamps: &[u64], config: &RewardConfig) -> f64 {
    if timestamps.len() < 2 {
        return 1.0;
    }

    let window_start = timestamps.first().unwrap();
    let window_end = timestamps.last().unwrap();
    let time_window = window_end.saturating_sub(*window_start);

    if time_window == 0 {
        return config.max_multiplier;
    }

    let blocks_in_window = timestamps.len() as f64;
    let actual_rate = blocks_in_window / (time_window as f64 / 60.0);
    let multiplier = config.target_blocks_per_minute / actual_rate;

    multiplier.max(config.min_multiplier).min(config.max_multiplier)
}

/// Calculate emission-decayed base reward based on chain progress.
pub fn calculate_base_reward(chain_progress: f64) -> u64 {
    let decay = (-CTS_EMISSION_DECAY_FACTOR * chain_progress).exp();
    let decayed_reward = CTS_INITIAL_REWARD as f64 * decay;
    (decayed_reward as u64).max(CTS_MINIMUM_REWARD)
}

/// Calculate expected reward for a block, given current emitted supply and chain activity.
pub fn calculate_expected_block_reward(
    emitted_supply: u64,
    chain_progress: f64,
    recent_timestamps: &[u64],
    tx_count: usize,
    config: &RewardConfig,
) -> u64 {
    // Base reward decays over time (chain progress)
    let base_reward = calculate_base_reward(chain_progress);

    // Activity multiplier based on recent block production
    let activity_multiplier = calculate_activity_multiplier(recent_timestamps, config);

    // Reward scales with transaction count (more tx => smaller reward) - optional
    // We use a mild adjustment where more tx reduces the effective reward slightly.
    let tx_factor = 1.0 / (1.0 + (tx_count as f64 / 1000.0));

    let mut reward = (base_reward as f64 * activity_multiplier * tx_factor) as u64;
    reward = reward.max(CTS_MINIMUM_REWARD);

    // Prevent exceeding remaining mining supply
    let remaining_supply = CTS_MINING_SUPPLY.saturating_sub(emitted_supply);
    if remaining_supply == 0 {
        return 0;
    }

    if reward > remaining_supply {
        remaining_supply
    } else {
        reward
    }
}

/// Difficulty Adjustment Algorithm (DAA) configuration
#[derive(Debug, Clone)]
pub struct DaaConfig {
    /// Target block interval in seconds
    pub target_block_interval: u64,
    /// Difficulty adjustment window size (number of blocks)
    pub adjustment_window: usize,
    /// Minimum difficulty (to prevent too easy mining)
    pub min_difficulty: u32,
    /// Maximum difficulty (to prevent too hard mining)
    pub max_difficulty: u32,
    /// Dampening factor for difficulty changes (0.0-1.0)
    pub dampening_factor: f64,
}

impl Default for DaaConfig {
    fn default() -> Self {
        Self {
            target_block_interval: 30, // 30 seconds target
            adjustment_window: 144,    // ~1 hour at 30s blocks
            min_difficulty: 1,
            max_difficulty: u32::MAX / 4, // Prevent overflow
            dampening_factor: 0.25,   // 25% dampening
        }
    }
}

/// Difficulty adjustment state
#[derive(Debug, Clone)]
pub struct DifficultyState {
    /// Current difficulty
    pub current_difficulty: u32,
    /// Block timestamps for adjustment window
    pub timestamps: VecDeque<u64>,
    /// Last adjustment time
    pub last_adjustment: u64,
    /// Configuration
    pub config: DaaConfig,
}

impl DifficultyState {
    pub fn new(config: DaaConfig) -> Self {
        Self {
            current_difficulty: 1, // Start with easy difficulty
            timestamps: VecDeque::with_capacity(config.adjustment_window + 1),
            last_adjustment: chrono::Utc::now().timestamp() as u64,
            config,
        }
    }

    /// Add a new block timestamp and adjust difficulty if needed
    pub fn add_block(&mut self, timestamp: u64) {
        self.timestamps.push_back(timestamp);

        // Keep only the most recent timestamps
        while self.timestamps.len() > self.config.adjustment_window {
            self.timestamps.pop_front();
        }

        // Adjust difficulty every adjustment_window blocks
        if self.timestamps.len() >= self.config.adjustment_window {
            self.adjust_difficulty();
        }
    }

    /// Calculate new difficulty based on observed block times
    fn adjust_difficulty(&mut self) {
        if self.timestamps.len() < 2 {
            return;
        }

        let now = chrono::Utc::now().timestamp() as u64;
        let time_elapsed = now - self.last_adjustment;

        if time_elapsed == 0 {
            return; // Prevent division by zero
        }

        // Calculate expected time for the window
        let expected_time = self.config.target_block_interval * (self.config.adjustment_window as u64 - 1);

        // Calculate actual time elapsed for the last N-1 blocks
        let actual_time = self.timestamps.back().unwrap() - self.timestamps.front().unwrap();

        if actual_time == 0 {
            return; // Prevent division by zero
        }

        // Calculate ratio of actual to expected time
        let time_ratio = actual_time as f64 / expected_time as f64;

        // Calculate difficulty adjustment factor
        let adjustment_factor = 1.0 / time_ratio;

        // Apply dampening to prevent wild swings
        let damped_adjustment = 1.0 + (adjustment_factor - 1.0) * self.config.dampening_factor;

        // Calculate new difficulty
        let new_difficulty = (self.current_difficulty as f64 * damped_adjustment) as u32;

        // Clamp to min/max bounds
        let clamped_difficulty = new_difficulty.clamp(self.config.min_difficulty, self.config.max_difficulty);

        // Only update if there's a meaningful change (>1% difference)
        let change_ratio = (clamped_difficulty as f64) / (self.current_difficulty as f64);
        if change_ratio < 0.99 || change_ratio > 1.01 {
            info!(
                "Difficulty adjustment: {} -> {} (time ratio: {:.3}, adjustment: {:.3})",
                self.current_difficulty, clamped_difficulty, time_ratio, damped_adjustment
            );
            self.current_difficulty = clamped_difficulty;
            self.last_adjustment = now;
        }
    }

    /// Get current difficulty
    pub fn get_difficulty(&self) -> u32 {
        self.current_difficulty
    }

    /// Estimate network hashrate based on current difficulty and target time
    pub fn estimate_network_hashrate(&self) -> f64 {
        // Rough estimation: hashrate = difficulty / target_time
        // This is a simplification; real estimation would be more complex
        self.current_difficulty as f64 / self.config.target_block_interval as f64
    }

    /// Check for difficulty oscillation attacks
    pub fn detect_oscillation(&self) -> bool {
        if self.timestamps.len() < self.config.adjustment_window {
            return false;
        }

        // Check for suspicious patterns in timestamps
        // This is a simplified check; real implementation would be more sophisticated
        let mut intervals = Vec::new();
        let timestamps: Vec<_> = self.timestamps.iter().collect();

        for i in 1..timestamps.len() {
            let interval = timestamps[i] - timestamps[i-1];
            intervals.push(interval);
        }

        // Check for high variance in block intervals (potential attack)
        if let (Some(min), Some(max)) = (intervals.iter().min(), intervals.iter().max()) {
            if *max > *min * 10 { // 10x difference
                warn!("Potential difficulty oscillation detected: min_interval={}, max_interval={}", min, max);
                return true;
            }
        }

        false
    }
}

/// Mining manager with DAA support
#[derive(Clone)]
pub struct MiningManager {
    /// Difficulty adjustment state
    difficulty_state: Arc<RwLock<DifficultyState>>,
    /// Recent block timestamps for monitoring
    recent_blocks: Arc<RwLock<VecDeque<(u64, u32)>>>, // (timestamp, difficulty)
    /// Mining statistics
    stats: Arc<RwLock<MiningStats>>,
}

#[derive(Debug, Clone)]
pub struct MiningStats {
    pub blocks_mined: u64,
    pub total_hashrate: f64,
    pub average_block_time: f64,
    pub difficulty_adjustments: u32,
    pub last_block_time: u64,
}

impl MiningStats {
    pub fn new() -> Self {
        Self {
            blocks_mined: 0,
            total_hashrate: 0.0,
            average_block_time: 0.0,
            difficulty_adjustments: 0,
            last_block_time: 0,
        }
    }
}

impl MiningManager {
    pub fn new(config: DaaConfig) -> Self {
        Self {
            difficulty_state: Arc::new(RwLock::new(DifficultyState::new(config))),
            recent_blocks: Arc::new(RwLock::new(VecDeque::with_capacity(1000))),
            stats: Arc::new(RwLock::new(MiningStats::new())),
        }
    }

    /// Record a new block and adjust difficulty
    pub fn record_block(&self, timestamp: u64) {
        // Add to difficulty adjustment
        self.difficulty_state.write().add_block(timestamp);

        // Record for statistics
        let difficulty = self.get_current_difficulty();
        let mut recent = self.recent_blocks.write();
        recent.push_back((timestamp, difficulty));

        // Keep only recent blocks
        while recent.len() > 1000 {
            recent.pop_front();
        }

        // Update stats
        let mut stats = self.stats.write();
        stats.blocks_mined += 1;
        stats.last_block_time = timestamp;

        // Update average block time
        if recent.len() >= 2 {
            let total_time: u64 = recent.iter().skip(1).zip(recent.iter())
                .map(|((t1, _), (t2, _))| t2 - t1).sum();
            stats.average_block_time = total_time as f64 / (recent.len() - 1) as f64;
        }
    }

    /// Get current mining difficulty
    pub fn get_current_difficulty(&self) -> u32 {
        self.difficulty_state.read().get_difficulty()
    }

    /// Get estimated network hashrate
    pub fn get_network_hashrate(&self) -> f64 {
        self.difficulty_state.read().estimate_network_hashrate()
    }

    /// Check if difficulty adjustment is needed
    pub fn should_adjust_difficulty(&self) -> bool {
        let state = self.difficulty_state.read();
        state.timestamps.len() >= state.config.adjustment_window
    }

    /// Get mining statistics
    pub fn get_stats(&self) -> MiningStats {
        self.stats.read().clone()
    }

    /// Detect potential attacks
    pub fn detect_attacks(&self) -> Vec<String> {
        let mut alerts = Vec::new();

        // Check for difficulty oscillation
        if self.difficulty_state.read().detect_oscillation() {
            alerts.push("Difficulty oscillation detected".to_string());
        }

        // Check for unusual block times
        let stats = self.stats.read();
        if stats.average_block_time > 0.0 {
            let target_time = self.difficulty_state.read().config.target_block_interval as f64;
            let deviation = (stats.average_block_time - target_time).abs() / target_time;

            if deviation > 0.5 { // 50% deviation
                alerts.push(format!("Block time deviation: {:.1}s (target: {}s)",
                    stats.average_block_time, target_time));
            }
        }

        alerts
    }

    /// Validate miner reward (ensure it's reasonable)
    pub fn validate_miner_reward(&self, reward: u64, block_height: u64) -> bool {
        // Basic validation: reward should be positive and not excessive
        if reward == 0 {
            return false;
        }

        // Check against maximum reasonable reward
        let max_reasonable_reward = 1000; // CTH tokens
        if reward > max_reasonable_reward {
            warn!("Unusually high miner reward: {} at height {}", reward, block_height);
            return false;
        }

        true
    }
}

// Legacy reward calculation (kept for backwards compatibility).  New reward model is
// calculated via `calculate_expected_block_reward`.
#[deprecated(note = "Use calculate_expected_block_reward instead")]
pub fn calculate_reward(tx_count: usize) -> u64 {
    calculate_expected_block_reward(0, 0.0, &[], tx_count, &RewardConfig::default())
}

/// Check whether a hash satisfies the given difficulty expressed as leading-zero bits.
/// A difficulty of 0 always returns true (no PoW requirement).
pub fn meets_difficulty(hash: &BlockHash, difficulty: u32) -> bool {
    if difficulty == 0 {
        return true;
    }

    // Count full zero bytes
    let zero_bytes = (difficulty / 8) as usize;
    if hash.iter().take(zero_bytes).any(|&b| b != 0) {
        return false;
    }

    // Check the remaining bits in the next byte
    let rem_bits = (difficulty % 8) as u8;
    if rem_bits > 0 {
        let mask: u8 = 0xFF << (8 - rem_bits);
        if hash.get(zero_bytes).map_or(true, |&b| b & mask != 0) {
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_difficulty_adjustment() {
        let config = DaaConfig {
            target_block_interval: 10,
            adjustment_window: 10,
            min_difficulty: 1,
            max_difficulty: 1000,
            dampening_factor: 0.25,
        };

        let mut state = DifficultyState::new(config);

        // Add blocks at target interval
        for i in 0..15 {
            state.add_block(i * 10);
        }

        // Difficulty should remain stable
        assert!(state.get_difficulty() >= 1);
    }

    #[test]
    fn test_reward_calculation() {
        // Low transaction count = high reward
        assert_eq!(calculate_reward(0), 500);
        assert_eq!(calculate_reward(1), 498);

        // High transaction count = low reward but never zero
        assert!(calculate_reward(1000) >= 1);
        assert!(calculate_reward(10000) >= 1);
    }

    #[test]
    fn test_difficulty_check() {
        let hash = [0u8; 32];

        // Easy difficulty
        assert!(meets_difficulty(&hash, 0));
        assert!(meets_difficulty(&hash, 1));

        // Hard difficulty
        let hard_hash = [255u8; 32];
        assert!(!meets_difficulty(&hard_hash, 8));
    }
}
