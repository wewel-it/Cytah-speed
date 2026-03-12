use crate::core::Block;
use crate::dag::blockdag::BlockDAG;

/// Difficulty adjustment algorithm (DAA) helpers.
///
/// The algorithm uses a moving-average window of the most recent N blocks
/// (currently 263).  It computes the actual elapsed time between the first
/// and last block in the window, clamping any individual timestamp gaps to
/// a reasonable range to resist manipulation.  The new difficulty is then
/// scaled linearly by the ratio of actual vs expected time, with a hard
/// cap on how quickly difficulty can change.
///
/// The difficulty value is expressed as "leading-zero bits"; we perform the
/// adjustment on the numeric value and cast back to `u32`.  This is
/// not cryptographically precise but is sufficient for a proof-of-concept.

const DAA_WINDOW: usize = 263;
const TARGET_BLOCK_TIME: u64 = 1; // 1 second per block
const MIN_ADJUST_RATIO: f64 = 0.25; // no more than 4x down
const MAX_ADJUST_RATIO: f64 = 4.0;  // no more than 4x up
const MAX_GAP: u64 = TARGET_BLOCK_TIME * 4; // clamp individual gap to 4s

/// Calculate the next difficulty given a slice of chronologically ordered blocks.
///
/// If fewer than `DAA_WINDOW` blocks are supplied the previous difficulty is
/// returned unchanged.
pub fn calculate_next_difficulty(blocks: &[Block]) -> u32 {
    if blocks.len() < DAA_WINDOW {
        return blocks.last().map(|b| b.header.difficulty).unwrap_or(1);
    }

    // consider only the last window
    let recent = &blocks[blocks.len() - DAA_WINDOW..];

    // compute actual timespan with clamped deltas
    let mut actual: u64 = 0;
    let mut prev_ts = recent[0].header.timestamp;
    for blk in &recent[1..] {
        let ts = blk.header.timestamp;
        let delta = if ts > prev_ts { ts - prev_ts } else { 0 };
        actual = actual.saturating_add(delta.min(MAX_GAP));
        prev_ts = ts;
    }

    let expected = (DAA_WINDOW as u64) * TARGET_BLOCK_TIME;
    let mut ratio = (actual as f64) / (expected as f64);
    if ratio < MIN_ADJUST_RATIO {
        ratio = MIN_ADJUST_RATIO;
    } else if ratio > MAX_ADJUST_RATIO {
        ratio = MAX_ADJUST_RATIO;
    }

    let prev_diff = recent.last().unwrap().header.difficulty as f64;
    let mut next = (prev_diff * ratio).round();
    if next < 1.0 {
        next = 1.0;
    }
    if next > (u32::MAX as f64) {
        next = u32::MAX as f64;
    }
    next as u32
}

/// Convenience wrapper: compute next difficulty directly from a BlockDAG.
///
/// This pulls the most recent blocks from the DAG's topological order and
/// invokes `calculate_next_difficulty`.
pub fn next_difficulty(dag: &BlockDAG) -> u32 {
    let order = dag.get_topological_order();
    let mut recent_blocks: Vec<Block> = Vec::new();
    for hash in order.iter().rev().take(DAA_WINDOW) {
        if let Some(b) = dag.get_block(hash) {
            recent_blocks.push(b);
        }
    }
    recent_blocks.reverse();
    calculate_next_difficulty(&recent_blocks)
}
