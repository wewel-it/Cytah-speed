use crate::dag::blockdag::BlockDAG;

/// Simple fee market logic inspired by EIP-1559.
///
/// Base fee is adjusted each block based on whether the previous block was
/// "full" (>50% of target capacity) or "empty".  We store the base fee in
/// the block header and compute the next value by looking at the most recent
/// mined block.  This is intentionally rudimentary for the prototype.

const BASE_FEE_ADJUST_UP_PERCENT: u64 = 10;   // 10% increase when full
const BASE_FEE_ADJUST_DOWN_PERCENT: u64 = 10; // 10% decrease when not full
const BASE_FEE_MIN: u64 = 1;

/// Compute the next base fee given the current DAG state.
///
/// The algorithm simply examines the tip blocks and uses the last block's
/// base_fee plus an adjustment depending on fullness.  Fullness is estimated
/// by comparing transaction count to a nominal target which is inferred from
/// the reserved maximum transactions per block (this is not stored on-chain,
/// so we cheat and assume 1000 as a default; callers may override by tracking
/// their own target separately).
pub fn next_base_fee(dag: &BlockDAG) -> u64 {
    // if no blocks exist, start with minimum base fee
    let order = dag.get_topological_order();
    if let Some(last_hash) = order.last() {
        if let Some(last_block) = dag.get_block(last_hash) {
            let prev_base = last_block.header.base_fee.max(BASE_FEE_MIN);
            // estimate fullness
            let tx_count = last_block.transactions.len() as u64;
            // nominal target (must be coordinated with producer configuration)
            let target = 1000;
            let new_fee = if tx_count * 2 > target {
                // more than 50% full -> increase
                prev_base + prev_base * BASE_FEE_ADJUST_UP_PERCENT / 100
            } else {
                // less than 50% -> decrease
                prev_base.saturating_sub(prev_base * BASE_FEE_ADJUST_DOWN_PERCENT / 100)
            };
            return new_fee.max(BASE_FEE_MIN);
        }
    }
    BASE_FEE_MIN
}
