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
