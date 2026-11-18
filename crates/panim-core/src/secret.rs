//! Secrets: passwords, tokens, API keys and persona keys.
//!
//! The core does not seal secrets. It uses the `SecretStore` interface. The crate `panim-vault` supplies a sealed
//! store behind this interface. Later, the family components `local-seal` and `sealed-layer` can replace it.

use std::collections::BTreeMap;
use std::fmt;

use zeroize::Zeroize;

use crate::store::Result;

/// A reference to a secret in a `SecretStore`. It does not contain the secret.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
pub struct SecretRef(pub u64);

/// The bytes of a secret.
///
/// The `Debug` form does not show the bytes, so the secret does not go into logs.
/// The bytes are set to zero when the value is dropped.
#[derive(Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SecretBytes(Vec<u8>);

impl SecretBytes {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    pub fn expose(&self) -> &[u8] {
        &self.0
    }
}

impl Drop for SecretBytes {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for SecretBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SecretBytes({} bytes)", self.0.len())
    }
}

/// A store for secrets.
pub trait SecretStore {
    /// Keeps a secret and gives a reference to it.
    fn put(&mut self, secret: SecretBytes) -> Result<SecretRef>;
    /// Gives the secret, or `None` if the store does not have it.
    fn get(&self, reference: SecretRef) -> Option<SecretBytes>;
    /// Removes the secret. Returns false if the store did not have it.
    fn remove(&mut self, reference: SecretRef) -> Result<bool>;
}

/// The secrets and the next reference, in a form that a sealed store can save.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct SecretTable {
    pub next: u64,
    pub secrets: BTreeMap<SecretRef, SecretBytes>,
}

impl SecretTable {
    pub fn put(&mut self, secret: SecretBytes) -> SecretRef {
        self.next += 1;
        let reference = SecretRef(self.next);
        self.secrets.insert(reference, secret);
        reference
    }
}

/// A secret store in memory, without a seal. Use it only for tests.
#[derive(Default)]
pub struct MemorySecrets {
    table: SecretTable,
}

impl SecretStore for MemorySecrets {
    fn put(&mut self, secret: SecretBytes) -> Result<SecretRef> {
        Ok(self.table.put(secret))
    }

    fn get(&self, reference: SecretRef) -> Option<SecretBytes> {
        self.table.secrets.get(&reference).cloned()
    }

    fn remove(&mut self, reference: SecretRef) -> Result<bool> {
        Ok(self.table.secrets.remove(&reference).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_form_does_not_show_the_secret() {
        let secret = SecretBytes::new(b"hunter2".to_vec());
        assert_eq!(format!("{secret:?}"), "SecretBytes(7 bytes)");
    }

    #[test]
    fn memory_secrets_keep_and_remove() {
        let mut store = MemorySecrets::default();
        let reference = store.put(SecretBytes::new(b"token".to_vec())).unwrap();
        assert_eq!(store.get(reference).unwrap().expose(), b"token");
        assert!(store.remove(reference).unwrap());
        assert!(store.get(reference).is_none());
        assert!(!store.remove(reference).unwrap());
    }
}
