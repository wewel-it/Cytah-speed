use crate::core::transaction::Address;
use crate::state::state_tree::{SparseMerkleTree, Account};

// bring serde traits into scope for serialization

pub const GENESIS_WALLET: Address = [1u8; 20];
pub const EMISSION_STATE_ACCOUNT: Address = [255u8; 20];

pub type Hash = [u8; 32];

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StateManager {
    pub state_tree: SparseMerkleTree,
    pub current_state_root: Hash,
}

impl StateManager {
    pub fn new() -> Self {
        let mut state_tree = SparseMerkleTree::new();

        // Initialize the genesis wallet and emission tracker in state
        // These accounts are created once and remain persistent.
        state_tree.update_account(GENESIS_WALLET, Account::new(0, 0));
        state_tree.update_account(EMISSION_STATE_ACCOUNT, Account::new(0, 0));

        let current_state_root = state_tree.calculate_root();
        Self {
            state_tree,
            current_state_root,
        }
    }

    /// Initialize genesis allocation and emission tracking if not already set.
    ///
    /// This is safe to call repeatedly; it does not overwrite existing balances.
    pub fn initialize_tokenomics(&mut self) {
        // Genesis allocation: 100M CTS to the genesis wallet
        if self.state_tree.get_account(&GENESIS_WALLET).is_none() {
            self.state_tree.update_account(GENESIS_WALLET, Account::new(0, 0));
        }
        let genesis_account = self.state_tree.get_account(&GENESIS_WALLET).cloned().unwrap();
        if genesis_account.balance == 0 {
            self.state_tree.update_account(GENESIS_WALLET, Account::new(100_000_000, genesis_account.nonce));
        }

        // Ensure emission tracker exists
        if self.state_tree.get_account(&EMISSION_STATE_ACCOUNT).is_none() {
            self.state_tree.update_account(EMISSION_STATE_ACCOUNT, Account::new(0, 0));
        }

        self.current_state_root = self.state_tree.calculate_root();
    }

    /// Get the total emitted supply (mining rewards) tracked in state.
    pub fn get_emitted_supply(&self) -> u64 {
        self.state_tree
            .get_account(&EMISSION_STATE_ACCOUNT)
            .map(|acc| acc.balance)
            .unwrap_or(0)
    }

    /// Add to the emitted supply counter.
    pub fn add_emitted_supply(&mut self, amount: u64) {
        if amount == 0 {
            return;
        }

        // Ensure we never exceed the mining supply cap.
        let mut acc = self.state_tree.get_account(&EMISSION_STATE_ACCOUNT)
            .cloned()
            .unwrap_or(Account::new(0, 0));
        let remaining = crate::consensus::mining::CTS_MINING_SUPPLY.saturating_sub(acc.balance);
        if remaining == 0 {
            return;
        }

        let to_add = amount.min(remaining);
        acc.balance = acc.balance.saturating_add(to_add);
        self.state_tree.update_account(EMISSION_STATE_ACCOUNT, acc);
        self.current_state_root = self.state_tree.calculate_root();
    }

    /// Get total supply of tokens in circulation
    pub fn get_total_supply(&self) -> u64 {
        let mut total = 0u64;
        self.state_tree.iter_accounts(|addr, account| {
            if *addr == EMISSION_STATE_ACCOUNT {
                // This account is used to track emitted supply and is not a real balance holder
                return;
            }
            total = total.saturating_add(account.balance);
        });
        total
    }


    /// Apply a transaction and charge its associated gas fee.  For simple
    /// transfers we assume the full gas limit is consumed; more accurate
    /// accounting is handled by `ContractExecutor` when executing contracts.
    pub fn apply_transaction(&mut self, tx: &crate::core::transaction::Transaction) -> Result<(), String> {
        use crate::core::transaction::TxPayload;

        // fee = gas_limit * gas_price (overflow shouldn't realistically happen)
        let fee = tx.gas_limit.saturating_mul(tx.gas_price);
        match &tx.payload {
            TxPayload::Transfer { to, amount } => {
                self.apply_transfer(tx.from, *to, *amount, tx.nonce, fee)
            }
            _ => {
                // Contract transactions are handled by ContractExecutor.  Note that
                // ContractExecutor is responsible for deducting the proper fee
                // after determining actual gas used.
                Ok(())
            }
        }
    }

