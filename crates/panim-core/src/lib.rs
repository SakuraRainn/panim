//! The core of PANIM: personas, accounts, the local store and the connector interface.
//!
//! This crate has no user interface and no network code. Refer to `docs/ARCHITECTURE.md` §3.

pub mod account;
pub mod ids;
pub mod key;
pub mod persona;
pub mod proof;
pub mod secret;
pub mod store;
pub mod timeline;
pub mod words;

pub use account::{Account, BrowserAccess, JoinedAccess, NewAccount, Platform};
pub use ids::{AccountId, PersonaAddress, PersonaId};
pub use key::{Domain, KeyError, PersonaKey, PersonaSignature, VerifyError};
pub use persona::{NewPersona, Persona, ProxyEndpoint, ProxyKind, Route};
pub use proof::{Block, Date, KeptProof, ProofError, Statement};
pub use secret::{MemorySecrets, SecretBytes, SecretRef, SecretStore, SecretTable};
pub use store::{Error, MemoryStore, Result, Store, StoreMeta, check_new_account};
pub use timeline::{ItemKey, NewItem, TimelineFilter, TimelineItem, Timestamp};
pub use words::RecoveryWords;
