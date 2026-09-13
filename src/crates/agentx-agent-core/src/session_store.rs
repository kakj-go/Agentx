use std::collections::BTreeMap;

use crate::{AgentSessionEntryV1, AgentSessionRegisterV1, AgentSessionUsageV1};

/// A small deterministic domain repository used by Core/Runtime contract
/// tests.  SQL adapters implement the same append/CAS rules at the boundary.
#[derive(Default)]
pub struct InMemorySessionRepository {
    entries: BTreeMap<String, Vec<AgentSessionEntryV1>>,
    registers: BTreeMap<String, AgentSessionRegisterV1>,
    usages: BTreeMap<String, AgentSessionUsageV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionRepositoryError {
    Conflict,
    LeaseLost,
    EntryAlreadyExists,
    UsageAlreadyExists,
    InvalidParent,
    InvalidEntry,
    InvalidRegister,
}

impl InMemorySessionRepository {
    pub fn append_entry(
        &mut self,
        key: &str,
        entry: AgentSessionEntryV1,
    ) -> Result<(), SessionRepositoryError> {
        entry
            .validate()
            .map_err(|_| SessionRepositoryError::InvalidEntry)?;
        let values = self.entries.entry(key.to_owned()).or_default();
        if values
            .iter()
            .any(|current| current.entry_id == entry.entry_id)
        {
            return Err(SessionRepositoryError::EntryAlreadyExists);
        }
        if let Some(parent) = &entry.parent_entry_id
            && !values.iter().any(|current| &current.entry_id == parent)
        {
            return Err(SessionRepositoryError::InvalidParent);
        }
        values.push(entry);
        Ok(())
    }

    pub fn entries(&self, key: &str) -> &[AgentSessionEntryV1] {
        self.entries.get(key).map(Vec::as_slice).unwrap_or_default()
    }

    pub fn cas_register(
        &mut self,
        key: &str,
        expected_state_version: u64,
        fencing_token: u64,
        mut register: AgentSessionRegisterV1,
    ) -> Result<u64, SessionRepositoryError> {
        let current = self.registers.get(key);
        if let Some(current) = current {
            if current.state_version != expected_state_version {
                return Err(SessionRepositoryError::Conflict);
            }
            if current.fencing_token > fencing_token {
                return Err(SessionRepositoryError::LeaseLost);
            }
        } else if expected_state_version != 0 {
            return Err(SessionRepositoryError::Conflict);
        }
        register
            .validate()
            .map_err(|_| SessionRepositoryError::InvalidRegister)?;
        register.state_version = expected_state_version.saturating_add(1);
        register.fencing_token = fencing_token;
        self.registers.insert(key.to_owned(), register);
        Ok(expected_state_version.saturating_add(1))
    }

    pub fn register(&self, key: &str) -> Option<&AgentSessionRegisterV1> {
        self.registers.get(key)
    }

    pub fn append_usage(
        &mut self,
        usage: AgentSessionUsageV1,
    ) -> Result<(), SessionRepositoryError> {
        usage
            .validate()
            .map_err(|_| SessionRepositoryError::InvalidEntry)?;
        let key = format!(
            "{}:{}:{}",
            usage.session_id, usage.operation_id, usage.effect_id
        );
        if self.usages.contains_key(&key) {
            return Err(SessionRepositoryError::UsageAlreadyExists);
        }
        self.usages.insert(key, usage);
        Ok(())
    }

    pub fn usage_count(&self) -> usize {
        self.usages.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AgentSessionEntryKindV1, AgentSessionLaneV1};
    use serde_json::json;

    fn entry(id: &str, parent: Option<&str>) -> AgentSessionEntryV1 {
        AgentSessionEntryV1 {
            entry_id: id.into(),
            session_id: "s".into(),
            lane: AgentSessionLaneV1::Main,
            parent_entry_id: parent.map(str::to_owned),
            entry_kind: AgentSessionEntryKindV1::MessageUser,
            payload_json: Some(json!({"id":id})),
            payload_artifact_id: None,
            operation_id: None,
            created_at_millis: 1,
        }
    }

    #[test]
    fn entries_are_append_only_and_parented() {
        let mut store = InMemorySessionRepository::default();
        store.append_entry("k", entry("1", None)).unwrap();
        assert_eq!(
            store.append_entry("k", entry("2", Some("missing"))),
            Err(SessionRepositoryError::InvalidParent)
        );
        store.append_entry("k", entry("2", Some("1"))).unwrap();
        assert_eq!(
            store.append_entry("k", entry("2", Some("1"))),
            Err(SessionRepositoryError::EntryAlreadyExists)
        );
    }

    #[test]
    fn register_cas_and_fencing_are_monotonic() {
        let mut store = InMemorySessionRepository::default();
        let register = AgentSessionRegisterV1 {
            session_id: "s".into(),
            ..AgentSessionRegisterV1::default()
        };
        assert_eq!(store.cas_register("k", 0, 3, register.clone()), Ok(1));
        assert_eq!(
            store.cas_register("k", 0, 4, register.clone()),
            Err(SessionRepositoryError::Conflict)
        );
        assert_eq!(
            store.cas_register("k", 1, 2, register),
            Err(SessionRepositoryError::LeaseLost)
        );
    }

    #[test]
    fn usage_effect_is_unique() {
        let mut store = InMemorySessionRepository::default();
        let usage = AgentSessionUsageV1 {
            usage_id: "u".into(),
            session_id: "s".into(),
            operation_id: "o".into(),
            effect_id: "e".into(),
            usage_kind: crate::UsageKindV1::Model,
            resource_reference: None,
            input_tokens: 1,
            output_tokens: 1,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            cost_micros: 1,
            cost_currency: None,
            created_at_millis: 1,
        };
        store.append_usage(usage.clone()).unwrap();
        assert_eq!(
            store.append_usage(usage),
            Err(SessionRepositoryError::UsageAlreadyExists)
        );
    }

    #[test]
    fn payload_must_have_exactly_one_storage_location() {
        let mut store = InMemorySessionRepository::default();
        let mut value = entry("inline", None);
        value.payload_json = None;
        value.payload_artifact_id = None;
        assert_eq!(
            store.append_entry("k", value),
            Err(SessionRepositoryError::InvalidEntry)
        );
    }
}
