pub mod dag_traversal;
pub mod blue_set;
pub mod ghostdag;
pub mod mining;
pub mod daa;
pub mod fee_market;

pub use dag_traversal::DAGTraversal;
pub use blue_set::{BlueSet, BlueSetStats};
pub use ghostdag::{GHOSTDAGEngine, GHOSTDAGStats};

// expose reward + PoW helpers
pub use mining::{calculate_reward, meets_difficulty};
pub use daa::{calculate_next_difficulty, next_difficulty};
pub use fee_market::next_base_fee;
