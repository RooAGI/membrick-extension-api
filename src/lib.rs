//! Generic contract for compiled-in Membrick extensions.
//!
//! An extension crate describes the capabilities it provides and requires,
//! declares the tools it exposes, implements [`ExtensionHandler`], and submits
//! one [`ExtensionRegistration`] to [`EXTENSIONS`]. The final binary gathers
//! registrations from its linked dependency graph at link time. The host then
//! validates the complete set once and dispatches tools by [`ToolId`].
//!
//! This API does not load shared libraries or create a runtime plugin loader.
//! An extension must still be a dependency of the final binary to be linked.
//!
//! # Registration shape
//!
//! ```ignore
//! use membrick_extension_api::{
//!     distributed_slice, CapabilityId, ExtensionContext, ExtensionDescriptor, ExtensionError,
//!     ExtensionHandler, ExtensionId, ExtensionRegistration, ToolDescriptor, ToolId, EXTENSIONS,
//! };
//!
//! static TOOLS: &[ToolDescriptor] = &[ToolDescriptor {
//!     id: ToolId::new("example.echo"),
//!     description: "Echo a JSON value",
//!     input_schema: r#"{"type":"object"}"#,
//! }];
//!
//! static DESCRIPTOR: ExtensionDescriptor = ExtensionDescriptor {
//!     id: ExtensionId::new("example.echo-extension"),
//!     provides: &[CapabilityId::new("example.echo")],
//!     requires: &[],
//!     tools: TOOLS,
//! };
//!
//! struct Echo;
//! impl ExtensionHandler for Echo {
//!     fn call(
//!         &self,
//!         _context: &mut dyn ExtensionContext,
//!         _tool: ToolId,
//!         input: serde_json::Value,
//!     ) -> Result<serde_json::Value, ExtensionError> {
//!         Ok(input)
//!     }
//! }
//! static HANDLER: Echo = Echo;
//!
//! #[distributed_slice(EXTENSIONS)]
//! static REGISTRATION: ExtensionRegistration = ExtensionRegistration {
//!     descriptor: &DESCRIPTOR,
//!     handler: &HANDLER,
//! };
//! ```

pub use linkme::distributed_slice;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::error::Error;
use std::fmt;

/// Provider-neutral MCP tool definition exported by an extension.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Stable identifier for one extension crate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ExtensionId(&'static str);

impl ExtensionId {
    /// Declare an extension identifier in a static descriptor.
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ExtensionId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// Stable identifier for a capability supplied by an extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CapabilityId(&'static str);

impl CapabilityId {
    /// Declare a capability identifier in a static extension descriptor.
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for CapabilityId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// Stable identifier for a tool exposed through a host transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ToolId(&'static str);

impl ToolId {
    /// Declare a tool identifier in a static extension descriptor.
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ToolId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// Static schema and description presented by a host transport for one tool.
#[derive(Debug, Clone, Copy)]
pub struct ToolDescriptor {
    pub id: ToolId,
    pub description: &'static str,
    /// JSON Schema encoded as JSON text. The registry checks that it parses.
    pub input_schema: &'static str,
}

/// Static identity and dependency contract for an extension.
#[derive(Debug, Clone, Copy)]
pub struct ExtensionDescriptor {
    pub id: ExtensionId,
    pub provides: &'static [CapabilityId],
    pub requires: &'static [CapabilityId],
    pub tools: &'static [ToolDescriptor],
}

/// Generic record passed through a host context's scoped document store.
///
/// The host binds owner/workspace scope outside this value and must enforce
/// it on every read and write. Extensions cannot choose or widen that scope.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionDocument {
    pub id: String,
    pub kind: String,
    pub value: Value,
}

/// Host-provided document access bound to one owner/workspace operation.
pub trait ScopedDocumentStore {
    fn get(&self, id: &str) -> Result<Option<ExtensionDocument>, ExtensionError>;
    fn list(&self, kind: Option<&str>) -> Result<Vec<ExtensionDocument>, ExtensionError>;
    /// Store related records atomically when the host supports batch writes.
    fn put_batch(&mut self, documents: Vec<ExtensionDocument>) -> Result<(), ExtensionError>;
}

/// Per-operation services supplied by the host.
///
/// `invoke` resolves an operation on a provided capability. Hosts should only
/// make declared, registered capabilities available and must keep all storage
/// scoped to this context's owner/workspace.
pub trait ExtensionContext {
    fn owner(&self) -> &str;
    fn workspace(&self) -> &str;
    fn documents(&mut self) -> &mut dyn ScopedDocumentStore;
    fn invoke(
        &mut self,
        capability: CapabilityId,
        operation: &str,
        input: Value,
    ) -> Result<Value, ExtensionError>;
}

/// One extension's implementation for all tools in its descriptor.
pub trait ExtensionHandler: Sync {
    /// Provider-neutral MCP tool definitions. The registry checks that these
    /// names match the statically declared tool IDs.
    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        Vec::new()
    }

