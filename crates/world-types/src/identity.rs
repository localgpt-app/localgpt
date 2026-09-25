//! Entity identity — dual ID + name system.
//!
//! Entities have a stable numeric [`EntityId`] (survives renames) and a
//! human-readable [`EntityName`] (LLM-friendly).  Cross-entity references
//! use [`EntityRef`] which can be either form and is resolved to an ID on
//! first use.

use serde::{Deserialize, Serialize};

/// Stable, monotonically increasing entity identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EntityId(pub u64);

/// Human-readable entity name. Unique within a world but may be renamed.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EntityName(pub String);

impl EntityName {
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for EntityName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::fmt::Display for EntityId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Cross-entity reference — used in behaviors, parenting, audio attachment.
///
/// LLMs produce `Name` references; these are resolved to `Id` on ingestion.
/// Saved worlds should only contain `Id` references.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum EntityRef {
    /// Stable reference by numeric ID (for persisted worlds).
    Id(EntityId),
    /// Human-readable reference by name (for LLM tool calls).
    Name(String),
}

impl EntityRef {
    pub fn name(name: impl Into<String>) -> Self {
        Self::Name(name.into())
    }

    pub fn id(id: u64) -> Self {
        Self::Id(EntityId(id))
    }
}

/// Unique identifier for a compound creation (group of entities).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CreationId(pub u64);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_ref_name_serializes_as_string() {
        let r = EntityRef::Name("campfire".to_string());
        let json = serde_json::to_string(&r).unwrap();
        assert_eq!(json, r#""campfire""#);
    }

    #[test]
    fn entity_ref_id_serializes_as_object() {
        let r = EntityRef::Id(EntityId(42));
        let json = serde_json::to_string(&r).unwrap();
        assert!(json.contains("42"));
    }

    #[test]
    fn entity_name_display() {
        let name = EntityName::new("sun_lamp");
        assert_eq!(format!("{}", name), "sun_lamp");
    }
}
