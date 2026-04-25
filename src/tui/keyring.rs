use std::collections::HashMap;

#[derive(Debug)]
pub struct KeyringEntry {
    pub username: String,
    pub password: String,
}

pub trait KeyringStore {
    fn set(&mut self, role: &str, username: &str, password: &str) -> Result<(), String>;
    fn get(&self, role: &str) -> Option<&KeyringEntry>;
}

/// In-memory keyring used in tests and until STEP 05 wires the real OS keychain.
#[derive(Default)]
pub struct InMemoryKeyringStore {
    entries: HashMap<String, KeyringEntry>,
}

impl KeyringStore for InMemoryKeyringStore {
    fn set(&mut self, role: &str, username: &str, password: &str) -> Result<(), String> {
        self.entries.insert(
            role.to_string(),
            KeyringEntry { username: username.to_string(), password: password.to_string() },
        );
        Ok(())
    }

    fn get(&self, role: &str) -> Option<&KeyringEntry> {
        self.entries.get(role)
    }
}
