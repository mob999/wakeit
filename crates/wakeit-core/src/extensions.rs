//! Compatibility exports for the shared plugin extensions contract.
pub use wakeit_plugin_protocol::extensions::{
    Action, Actor, Binding, ConfigUpdate, Contract, CredentialSlot, Decision, DeliveryOutcome,
    EVENT_TYPES, Event, Extension, ExtensionError, FailurePolicy, HOOK_STAGES, Handler, Hook,
    Instance, Interaction, Invocation, Provider, Receipt, RuntimeInfo, SDK_PROTOCOL, SecretUpdate,
    UiChoice, apply_decision, event_matches, schema, valid_event_pattern, validate_schema,
};
