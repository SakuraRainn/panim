//! The sealed store and the sealed secret store.
//!
//! `SealedStore` keeps a `MemoryStore` and writes its parts to the vault after each change:
//!
//! - `store/meta`: the personas, the accounts and the next identifiers.
//! - `store/items/<account>`: the timeline items of one account. A change to one account writes only its file.
//! - `store/proofs`: the latest proof block that PANIM made for each account, and its location (milestone 6).
//!
//! The store writes its parts with `postcard`. This form has no field names, so a new field in a stored struct
//! makes the old files unreadable. Thus, new data goes into a new place. A vault from before milestone 6 has no
//! `store/proofs`, and the store reads that as "no proofs". The file of the proofs starts with a version number.
//!
//! `SealedSecrets` writes all secrets to `secrets` after each change.
//!
//! Each change keeps a copy of the part that it changes: the meta, the items of one account, the proofs or the
//! secret table. If the write fails, the change puts the copy back and gives `Error::Storage`. Thus, the data in
//! memory stays the same as the data on the disk. `Vault::write` writes a temporary file and then renames it, so
//! each file on the disk is complete.
//!
//! `remove_account` writes more than one file. It writes the meta first. If a later step fails, an items file or a
//! proof without its account stays on the disk. `SealedStore::open` ignores this data.

use std::sync::Arc;

use std::collections::BTreeMap;

use panim_core::{
    Account, AccountId, Error, ItemKey, KeptProof, MemoryStore, NewAccount, NewItem, NewPersona, Persona, PersonaId,
    Result, Route, SecretBytes, SecretRef, SecretStore, SecretTable, Store, StoreMeta, TimelineFilter, TimelineItem,
};

use crate::Vault;

const META: &str = "store/meta";
const SECRETS: &str = "secrets";
const PROOFS: &str = "store/proofs";
/// The version of the form of `store/proofs`. A new form gets a new number.
const PROOFS_VERSION: u32 = 1;

/// The form of `store/proofs`.
#[derive(serde::Serialize, serde::Deserialize)]
struct ProofsFile {
    version: u32,
    proofs: Vec<(AccountId, KeptProof)>,
}

impl Default for ProofsFile {
    fn default() -> Self {
        Self {
            version: PROOFS_VERSION,
            proofs: Vec::new(),
        }
    }
}

fn items_place(account: AccountId) -> String {
    format!("store/items/{account}")
}

fn storage(error: impl std::fmt::Display) -> Error {
    Error::Storage(error.to_string())
}

fn load<T: serde::de::DeserializeOwned + Default>(vault: &Vault, place: &str) -> Result<T> {
    match vault.read(place).map_err(storage)? {
        Some(bytes) => postcard::from_bytes(&bytes).map_err(|e| storage(format!("{place}: {e}"))),
        None => Ok(T::default()),
    }
}

fn save<T: serde::Serialize>(vault: &Vault, place: &str, value: &T) -> Result<()> {
    let bytes = zeroize::Zeroizing::new(postcard::to_allocvec(value).map_err(storage)?);
    vault.write(place, &bytes).map_err(storage)
}

/// The local store, sealed in the vault.
pub struct SealedStore {
    vault: Arc<Vault>,
    inner: MemoryStore,
    /// The kept proof of each account (`store/proofs`).
    proofs: BTreeMap<AccountId, KeptProof>,
}

impl SealedStore {
    /// Reads the store from the vault. A new vault gives an empty store.
    pub fn open(vault: Arc<Vault>) -> Result<Self> {
        let meta: StoreMeta = load(&vault, META)?;
        let mut items = Vec::new();
        for account in &meta.accounts {
            items.extend(load::<Vec<TimelineItem>>(&vault, &items_place(account.id))?);
        }
        let file: ProofsFile = load(&vault, PROOFS)?;
        if file.version != PROOFS_VERSION {
            return Err(storage(format!("{PROOFS}: the version {} is not known", file.version)));
        }
        // A proof of an account that does not exist is not kept.
        let proofs = file
            .proofs
            .into_iter()
            .filter(|(account, _)| meta.accounts.iter().any(|a| a.id == *account))
            .collect();
        Ok(Self {
            inner: MemoryStore::from_parts(meta, items),
            vault,
            proofs,
        })
    }