    /// Whether a tool changes extension state and therefore needs the host's
    /// serialized mutation path.
    fn tool_is_mutating(&self, _tool: ToolId) -> bool {
        false
    }

    fn call(
        &self,
        context: &mut dyn ExtensionContext,
        tool: ToolId,
        input: Value,
    ) -> Result<Value, ExtensionError>;

    /// Handle an operation requested through one of this extension's
    /// capabilities. Extensions that expose no cross-extension operations can
    /// keep the default implementation.
    fn invoke(
        &self,
        _context: &mut dyn ExtensionContext,
        capability: CapabilityId,
        operation: &str,
        _input: Value,
    ) -> Result<Value, ExtensionError> {
        Err(ExtensionError::new(format!(
            "extension does not implement operation `{operation}` for capability `{}`",
            capability.as_str()
        )))
    }
}

/// Link-time registration of a descriptor and its stateless handler.
#[derive(Clone, Copy)]
pub struct ExtensionRegistration {
    pub descriptor: &'static ExtensionDescriptor,
    pub handler: &'static dyn ExtensionHandler,
}

/// Extensions contribute entries from any linked crate with `linkme`:
/// `#[distributed_slice(membrick_extension_api::EXTENSIONS)]`.
#[distributed_slice]
pub static EXTENSIONS: [ExtensionRegistration];

/// A validated set of linked extensions and generic tool routes.
pub struct ExtensionRegistry {
    ordered: Vec<&'static ExtensionRegistration>,
    tools: HashMap<ToolId, &'static ExtensionRegistration>,
    capabilities: HashMap<CapabilityId, &'static ExtensionRegistration>,
}

impl ExtensionRegistry {
    /// Collect and validate registrations contributed by linked crates.
    pub fn from_linked() -> Result<Self, RegistryError> {
        Self::new(EXTENSIONS.iter().collect())
    }

