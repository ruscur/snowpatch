/// Helpers for database stuff.
use anyhow::{bail, Context};
use log::error;
use sled::transaction::{ConflictableTransactionError, TransactionError};
use sled::{IVec, Iter, Transactional, Tree};
use std::result::Result;

pub fn move_to_new_queue(old: &Tree, new: &Tree, key: &[u8]) -> Result<(), TransactionError> {
    (old, new).transaction(|(inbound, outbound)| {
        let value = inbound.remove(key)?.ok_or_else(|| {
            error!(
                "move_to_new_queue() failed: {} {} {}",
                String::from_utf8_lossy(&old.name()),
                String::from_utf8_lossy(&new.name()),
                String::from_utf8_lossy(&key)
            );
            return ConflictableTransactionError::Conflict;
        })?;
        outbound.insert(key, value)?;
        Ok(())
    })
}

pub fn db_collect_string_values(iter: Iter) -> Result<Vec<(String, String)>, anyhow::Error> {
    let mut pairs: Vec<(String, String)> = vec![];
    for tuple in iter {
        match tuple {
            Ok(tuple) => {
                let key = String::from_utf8_lossy(&tuple.0).to_string();
                let value = String::from_utf8_lossy(&tuple.1).to_string();
                pairs.push((key, value));
            }
            Err(e) => {
                bail!(format!("Something went wrong: {}", e.to_string()));
            }
        }
    }

    Ok(pairs)
}

/// Does nothing until something on the tree changes.
pub fn wait_for_tree(tree: &Tree) {
    // event subscriber for any tree modifications
    let sub = tree.watch_prefix(vec![]);
    // we need to do this after the subscriber is configured to avoid any potential
    // race conditions.
    if !tree.is_empty() {
        return;
    }
    // blocks until there's an update to the tree
    for _ in sub.take(1) {}
}
