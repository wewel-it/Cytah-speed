use rocksdb::{DB, Options};
use std::sync::Arc;
use crate::state::state_manager::StateManager;

/// Simple helper that keeps a sliding (rolling) window of the most recent
/// blocks and aggressively deletes older data from RocksDB.  This is only a
/// demonstration of the pruning logic; the rest of the blockchain currently
/// keeps blocks in memory in `BlockStore`.
///
/// The policy implemented here matches the requirements:
/// * once `current_height` reaches 200_000, the first 100 000 blocks are
///   removed in a single batch; subsequently each new block causes the
///   earliest block outside the window to be deleted, so at all times the
///   database contains at most `window_size` blocks.
/// * before any deletion, a snapshot of the provided `StateManager` is
///   written to the database under the key `latest_state_root` so that account
///   balances are preserved even though historic block data is removed.
#[derive(Clone)]
pub struct RollingWindowPruner {
    db: Arc<DB>,
    window_size: u64,
    last_pruned_height: u64,
}

impl RollingWindowPruner {
    /// Open or create a RocksDB instance at `path` and prepare the pruner.
    pub fn new(path: &str, window_size: u64) -> Self {
        let mut opts = Options::default();
        opts.create_if_missing(true);

        // make sure the necessary column families exist; "blocks" holds
        // serialized blocks, "transactions" holds indexed tx data.
        let cfs = vec!["default", "blocks", "transactions"];
        let db = DB::open_cf(&opts, path, &cfs).expect("failed to open pruning db");

        RollingWindowPruner {
            db: Arc::new(db),
            window_size,
            last_pruned_height: 0,
        }
    }

    /// Should be called whenever the node processes a new block at
    /// `current_height` (1‑indexed).  The `state` argument is used to take an
    /// up‑to‑date snapshot before any data is removed.
    pub fn maybe_prune(&mut self, current_height: u64, state: &mut StateManager) {
        // nothing to do until we cross the threshold
        if current_height < 200_000 {
            return;
        }

        // store latest state root (balance snapshot) before any deletion
        let root = state.get_state_root();
        let _ = self.db.put(b"latest_state_root", &root);

        // calculate highest height that should be pruned
        let prune_up_to = if current_height > self.window_size {
            current_height - self.window_size
        } else {
            0
        };

        if prune_up_to <= self.last_pruned_height {
            // already pruned this far
            return;
        }

        // we store block entries keyed by a height prefix, so we can delete a
        // contiguous range efficiently.  The format is "block_<height>" with
        // zero padding to keep lexicographic order.
        let start_key = format!("block_{:010}", self.last_pruned_height + 1);
        let end_key = format!("block_{:010}", prune_up_to + 1); // end is exclusive

        // delete blocks in both the "blocks" column family and the
        // "transactions" family (if txs are stored per-height as well).
        if let Some(cf) = self.db.cf_handle("blocks") {
            let _ = self.db.delete_range_cf(cf, start_key.as_bytes(), end_key.as_bytes());
        }
        if let Some(cf) = self.db.cf_handle("transactions") {
            let _ = self.db.delete_range_cf(cf, start_key.as_bytes(), end_key.as_bytes());
        }

        self.last_pruned_height = prune_up_to;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::state_manager::StateManager;

    #[test]
    fn test_pruner_basic() {
        let mut pruner = RollingWindowPruner::new("./test_pruner.db", 100_000);
        let mut state = StateManager::new();

        // before threshold nothing happens
        pruner.maybe_prune(199_999, &mut state);
        assert_eq!(pruner.last_pruned_height, 0);

        pruner.maybe_prune(200_000, &mut state);
        assert_eq!(pruner.last_pruned_height, 100_000);

        // a later block should prune one more height
        pruner.maybe_prune(200_001, &mut state);
        assert_eq!(pruner.last_pruned_height, 100_001);
    }
}
