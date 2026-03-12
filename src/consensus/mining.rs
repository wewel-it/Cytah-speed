use crate::core::BlockHash;

/// Calculate mining reward based on the number of transactions.
///
/// - `tx_count` is either the number of transactions in a candidate block or an
///   average over a recent interval. A small count yields a high reward (up to
///   500 CTS). As load increases the reward decays toward a floor of 0.1 CTS.
///
/// The implementation uses an exponential decay curve with a fixed rate; the
/// constants can be tuned later.
pub fn calculate_reward(tx_count: usize) -> f64 {
    const BASE: f64 = 500.0;
    const MIN_REWARD: f64 = 0.1;
    const DECAY_RATE: f64 = 0.005; // smaller rate = slower decay

    let decay = (-DECAY_RATE * (tx_count as f64)).exp();
    let reward = BASE * decay;
    if reward < MIN_REWARD { MIN_REWARD } else if reward > BASE { BASE } else { reward }
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
    fn reward_behaviour() {
        assert_eq!(calculate_reward(0), 500.0);
        assert_eq!(calculate_reward(1), calculate_reward(1));
        assert!(calculate_reward(1000) < calculate_reward(10));
        assert!(calculate_reward(1000000) > 0.099);
    }

    #[test]
    fn difficulty_checks() {
        let mut hash:[u8;32] = [0;32];
        assert!(meets_difficulty(&hash, 0));
        // first byte nonzero
        hash[0] = 1;
        assert!(!meets_difficulty(&hash,8));
        // set bits to satisfy 7 bits
        hash[0] = 0b0000_0000;
        assert!(meets_difficulty(&hash,7));
    }
}
