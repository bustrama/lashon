//! A generic, per-stage registry of providers. Each stage (STT, LLM, future
//! TTS) owns one `ProviderRegistry<T>` where `T` is the trait object for
//! that stage (`dyn SttProvider`, `dyn LLMProvider`, …).
//!
//! The registry is intentionally minimal: a `HashMap<id, Arc<T>>` plus the
//! id of the active provider. Lookups clone the `Arc` so the active provider
//! is shared between concurrent callers without holding the registry lock
//! beyond the read (docs/adr/0019).
//!
//! Construction is lazy in the Tauri shell: cloud providers are added to the
//! registry up front but their HTTP clients are cheap to build and carry no
//! per-call state. The first call that needs an API key fetches it from the
//! keychain — a missing key surfaces as `ProviderError::KeyNotFound` at call
//! time, not at startup, so the app starts even if the user has not entered
//! any cloud credentials (docs/adr/0020, docs/adr/0022 Invariant 5).

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{anyhow, Result};

/// A registry of providers keyed by their stable id. Generic over the
/// stage trait so the same struct serves STT, LLM, and future TTS without
/// the type-system collapsing on a single discriminant.
pub struct ProviderRegistry<T: ?Sized + Send + Sync> {
    providers: HashMap<String, Arc<T>>,
    active_id: String,
}

impl<T: ?Sized + Send + Sync> ProviderRegistry<T> {
    /// Create an empty registry whose active id is set up-front. The id need
    /// not be registered yet — providers can be added after construction.
    pub fn new(default_id: impl Into<String>) -> Self {
        Self {
            providers: HashMap::new(),
            active_id: default_id.into(),
        }
    }

    /// Register a provider. Overwrites any previous registration for the
    /// same id (intentionally — `set_llm_provider` re-installs cloud
    /// providers after a key save).
    pub fn register(&mut self, id: impl Into<String>, provider: Arc<T>) {
        self.providers.insert(id.into(), provider);
    }

    /// The currently-active provider, if its id has been registered.
    /// `None` means the active id refers to an unregistered provider —
    /// callers raise a typed error from there.
    pub fn active(&self) -> Option<Arc<T>> {
        self.providers.get(&self.active_id).cloned()
    }

    /// Look up a provider by id without changing the active selection.
    /// Used by the Hub's per-mode "test prompt" — the chat-mode provider
    /// must be invocable without flipping the dictation-time provider.
    pub fn get(&self, id: &str) -> Option<Arc<T>> {
        self.providers.get(id).cloned()
    }

    /// Switch the active provider. Errors if the id is not registered;
    /// callers surface that to the Hub as a localised error toast.
    pub fn set_active(&mut self, id: &str) -> Result<()> {
        if !self.providers.contains_key(id) {
            return Err(anyhow!("provider id not registered: {id}"));
        }
        self.active_id = id.to_string();
        Ok(())
    }

    /// Force-set the active id even if it has not been registered. Used at
    /// startup to honour `settings.json` for providers whose construction
    /// is deferred (cloud providers without a stored key).
    pub fn set_active_unchecked(&mut self, id: impl Into<String>) {
        self.active_id = id.into();
    }

    /// The active id, even if it does not resolve to a registered provider.
    pub fn active_id(&self) -> &str {
        &self.active_id
    }

    /// Whether `id` resolves to a registered provider.
    pub fn contains(&self, id: &str) -> bool {
        self.providers.contains_key(id)
    }

    /// All registered ids, sorted alphabetically. The Hub presents the chip
    /// grid in this order; callers that want a custom ordering filter and
    /// re-sort externally.
    pub fn ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.providers.keys().cloned().collect();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::test_support::MockLlmProvider;
    use crate::llm::LLMProvider;

    fn make_registry() -> ProviderRegistry<dyn LLMProvider> {
        let mut registry: ProviderRegistry<dyn LLMProvider> = ProviderRegistry::new("none");
        let local = MockLlmProvider::hebrew_excellent_local("שלום מקומי");
        let cloud = {
            let mut m = MockLlmProvider::hebrew_excellent_local("שלום ענן");
            m.name = "cloud";
            m.display_name_key = "provider.llm.cloud";
            m.local = false;
            m
        };
        registry.register("local", Arc::new(local) as Arc<dyn LLMProvider>);
        registry.register("cloud", Arc::new(cloud) as Arc<dyn LLMProvider>);
        registry
    }

    #[test]
    fn empty_registry_has_no_active_provider() {
        let registry: ProviderRegistry<dyn LLMProvider> = ProviderRegistry::new("none");
        assert!(registry.active().is_none());
        assert_eq!(registry.active_id(), "none");
    }

    #[test]
    fn registering_a_provider_makes_it_lookupable() {
        let registry = make_registry();
        assert!(registry.get("local").is_some());
        assert!(registry.get("cloud").is_some());
        assert!(registry.get("missing").is_none());
    }

    #[test]
    fn set_active_to_registered_provider_succeeds() {
        let mut registry = make_registry();
        registry.set_active("cloud").unwrap();
        assert_eq!(registry.active_id(), "cloud");
        let active = registry.active().expect("cloud must resolve");
        assert!(!active.is_local());
    }

    #[test]
    fn set_active_to_unknown_provider_errors() {
        let mut registry = make_registry();
        let err = registry.set_active("missing").unwrap_err();
        let rendered = format!("{err}");
        assert!(rendered.contains("missing"));
    }

    #[test]
    fn set_active_unchecked_does_not_validate() {
        let mut registry = make_registry();
        registry.set_active_unchecked("not-registered");
        assert_eq!(registry.active_id(), "not-registered");
        // The lookup returns None — callers surface that as the typed error.
        assert!(registry.active().is_none());
    }

    #[test]
    fn ids_are_sorted_alphabetically() {
        let registry = make_registry();
        assert_eq!(
            registry.ids(),
            vec!["cloud".to_string(), "local".to_string()]
        );
    }
}