    fn apply_transfer(&mut self, from: crate::core::transaction::Address, to: crate::core::transaction::Address, amount: u64, nonce: u64, fee: u64) -> Result<(), String> {
        // Validate transaction
        if amount == 0 {
            return Err("Amount must be greater than 0".to_string());
        }

        // Get sender account
        let sender_account = self.state_tree.get_account(&from)
            .cloned()
            .unwrap_or(Account::new(0, 0));

        // Check nonce
        if sender_account.nonce != nonce {
            return Err(format!("Invalid nonce: expected {}, got {}", sender_account.nonce, nonce));
        }

        // Check balance (must cover amount + fee)
        let total_cost = amount.saturating_add(fee);
        if sender_account.balance < total_cost {
            return Err("Insufficient balance for amount and fee".to_string());
        }

        // Update sender: subtract both amount and fee
        let new_sender_balance = sender_account.balance - total_cost;
        let new_sender_nonce = sender_account.nonce + 1;
        let new_sender_account = Account::new(new_sender_balance, new_sender_nonce);
        self.state_tree.update_account(from, new_sender_account);

        // Get receiver account
        let receiver_account = self.state_tree.get_account(&to)
            .cloned()
            .unwrap_or(Account::new(0, 0));

        // Update receiver only by amount
        let new_receiver_balance = receiver_account.balance + amount;
        let new_receiver_account = Account::new(new_receiver_balance, receiver_account.nonce);
        self.state_tree.update_account(to, new_receiver_account);

        // Update state root
        self.current_state_root = self.state_tree.calculate_root();

        Ok(())
    }

    /// Called after a block has been executed; this method currently does
    /// nothing because individual transactions already mutate state.  We'll
    /// keep it for compatibility but it may also credit miner rewards and fees
    /// in the future.
    pub fn apply_block(&mut self, _transactions: &[crate::core::transaction::Transaction]) -> Result<(), String> {
        Ok(())
    }

    pub fn get_state_root(&self) -> Hash {
        self.current_state_root
    }

    /// Return the balance of an account (0 if not present)
    pub fn get_balance(&self, addr: crate::core::transaction::Address) -> u64 {
        self.state_tree
            .get_account(&addr)
            .cloned()
            .map(|acc| acc.balance)
            .unwrap_or(0)
    }

    /// Deduct a fee from an account.  Returns an error if the account does not
    /// have sufficient balance.
    pub fn deduct_fee(&mut self, from: crate::core::transaction::Address, fee: u64) -> Result<(), String> {
        if fee == 0 {
            return Ok(());
        }
        let account = self.state_tree.get_account(&from)
            .cloned()
            .unwrap_or(Account::new(0, 0));
        if account.balance < fee {
            return Err("Insufficient balance for fee".to_string());
        }
        let new_balance = account.balance - fee;
        let new_account = Account::new(new_balance, account.nonce);
        self.state_tree.update_account(from, new_account);
        self.current_state_root = self.state_tree.calculate_root();
        Ok(())
    }

    /// Credit an account with newly minted tokens (e.g. mining reward or gas
    /// fees).  This is equivalent to a transfer from the implicit genesis
    /// account.
    pub fn credit_account(&mut self, to: crate::core::transaction::Address, amount: u64) -> Result<(), String> {
        if amount == 0 {
            return Ok(());
        }

        // Enforce global supply cap (600M CTS): cannot mint more tokens beyond the hard cap.
        let current_supply = self.get_total_supply();
        if current_supply.saturating_add(amount) > crate::consensus::mining::CTS_MAX_SUPPLY {
            return Err("Cannot mint beyond maximum supply".to_string());
        }

        let account = self.state_tree.get_account(&to)
            .cloned()
            .unwrap_or(Account::new(0, 0));
        let new_balance = account.balance + amount;
        let new_account = Account::new(new_balance, account.nonce);
        self.state_tree.update_account(to, new_account);
        self.current_state_root = self.state_tree.calculate_root();
        Ok(())
    }