    /// The kept proof of an account, if PANIM made one.
    pub fn kept_proof(&self, account: AccountId) -> Option<&KeptProof> {
        self.proofs.get(&account)
    }

    /// Keeps the proof of an account. It replaces the proof that the store had for the account.
    pub fn keep_proof(&mut self, account: AccountId, proof: KeptProof) -> Result<()> {
        if self.inner.account(account).is_none() {
            return Err(Error::UnknownAccount(account));
        }
        let old = self.proofs.insert(account, proof);
        if let Err(error) = self.save_proofs() {
            match old {
                Some(old) => self.proofs.insert(account, old),
                None => self.proofs.remove(&account),
            };
            return Err(error);
        }
        Ok(())
    }

    /// Removes the kept proof of an account. Returns true if there was a kept proof.
    pub fn forget_proof(&mut self, account: AccountId) -> Result<bool> {
        let Some(old) = self.proofs.remove(&account) else {
            return Ok(false);
        };
        if let Err(error) = self.save_proofs() {
            self.proofs.insert(account, old);
            return Err(error);
        }
        Ok(true)
    }

    fn save_proofs(&self) -> Result<()> {
        let file = ProofsFile {
            version: PROOFS_VERSION,
            proofs: self.proofs.iter().map(|(id, proof)| (*id, proof.clone())).collect(),
        };
        save(&self.vault, PROOFS, &file)
    }

    fn save_meta(&self) -> Result<()> {
        save(&self.vault, META, &self.inner.meta())
    }

    fn save_items(&self, account: AccountId) -> Result<()> {
        save(&self.vault, &items_place(account), &self.inner.items_of(account))
    }

    /// Does a change to the meta and writes the meta. If the write fails, the meta in memory goes back.
    fn change_meta<T>(&mut self, change: impl FnOnce(&mut MemoryStore) -> Result<T>) -> Result<T> {
        let before = self.inner.meta();
        let value = change(&mut self.inner)?;
        if let Err(error) = self.save_meta() {
            self.inner.restore_meta(before);
            return Err(error);
        }
        Ok(value)
    }

    /// Does a change to the items of one account. If `changed` is true for the result, the store writes the items
    /// of the account. If the write fails, the items of the account in memory go back.
    fn change_items<T>(
        &mut self,
        account: AccountId,
        change: impl FnOnce(&mut MemoryStore) -> Result<T>,
        changed: impl FnOnce(&T) -> bool,
    ) -> Result<T> {
        let before = self.inner.items_of(account);
        let value = change(&mut self.inner)?;
        if changed(&value)
            && let Err(error) = self.save_items(account)
        {
            self.inner.replace_items(account, before);
            return Err(error);
        }
        Ok(value)
    }
}

impl Store for SealedStore {
    fn add_persona(&mut self, persona: NewPersona) -> Result<Persona> {
        self.change_meta(|inner| inner.add_persona(persona))
    }

    fn persona(&self, id: PersonaId) -> Option<Persona> {
        self.inner.persona(id)
    }

    fn personas(&self) -> Vec<Persona> {
        self.inner.personas()
    }

    fn set_route(&mut self, persona: PersonaId, route: Route) -> Result<()> {
        self.change_meta(|inner| inner.set_route(persona, route))
    }

    fn add_account(&mut self, account: NewAccount) -> Result<Account> {
        self.change_meta(|inner| inner.add_account(account))
    }

    fn account(&self, id: AccountId) -> Option<Account> {
        self.inner.account(id)
    }

    fn accounts(&self, persona: Option<PersonaId>) -> Vec<Account> {
        self.inner.accounts(persona)
    }

    /// Removes the account from the meta first. If that write fails, nothing changes.
    ///
    /// After the meta is on the disk, the account is removed. Then the store removes the items file and the kept
    /// proof. If one of these steps fails, the method still gives the account, because the account is not on the
    /// disk. The file or the proof stays on the disk, but `SealedStore::open` ignores the data of an account that
    /// is not in the meta. The memory has no data of the account, the same as the store that opens again.
    fn remove_account(&mut self, id: AccountId) -> Result<Account> {
        let items = self.inner.items_of(id);
        let before = self.inner.meta();
        let account = self.inner.remove_account(id)?;
        if let Err(error) = self.save_meta() {
            self.inner.restore_meta(before);
            self.inner.replace_items(id, items);
            return Err(error);
        }
        // An error here does not change the result. The next write of the proofs does not include the proof.
        let _ = self.vault.remove(&items_place(id));
        if self.proofs.remove(&id).is_some() {
            let _ = self.save_proofs();
        }
        Ok(account)
    }