    /// Validate a set of registrations and sort it in dependency order.
    ///
    /// This constructor also allows a host to supply an explicit set, for
    /// example in an embedded application that does not use link-time
    /// collection.
    pub fn new(registrations: Vec<&'static ExtensionRegistration>) -> Result<Self, RegistryError> {
        let mut extension_ids = BTreeSet::new();
        let mut capabilities = HashMap::new();
        let mut tools = HashMap::new();

        for registration in &registrations {
            let descriptor = registration.descriptor;
            if descriptor.id.as_str().trim().is_empty() {
                return Err(RegistryError::new("extension ID must not be empty"));
            }
            if !extension_ids.insert(descriptor.id) {
                return Err(RegistryError::new(format!(
                    "extension ID `{}` is registered more than once",
                    descriptor.id.as_str()
                )));
            }
            if descriptor.provides.is_empty() {
                return Err(RegistryError::new(format!(
                    "extension `{}` does not provide a capability",
                    descriptor.id.as_str()
                )));
            }

            let mut own_capabilities = BTreeSet::new();
            for capability in descriptor.provides {
                if capability.as_str().trim().is_empty() {
                    return Err(RegistryError::new(format!(
                        "extension `{}` declares an empty capability ID",
                        descriptor.id
                    )));
                }
                if !own_capabilities.insert(*capability) {
                    return Err(RegistryError::new(format!(
                        "extension `{}` declares capability `{}` more than once",
                        descriptor.id.as_str(),
                        capability.as_str()
                    )));
                }
                if capabilities.insert(*capability, *registration).is_some() {
                    return Err(RegistryError::new(format!(
                        "capability `{}` has more than one provider",
                        capability.as_str()
                    )));
                }
            }

            let mut own_tools = BTreeSet::new();
            for tool in descriptor.tools {
                if tool.id.as_str().trim().is_empty() {
                    return Err(RegistryError::new(format!(
                        "extension `{}` declares an empty tool ID",
                        descriptor.id.as_str()
                    )));
                }
                if !own_tools.insert(tool.id) {
                    return Err(RegistryError::new(format!(
                        "extension `{}` declares tool `{}` more than once",
                        descriptor.id.as_str(),
                        tool.id.as_str()
                    )));
                }
                if serde_json::from_str::<Value>(tool.input_schema).is_err() {
                    return Err(RegistryError::new(format!(
                        "tool `{}` in extension `{}` has invalid JSON Schema JSON",
                        tool.id.as_str(),
                        descriptor.id
                    )));
                }
                if tools.insert(tool.id, *registration).is_some() {
                    return Err(RegistryError::new(format!(
                        "tool `{}` is registered more than once",
                        tool.id.as_str()
                    )));
                }
            }

            let declared_tool_ids = descriptor
                .tools
                .iter()
                .map(|tool| tool.id.as_str())
                .collect::<BTreeSet<_>>();
            let handler_tool_ids = registration
                .handler
                .tool_definitions()
                .into_iter()
                .map(|tool| tool.name)
                .collect::<BTreeSet<_>>();
            if !handler_tool_ids.is_empty()
                && handler_tool_ids
                    .iter()
                    .any(|tool| !declared_tool_ids.contains(tool.as_str()))
            {
                return Err(RegistryError::new(format!(
                    "extension `{}` exposes a tool that is absent from its static descriptor",
                    descriptor.id
                )));
            }
        }

        let mut dependencies = vec![BTreeSet::<usize>::new(); registrations.len()];
        let indexes_by_id: HashMap<_, _> = registrations
            .iter()
            .enumerate()
            .map(|(index, registration)| (registration.descriptor.id, index))
            .collect();
        let mut dependents = vec![Vec::<usize>::new(); registrations.len()];

        for (index, registration) in registrations.iter().enumerate() {
            let descriptor = registration.descriptor;
            let mut seen_requirements = BTreeSet::new();
            for required in descriptor.requires {
                if !seen_requirements.insert(*required) {
                    return Err(RegistryError::new(format!(
                        "extension `{}` repeats required capability `{}`",
                        descriptor.id,
                        required.as_str()
                    )));
                }
                let provider = capabilities.get(required).ok_or_else(|| {
                    RegistryError::new(format!(
                        "extension `{}` requires missing capability `{}`",
                        descriptor.id,
                        required.as_str()
                    ))
                })?;
                let provider_index = indexes_by_id[&provider.descriptor.id];
                if provider_index != index {
                    dependencies[index].insert(provider_index);
                    dependents[provider_index].push(index);
                } else {
                    return Err(RegistryError::new(format!(
                        "extension `{}` requires its own capability `{}`",
                        descriptor.id,
                        required.as_str()
                    )));
                }
            }
        }

        // Kahn topological sort with registration order as the stable tie
        // breaker. A remaining vertex indicates a dependency cycle.
        let mut ready = (0..registrations.len())
            .filter(|index| dependencies[*index].is_empty())
            .collect::<Vec<_>>();
        let mut sorted = Vec::with_capacity(registrations.len());
        while !ready.is_empty() {
            let index = ready.remove(0);
            sorted.push(registrations[index]);
            for dependent in &dependents[index] {
                dependencies[*dependent].remove(&index);
                if dependencies[*dependent].is_empty()
                    && !sorted
                        .iter()
                        .any(|entry| entry.descriptor.id == registrations[*dependent].descriptor.id)
                    && !ready.contains(dependent)
                {
                    ready.push(*dependent);
                }
            }
            ready.sort_by_key(|candidate| *candidate);
        }
        if sorted.len() != registrations.len() {
            let cycle = registrations
                .iter()
                .enumerate()
                .filter(|(index, _)| !dependencies[*index].is_empty())
                .map(|(_, registration)| registration.descriptor.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(RegistryError::new(format!(
                "extension dependency cycle detected among: {cycle}"
            )));
        }

        Ok(Self {
            ordered: sorted,
            tools,
            capabilities,
        })
    }

    /// Extensions in deterministic dependency order.
    pub fn extensions(&self) -> impl Iterator<Item = &'static ExtensionRegistration> + '_ {
        self.ordered.iter().copied()
    }

    /// Return the provider for a registered capability.
    pub fn capability_provider(&self, id: CapabilityId) -> Option<&'static ExtensionRegistration> {
        self.capabilities.get(&id).copied()
    }

