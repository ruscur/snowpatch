use anyhow::{bail, Context};
use log::error;
use sled::transaction::{ConflictableTransactionError, TransactionError};
use sled::{IVec, Iter, Transactional, Tree};
/// Helpers for database stuff.
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

///
pub fn db_collect_string_values(iter: Iter) -> Result<(Vec<String>, Vec<String>), anyhow::Error> {
    let mut keys: Vec<String> = vec![];
    let mut values: Vec<String> = vec![];
    for tuple in iter {
        match tuple {
            Ok(tuple) => {
                keys.push(String::from_utf8_lossy(&tuple.0).to_string());
                values.push(String::from_utf8_lossy(&tuple.1).to_string());
            }
            Err(e) => {
                bail!(format!("Something went wrong: {}", e.to_string()));
            }
        }
    }

    Ok((keys, values))
}

// this was a terrible idea
/*
pub fn append_to_str_vec(tree: &Tree, key: &[u8], value: &str) -> Result<(), anyhow::Error> {
    match tree.get(key)? {
        Some(v) => {
            let mut str_vec: Vec<&str> = bincode::deserialize(&v)?;
            if !str_vec.contains(&value) {
                str_vec.push(value);
                let new_value: Vec<u8> = bincode::serialize(&str_vec)?;
                tree.compare_and_swap(key, Some(v), Some(new_value))?;
            }
        }
        None => {}
    }

    Ok(())
}
*/

/// Does nothing until something on the tree changes.
pub fn wait_for_tree(tree: &Tree) {
    let sub = tree.watch_prefix(vec![]);
    // blocks until there's an update to the tree
    for _ in sub.take(1) {}
}
