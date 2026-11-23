use crate::error::{Error, Result};
use axum::http::StatusCode;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use uuid::Uuid;

#[derive(Clone, Default)]
pub struct UserTransfers(Arc<Mutex<HashMap<Uuid, u8>>>);
pub struct TransferGuard {
    users: UserTransfers,
    user: Uuid,
}
impl UserTransfers {
    pub fn acquire(&self, user: Uuid) -> Result<TransferGuard> {
        let mut counts = self.0.lock().map_err(|_| Error::unavailable())?;
        let count = counts.entry(user).or_default();
        if *count >= 2 {
            return Err(Error(
                StatusCode::TOO_MANY_REQUESTS,
                "user_transfer_capacity",
            ));
        }
        *count += 1;
        Ok(TransferGuard {
            users: self.clone(),
            user,
        })
    }
}
impl Drop for TransferGuard {
    fn drop(&mut self) {
        if let Ok(mut counts) = self.users.0.lock()
            && let Some(count) = counts.get_mut(&self.user)
        {
            *count -= 1;
            if *count == 0 {
                counts.remove(&self.user);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn one_user_cannot_occupy_every_transfer_and_cancel_releases_slots() {
        let users = UserTransfers::default();
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let first = users.acquire(a).unwrap();
        let second = users.acquire(a).unwrap();
        assert!(users.acquire(a).is_err());
        let other = users.acquire(b).unwrap();
        drop(first);
        assert!(users.acquire(a).is_ok());
        drop(second);
        drop(other);
        assert!(users.0.lock().unwrap().is_empty());
    }
}