    /// Whether a tool name is registered by any linked extension.
    pub fn has_tool(&self, id: ToolId) -> bool {
        self.tools.contains_key(&id)
    }

    /// Whether a tool name is registered. Use this for names received at
    /// runtime; [`ToolId::new`] is intended for static extension declarations.
    pub fn has_tool_name(&self, name: &str) -> bool {
        self.tools.keys().any(|tool| tool.as_str() == name)
    }

    /// Extension that declared a tool.
    pub fn tool_provider(&self, id: ToolId) -> Option<ExtensionId> {
        self.tools
            .get(&id)
            .map(|registration| registration.descriptor.id)
    }

    /// Extension that declared a runtime tool name.
    pub fn tool_provider_name(&self, name: &str) -> Option<ExtensionId> {
        self.tools
            .iter()
            .find(|(tool, _)| tool.as_str() == name)
            .map(|(_, registration)| registration.descriptor.id)
    }

    /// Mutation classification supplied by the owning extension.
    pub fn tool_is_mutating_name(&self, name: &str) -> Option<bool> {
        let (tool, registration) = self.tools.iter().find(|(tool, _)| tool.as_str() == name)?;
        Some(registration.handler.tool_is_mutating(*tool))
    }

    /// Tool definitions contributed by registered extension handlers.
    /// Definitions are returned in dependency order and preserve each
    /// extension's declaration order.
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.ordered
            .iter()
            .flat_map(|registration| registration.handler.tool_definitions())
            .collect()
    }

    /// Tool descriptors in deterministic extension and declaration order.
    pub fn tools(&self) -> impl Iterator<Item = &'static ToolDescriptor> + '_ {
        self.ordered
            .iter()
            .flat_map(|registration| registration.descriptor.tools.iter())
    }

    /// Dispatch a generic tool request to the extension that declared it.
    pub fn call(
        &self,
        context: &mut dyn ExtensionContext,
        tool: ToolId,
        input: Value,
    ) -> Result<Value, ExtensionError> {
        let registration = self.tools.get(&tool).ok_or_else(|| {
            ExtensionError::new(format!("unknown extension tool `{}`", tool.as_str()))
        })?;
        registration.handler.call(context, tool, input)
    }

    /// Dispatch a tool name received from a transport.
    pub fn call_named(
        &self,
        context: &mut dyn ExtensionContext,
        name: &str,
        input: Value,
    ) -> Result<Value, ExtensionError> {
        let (tool, registration) = self
            .tools
            .iter()
            .find(|(tool, _)| tool.as_str() == name)
            .ok_or_else(|| ExtensionError::new(format!("unknown extension tool `{name}`")))?;
        registration.handler.call(context, *tool, input)
    }

    /// Invoke an operation through the provider of a named capability.
    pub fn invoke(
        &self,
        context: &mut dyn ExtensionContext,
        capability: CapabilityId,
        operation: &str,
        input: Value,
    ) -> Result<Value, ExtensionError> {
        let registration = self.capabilities.get(&capability).ok_or_else(|| {
            ExtensionError::new(format!(
                "unknown extension capability `{}`",
                capability.as_str()
            ))
        })?;
        registration
            .handler
            .invoke(context, capability, operation, input)
    }
}

/// Error returned by an extension handler or host context.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionError {
    message: String,
}

impl ExtensionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ExtensionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ExtensionError {}

/// Error found while building the generic extension registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistryError {
    message: String,
}

impl RegistryError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for RegistryError {}