    pub fn get_account(&self, address: &Address) -> Option<&Account> {
        self.state_tree.get_account(address)
    }

    /// Get count of accounts with non-zero balance
    pub fn get_active_account_count(&self) -> u64 {
        let mut count = 0u64;
        self.state_tree.iter_accounts(|_addr, account| {
            if account.balance > 0 {
                count += 1;
            }
        });
        count
    }

    /// Helper invoked by the node runtime when a new block arrives.  This
    /// method ensures the state root is saved and then delegates to the
    /// provided pruner object to remove old block/tx entries.  It exists so
    /// that pruner logic is triggered from within the state manager context,
    /// satisfying the requirement that pruning be tied to state updates.
    pub fn snapshot_and_prune(
        &mut self,
        current_height: u64,
        pruner: &mut crate::storage::pruning::RollingWindowPruner,
    ) {
        // the pruner uses this snapshot internally
        pruner.maybe_prune(current_height, self);
    }

    /// Rebuild entire state by replaying transactions from a set of blocks.
    /// Used during chain reorganization to revert state to a prior canonical
    /// history.  This implementation resets the tree and reapplies each
    /// transaction in order.  Miner rewards/fees are not handled here (node
    /// logic can credit separately if needed).
    pub fn rebuild_from_blocks(&mut self, blocks: &[crate::core::Block]) -> Result<(), String> {
        self.state_tree = SparseMerkleTree::new();
        self.current_state_root = self.state_tree.calculate_root();
        for block in blocks {
            for tx in &block.transactions {
                self.apply_transaction(tx)?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Transaction;
    use secp256k1::{Secp256k1, SecretKey};
    use rand::{Rng, thread_rng};

    fn create_signed_transaction(from: Address, to: Address, amount: u64, nonce: u64, private_key: &SecretKey) -> Transaction {
        let mut tx = Transaction::new(from, to, amount, nonce, 21000, 1);
        tx.sign(private_key).unwrap();
        tx
    }

    #[test]
    fn test_apply_transfer_with_fee() {
        let mut mgr = StateManager::new();
        let sender: Address = [1; 20];
        let receiver: Address = [2; 20];

        mgr.state_tree.update_account(sender, crate::state::state_tree::Account::new(1000, 0));
        let tx = Transaction::new_transfer(sender, receiver, 100, 0, 21000, 2);
        // apply_transaction will deduct amount + fee (21000*2)
        assert!(mgr.apply_transaction(&tx).is_err()); // not enough for fee

        // give enough balance
        mgr.state_tree.update_account(sender, crate::state::state_tree::Account::new(50000, 0));
        assert!(mgr.apply_transaction(&tx).is_ok());
        let acc_sender = mgr.get_account(&sender).unwrap();
        let acc_rec = mgr.get_account(&receiver).unwrap();
        assert!(acc_sender.balance < 50000);
        assert_eq!(acc_rec.balance, 100);
    }

    #[test]
    fn test_fee_and_credit() {
        let mut mgr = StateManager::new();
        let addr: Address = [3; 20];
        mgr.state_tree.update_account(addr, crate::state::state_tree::Account::new(1000, 0));

        // deduct fee
        assert!(mgr.deduct_fee(addr, 200).is_ok());
        let acc = mgr.get_account(&addr).unwrap();
        assert_eq!(acc.balance, 800);

        // credit account
        assert!(mgr.credit_account(addr, 500).is_ok());
        let acc2 = mgr.get_account(&addr).unwrap();
        assert_eq!(acc2.balance, 1300);
    }
}