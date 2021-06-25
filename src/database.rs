use sled::transaction::TransactionError;
use sled::{Transactional, Tree};
/// Helpers for database stuff.
use std::result::Result;

pub fn move_to_new_queue(old: &Tree, new: &Tree, key: &[u8]) -> Result<(), TransactionError> {
    (old, new).transaction(|(inbound, outbound)| {
        let value = inbound.remove(key)?.unwrap();
        outbound.insert(key, value)?;
        Ok(())
    })
}

/// Does nothing until something on the tree changes.
pub fn wait_for_tree(tree: &Tree) {
    let sub = tree.watch_prefix(vec![]);
    // blocks until there's an update to the tree
    for _ in sub.take(1) {}
}