    fn set_identity(&mut self, id: AccountId, handle: String, remote_id: Option<String>) -> Result<Account> {
        self.change_meta(|inner| inner.set_identity(id, handle, remote_id))
    }

    fn put_items(&mut self, account: AccountId, items: Vec<NewItem>) -> Result<usize> {
        self.change_items(account, |inner| inner.put_items(account, items), |_| true)
    }

    fn remove_items(&mut self, account: AccountId, source_ids: &[String]) -> Result<usize> {
        self.change_items(
            account,
            |inner| inner.remove_items(account, source_ids),
            |removed| *removed > 0,
        )
    }

    fn timeline(&self, filter: &TimelineFilter) -> Vec<TimelineItem> {
        self.inner.timeline(filter)
    }

    fn set_read(&mut self, key: &ItemKey, read: bool) -> Result<()> {
        self.change_items(key.account, |inner| inner.set_read(key, read), |_| true)
    }

    fn set_read_many(&mut self, account: AccountId, changes: &[(String, bool)]) -> Result<usize> {
        self.change_items(
            account,
            |inner| inner.set_read_many(account, changes),
            |changed| *changed > 0,
        )
    }

    fn unread_count(&self, account: AccountId) -> usize {
        self.inner.unread_count(account)
    }
}

/// The secret store, sealed in the vault.
pub struct SealedSecrets {
    vault: Arc<Vault>,
    table: SecretTable,
}

impl SealedSecrets {
    pub fn open(vault: Arc<Vault>) -> Result<Self> {
        Ok(Self {
            table: load(&vault, SECRETS)?,
            vault,
        })
    }
}

impl SecretStore for SealedSecrets {
    fn put(&mut self, secret: SecretBytes) -> Result<SecretRef> {
        let next = self.table.next;
        let reference = self.table.put(secret);
        if let Err(error) = save(&self.vault, SECRETS, &self.table) {
            self.table.secrets.remove(&reference);
            self.table.next = next;
            return Err(error);
        }
        Ok(reference)
    }

    fn get(&self, reference: SecretRef) -> Option<SecretBytes> {
        self.table.secrets.get(&reference).cloned()
    }

    fn remove(&mut self, reference: SecretRef) -> Result<bool> {
        let Some(old) = self.table.secrets.remove(&reference) else {
            return Ok(false);
        };
        if let Err(error) = save(&self.vault, SECRETS, &self.table) {
            self.table.secrets.insert(reference, old);
            return Err(error);
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::KdfParams;
    use panim_core::{JoinedAccess, PersonaKey, Platform, ProxyEndpoint, ProxyKind, Timestamp};

    fn item(id: &str, time: i64) -> NewItem {
        NewItem {
            source_id: id.into(),
            author: "a".into(),
            time: Timestamp(time),
            text: format!("text {id}"),
            link: None,
            raw: vec![1, 2, 3],
        }
    }

    #[test]
    fn data_stays_after_the_vault_closes() {
        let dir = tempfile::tempdir().unwrap();
        let key = PersonaKey::generate().unwrap();
        let vault = Arc::new(Vault::create(dir.path(), "passcode1", &key, KdfParams::TEST).unwrap());

        let mut secrets = SealedSecrets::open(vault.clone()).unwrap();
        let key_ref = secrets.put(key.to_secret()).unwrap();
        let token = secrets.put(SecretBytes::new(b"gho_token".to_vec())).unwrap();

        let mut store = SealedStore::open(vault.clone()).unwrap();
        let me = store
            .add_persona(NewPersona {
                name: "Me".into(),
                address: key.address(),
                route: Route::Direct,
            })
            .unwrap();
        let github = store
            .add_account(NewAccount {
                persona: me.id,
                platform: Platform::GitHub,
                display_name: "GitHub".into(),
                handle: "me".into(),
                remote_id: Some("1".into()),
                joined: Some(JoinedAccess { credentials: token }),
                browser: None,
            })
            .unwrap();
        store
            .put_items(github.id, vec![item("x", 1), item("y", 2), item("z", 3)])
            .unwrap();
        assert_eq!(store.remove_items(github.id, &["z".to_string()]), Ok(1));
        assert_eq!(store.set_read_many(github.id, &[("y".to_string(), true)]), Ok(1));
        assert_eq!(store.set_read_many(github.id, &[("y".to_string(), false)]), Ok(1));
        store
            .set_read(
                &ItemKey {
                    account: github.id,
                    source_id: "x".into(),
                },
                true,
            )
            .unwrap();
        let timeline = store.timeline(&TimelineFilter::default());
        drop((store, secrets, vault));

        let vault = Arc::new(Vault::open(dir.path(), "passcode1", KdfParams::TEST).unwrap());
        let store = SealedStore::open(vault.clone()).unwrap();
        let secrets = SealedSecrets::open(vault).unwrap();
        assert_eq!(store.personas(), vec![me]);
        assert_eq!(store.timeline(&TimelineFilter::default()), timeline);
        assert_eq!(store.unread_count(github.id), 1);
        assert_eq!(secrets.get(token).unwrap().expose(), b"gho_token");
        let again = PersonaKey::from_secret(&secrets.get(key_ref).unwrap()).unwrap();
        assert_eq!(again.address(), key.address());
    }

    #[test]
    fn removing_an_account_removes_its_file() {
        let dir = tempfile::tempdir().unwrap();
        let key = PersonaKey::generate().unwrap();
        let vault = Arc::new(Vault::create(dir.path(), "passcode1", &key, KdfParams::TEST).unwrap());
        let mut store = SealedStore::open(vault.clone()).unwrap();
        let me = store
            .add_persona(NewPersona {
                name: "Me".into(),
                address: key.address(),
                route: Route::Direct,
            })
            .unwrap();
        let account = store
            .add_account(NewAccount {
                persona: me.id,
                platform: Platform::Telegram,
                display_name: "T".into(),
                handle: "me".into(),
                remote_id: None,
                joined: Some(JoinedAccess {
                    credentials: SecretRef(1),
                }),
                browser: None,
            })
            .unwrap();
        store.put_items(account.id, vec![item("x", 1)]).unwrap();
        assert!(vault.read(&items_place(account.id)).unwrap().is_some());
        store.remove_account(account.id).unwrap();
        assert!(vault.read(&items_place(account.id)).unwrap().is_none());
    }

    /// The stored structs of the store before milestone 6, as the code at commit ae55bb4 had them. `postcard` has
    /// no field names, so the sequence and the types of the fields are the stored form. Do not change these copies.
    mod old {
        use serde::{Deserialize, Serialize};

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        pub struct StoreMeta {
            pub next_persona: u64,
            pub next_account: u64,
            pub personas: Vec<Persona>,
            pub accounts: Vec<Account>,
        }

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        pub struct Persona {
            pub id: u64,
            pub name: String,
            pub address: [u8; 20],
            pub route: Route,
        }

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        pub enum Route {
            Direct,
            Proxy(ProxyEndpoint),
        }

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        pub struct ProxyEndpoint {
            pub kind: ProxyKind,
            pub host: String,
            pub port: u16,
        }

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        pub enum ProxyKind {
            Http,
            Socks5,
        }

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        pub enum Platform {
            GitHub,
            Telegram,
            Email,
            X,
            Discord,
        }

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        pub struct Account {
            pub id: u64,
            pub persona: u64,
            pub platform: Platform,
            pub display_name: String,
            pub handle: String,
            pub remote_id: Option<String>,
            pub joined: Option<Joined>,
            pub browser: Option<Browser>,
        }

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        pub struct Joined {
            pub credentials: u64,
        }

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        pub struct Browser {
            pub start_url: String,
        }

        #[derive(Serialize, Deserialize, Debug, PartialEq)]
        pub struct TimelineItem {
            pub account: u64,
            pub source_id: String,
            pub persona: u64,
            pub author: String,
            pub time: i64,
            pub text: String,
            pub link: Option<String>,
            pub read: bool,
            pub raw: Vec<u8>,
        }
    }

    #[test]
    fn a_vault_from_before_milestone_6_opens_and_keeps_proofs() {
        let dir = tempfile::tempdir().unwrap();
        let key = PersonaKey::generate().unwrap();
        let vault = Arc::new(Vault::create(dir.path(), "passcode1", &key, KdfParams::TEST).unwrap());
        // Write the files as the old code wrote them.
        let account = |id: u64, platform, handle: &str, remote_id: Option<&str>, joined: bool| old::Account {
            id,
            persona: 1,
            platform,
            display_name: handle.into(),
            handle: handle.into(),
            remote_id: remote_id.map(Into::into),
            joined: joined.then_some(old::Joined { credentials: 3 }),
            browser: (!joined).then(|| old::Browser {
                start_url: "https://x.com".into(),
            }),
        };
        let meta = old::StoreMeta {
            next_persona: 1,
            next_account: 2,
            personas: vec![old::Persona {
                id: 1,
                name: "Me".into(),
                address: key.address().0,
                route: old::Route::Proxy(old::ProxyEndpoint {
                    kind: old::ProxyKind::Socks5,
                    host: "127.0.0.1".into(),
                    port: 9050,
                }),
            }],
            accounts: vec![
                account(1, old::Platform::GitHub, "octocat", Some("583231"), true),
                account(2, old::Platform::X, "@me", None, false),
            ],
        };
        let items = vec![old::TimelineItem {
            account: 1,
            source_id: "n/1".into(),
            persona: 1,
            author: "a".into(),
            time: 5,
            text: "hello".into(),
            link: Some("https://github.com/".into()),
            read: true,
            raw: vec![7],
        }];
        vault.write(META, &postcard::to_allocvec(&meta).unwrap()).unwrap();
        vault
            .write(&items_place(AccountId(1)), &postcard::to_allocvec(&items).unwrap())
            .unwrap();
        assert!(vault.read(PROOFS).unwrap().is_none(), "an old vault has no proofs");

        // The new code reads the old vault. It has no proofs.
        let mut store = SealedStore::open(vault.clone()).unwrap();
        assert_eq!(store.accounts(None).len(), 2);
        assert_eq!(store.timeline(&TimelineFilter::default())[0].text, "hello");
        assert_eq!(store.kept_proof(AccountId(1)), None);

        // The person gives the id of the X account, and PANIM keeps two proofs.
        let x = store
            .set_identity(AccountId(2), "me".into(), Some("12345".into()))
            .unwrap();
        assert_eq!(x.remote_id.as_deref(), Some("12345"));
        let proof = |text: &str| KeptProof {
            block: text.into(),
            location: None,
        };
        store.keep_proof(AccountId(1), proof("block 1")).unwrap();
        store.keep_proof(AccountId(2), proof("block 2")).unwrap();
        assert_eq!(
            store.keep_proof(AccountId(9), proof("x")),
            Err(Error::UnknownAccount(AccountId(9)))
        );
        drop((store, vault));

        let vault = Arc::new(Vault::open(dir.path(), "passcode1", KdfParams::TEST).unwrap());
        let store = SealedStore::open(vault.clone()).unwrap();
        assert_eq!(store.kept_proof(AccountId(1)), Some(&proof("block 1")));
        assert_eq!(store.account(AccountId(2)).unwrap().remote_id.as_deref(), Some("12345"));
        // The meta file keeps the old form. Thus, the old code can still read it.
        let again: old::StoreMeta = postcard::from_bytes(&vault.read(META).unwrap().unwrap()).unwrap();
        assert_eq!(again.accounts[1].remote_id.as_deref(), Some("12345"));
        assert_eq!(again.accounts[1].handle, "me");
        assert_eq!(again.personas, meta.personas);
    }

    #[test]
    fn removing_an_account_removes_its_kept_proof() {
        let dir = tempfile::tempdir().unwrap();
        let key = PersonaKey::generate().unwrap();
        let vault = Arc::new(Vault::create(dir.path(), "passcode1", &key, KdfParams::TEST).unwrap());
        let mut store = SealedStore::open(vault.clone()).unwrap();
        let me = store
            .add_persona(NewPersona {
                name: "Me".into(),
                address: key.address(),
                route: Route::Direct,
            })
            .unwrap();
        let mut add = || {
            store
                .add_account(NewAccount {
                    persona: me.id,
                    platform: Platform::GitHub,
                    display_name: "G".into(),
                    handle: "me".into(),
                    remote_id: Some("1".into()),
                    joined: Some(JoinedAccess {
                        credentials: SecretRef(1),
                    }),
                    browser: None,
                })
                .unwrap()
        };
        let (first, second) = (add(), add());
        let proof = KeptProof {
            block: "b".into(),
            location: Some("https://gist.github.com/me/1".into()),
        };
        store.keep_proof(first.id, proof.clone()).unwrap();
        store.keep_proof(second.id, proof.clone()).unwrap();
        store.remove_account(first.id).unwrap();
        assert_eq!(store.kept_proof(first.id), None);
        drop(store);
        let store = SealedStore::open(vault).unwrap();
        assert_eq!(store.kept_proof(first.id), None);
        assert_eq!(store.kept_proof(second.id), Some(&proof));
    }

    /// The data of a store that a failed write must not change.
    #[derive(Debug, PartialEq)]
    struct Snapshot {
        meta: StoreMeta,
        timeline: Vec<TimelineItem>,
        proofs: BTreeMap<AccountId, KeptProof>,
    }

    fn snapshot(store: &SealedStore) -> Snapshot {
        Snapshot {
            meta: store.inner.meta(),
            timeline: store.timeline(&TimelineFilter::default()),
            proofs: store.proofs.clone(),
        }
    }

    /// A vault with a persona, two GitHub accounts with items, and a kept proof for the first account.
    struct Fixture {
        _dir: tempfile::TempDir,
        vault: Arc<Vault>,
        store: SealedStore,
        persona: PersonaId,
        first: AccountId,
        second: AccountId,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let key = PersonaKey::generate().unwrap();
        let vault = Arc::new(Vault::create(dir.path(), "passcode1", &key, KdfParams::TEST).unwrap());
        let mut store = SealedStore::open(vault.clone()).unwrap();
        let persona = store
            .add_persona(NewPersona {
                name: "Me".into(),
                address: key.address(),
                route: Route::Direct,
            })
            .unwrap()
            .id;
        let mut ids = Vec::new();
        for _ in 0..2 {
            let account = store.add_account(github(persona)).unwrap();
            store.put_items(account.id, vec![item("x", 1), item("y", 2)]).unwrap();
            ids.push(account.id);
        }
        let proof = KeptProof {
            block: "b".into(),
            location: None,
        };
        store.keep_proof(ids[0], proof).unwrap();
        Fixture {
            _dir: dir,
            vault,
            store,
            persona,
            first: ids[0],
            second: ids[1],
        }
    }

    fn github(persona: PersonaId) -> NewAccount {
        NewAccount {
            persona,
            platform: Platform::GitHub,
            display_name: "G".into(),
            handle: "me".into(),
            remote_id: Some("1".into()),
            joined: Some(JoinedAccess {
                credentials: SecretRef(1),
            }),
            browser: None,
        }
    }

    /// Makes the next write fail and does the change. The change must give `Error::Storage`. The data in memory and
    /// the data of a store that opens again must be the same as before the change.
    fn fails_without_change<T: std::fmt::Debug>(f: &mut Fixture, change: impl FnOnce(&mut SealedStore) -> Result<T>) {
        let before = snapshot(&f.store);
        Vault::fail_next_write();
        let result = change(&mut f.store);
        assert!(matches!(result, Err(Error::Storage(_))), "{result:?}");
        assert_eq!(snapshot(&f.store), before);
        assert_eq!(snapshot(&SealedStore::open(f.vault.clone()).unwrap()), before);
    }

    #[test]
    fn a_failed_write_does_not_change_the_meta() {
        let mut f = fixture();
        let (persona, first) = (f.persona, f.first);
        fails_without_change(&mut f, |s| {
            s.add_persona(NewPersona {
                name: "Other".into(),
                address: PersonaKey::generate().unwrap().address(),
                route: Route::Direct,
            })
        });
        fails_without_change(&mut f, |s| {
            s.set_route(
                persona,
                Route::Proxy(ProxyEndpoint {
                    kind: ProxyKind::Socks5,
                    host: "127.0.0.1".into(),
                    port: 9050,
                }),
            )
        });
        fails_without_change(&mut f, |s| s.add_account(github(persona)));
        fails_without_change(&mut f, |s| s.set_identity(first, "other".into(), Some("2".into())));
        fails_without_change(&mut f, |s| s.remove_account(first));
        // The hook stopped only one write each time. The next change succeeds.
        f.store.set_identity(first, "other".into(), None).unwrap();
        assert_eq!(f.store.account(first).unwrap().handle, "other");
    }

    #[test]
    fn a_failed_write_does_not_change_the_items() {
        let mut f = fixture();
        let first = f.first;
        let key = ItemKey {
            account: first,
            source_id: "x".into(),
        };
        f.store.set_read(&key, true).unwrap();
        // A new item, and a later time for an item that is read.
        fails_without_change(&mut f, |s| s.put_items(first, vec![item("z", 3), item("x", 5)]));
        fails_without_change(&mut f, |s| s.remove_items(first, &["x".to_string()]));
        fails_without_change(&mut f, |s| s.set_read(&key, false));
        fails_without_change(&mut f, |s| s.set_read_many(first, &[("y".to_string(), true)]));
    }

    #[test]
    fn a_failed_write_does_not_change_the_proofs() {
        let mut f = fixture();
        let (first, second) = (f.first, f.second);
        let proof = KeptProof {
            block: "new".into(),
            location: Some("https://gist.github.com/me/1".into()),
        };
        // A replaced proof and a new proof.
        fails_without_change(&mut f, |s| s.keep_proof(first, proof.clone()));
        fails_without_change(&mut f, |s| s.keep_proof(second, proof.clone()));
        fails_without_change(&mut f, |s| s.forget_proof(first));
    }

    #[test]
    fn remove_account_succeeds_when_a_step_after_the_meta_fails() {
        let mut f = fixture();
        let first = f.first;
        // The write of the meta occurs. The write of the proofs fails.
        Vault::fail_write_after(1);
        assert_eq!(f.store.remove_account(first).map(|a| a.id), Ok(first));
        let after = snapshot(&f.store);
        assert!(after.meta.accounts.iter().all(|a| a.id != first));
        assert!(after.timeline.iter().all(|i| i.key.account != first));
        assert_eq!(after.proofs.get(&first), None);
        // The old proof is on the disk, but the store that opens again ignores it.
        assert_eq!(snapshot(&SealedStore::open(f.vault.clone()).unwrap()), after);
    }

    #[test]
    fn a_failed_write_does_not_change_the_secrets() {
        let dir = tempfile::tempdir().unwrap();
        let key = PersonaKey::generate().unwrap();
        let vault = Arc::new(Vault::create(dir.path(), "passcode1", &key, KdfParams::TEST).unwrap());
        let mut secrets = SealedSecrets::open(vault.clone()).unwrap();
        let token = secrets.put(SecretBytes::new(b"token".to_vec())).unwrap();
        let state = |secrets: &SealedSecrets| {
            let table = &secrets.table;
            let entries: Vec<_> = table.secrets.iter().map(|(r, s)| (*r, s.expose().to_vec())).collect();
            (table.next, entries)
        };
        let before = state(&secrets);

        Vault::fail_next_write();
        assert!(matches!(
            secrets.put(SecretBytes::new(b"new".to_vec())),
            Err(Error::Storage(_))
        ));
        assert_eq!(state(&secrets), before);
        assert_eq!(state(&SealedSecrets::open(vault.clone()).unwrap()), before);

        Vault::fail_next_write();
        assert!(matches!(secrets.remove(token), Err(Error::Storage(_))));
        assert_eq!(state(&secrets), before);
        assert_eq!(state(&SealedSecrets::open(vault.clone()).unwrap()), before);

        // The reference of the failed put is free, so the next put gets it.
        assert_eq!(secrets.put(SecretBytes::new(b"new".to_vec())), Ok(SecretRef(2)));
    }
}
