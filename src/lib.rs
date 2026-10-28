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
//!     ExtensionHandler, ExtensionId, ExtensionRegistration, ToolDefinition, ToolDescriptor,
//!     ToolId, EXTENSIONS,
//! };
//!
//! static TOOLS: &[ToolDescriptor] = &[
//!     ToolDescriptor {
//!         id: ToolId::new("example.echo"),
//!         description: "Echo a JSON value",
//!         input_schema: r#"{"type":"object"}"#,
//!     },
//!     ToolDescriptor {
//!         id: ToolId::new("example.echo-again"),
//!         description: "Echo another JSON value",
//!         input_schema: r#"{"type":"object"}"#,
//!     },
//! ];
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
//!     fn tool_definitions(&self) -> Vec<ToolDefinition> {
//!         // The second descriptor is automatically listed from `TOOLS`.
//!         vec![ToolDefinition {
//!             name: "example.echo".to_owned(),
//!             description: "Echo JSON, with a custom display description".to_owned(),
//!             input_schema: serde_json::json!({"type": "object"}),
//!         }]
//!     }
//!
//!     fn call(
//!         &self,
//!         context: &mut dyn ExtensionContext,
//!         _tool: ToolId,
//!         input: serde_json::Value,
//!     ) -> Result<serde_json::Value, ExtensionError> {
//!         // Attribution is host-supplied, never trusted from tool arguments.
//!         let _identity = context.invocation_identity();
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
//!
//! If a handler does not override `tool_definitions`, the registry builds one
//! definition per descriptor tool from its ID, description, and JSON Schema.
//! Handler-provided definitions may customize those values, but cannot add or
//! duplicate tool IDs; omitted definitions are filled from the descriptor.

pub use linkme::distributed_slice;
use serde_json::Value;
use std::collections::{BTreeSet, HashMap};
use std::error::Error;
use std::fmt;
use std::sync::Mutex;

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

/// Stable identifier for one HTTP route exposed by an extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct RouteId(&'static str);

impl RouteId {
    pub const fn new(value: &'static str) -> Self {
        Self(value)
    }

    pub const fn as_str(self) -> &'static str {
        self.0
    }
}

impl fmt::Display for RouteId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

/// HTTP methods understood by the host-neutral extension route contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Head,
    Options,
}

impl HttpMethod {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Head => "HEAD",
            Self::Options => "OPTIONS",
        }
    }
}

/// Static route registration contributed by an extension.
///
/// `path` uses literal segments and `{parameter}` segments, for example
/// `/api/items/{item_id}`. Matching is case-sensitive. Routes with the same
/// method may overlap only when one is strictly more specific; the host always
/// prefers the route with more literal path segments.
#[derive(Debug, Clone, Copy)]
pub struct ExtensionRouteDescriptor {
    pub id: RouteId,
    pub method: HttpMethod,
    pub path: &'static str,
    /// Mutating routes run under the host's serialized owner/workspace path.
    pub mutates: bool,
}

/// Generic request passed to an extension HTTP route handler.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionHttpRequest {
    pub path_params: std::collections::BTreeMap<String, String>,
    /// Query keys can occur more than once; values retain their received order.
    pub query: std::collections::BTreeMap<String, Vec<String>>,
    /// Empty or absent request bodies are represented as JSON `null`.
    pub body: Value,
}

/// JSON response returned by an extension HTTP route handler.
#[derive(Debug, Clone, PartialEq)]
pub struct ExtensionHttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Value,
}

impl ExtensionHttpResponse {
    pub fn json(status: u16, body: Value) -> Self {
        Self {
            status,
            headers: Vec::new(),
            body,
        }
    }
}

/// Successful route match, including parameters extracted from the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionRouteMatch {
    route_id: RouteId,
    extension_id: ExtensionId,
    method: HttpMethod,
    mutates: bool,
    path_params: std::collections::BTreeMap<String, String>,
}

impl ExtensionRouteMatch {
    pub const fn route_id(&self) -> RouteId {
        self.route_id
    }

    pub const fn extension_id(&self) -> ExtensionId {
        self.extension_id
    }

    pub const fn method(&self) -> HttpMethod {
        self.method
    }

    pub const fn mutates(&self) -> bool {
        self.mutates
    }

    pub fn path_params(&self) -> &std::collections::BTreeMap<String, String> {
        &self.path_params
    }
}

/// Static schema and description presented by a host transport for one tool.
#[derive(Debug, Clone, Copy)]
pub struct ToolDescriptor {
    pub id: ToolId,
    pub description: &'static str,
    /// JSON Schema encoded as JSON text. The registry requires a JSON object.
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

/// Generic selectors accepted by a host's document search implementation.
///
/// `kind` is an exact match, `id_prefix` is a prefix match, and `text` asks
/// the host to search the document ID, kind, and serialized value. The host
/// may use an index, but it must only return documents visible in the scope
/// bound to the current operation. Results should be deterministic for a
/// fixed store state and query. `limit` is a maximum result count; zero returns
/// no results.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DocumentQuery {
    pub kind: Option<String>,
    pub id_prefix: Option<String>,
    pub text: Option<String>,
    pub limit: usize,
}

/// Host-attributed identity for the caller that initiated one extension call.
///
/// Values are supplied by the host from its trusted transport or adapter
/// context. Extensions must not derive or override them from tool arguments.
/// They describe attribution only; they are not an authorization decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvocationIdentity<'a> {
    /// Provider or adapter identity, such as `codex`, `claude`, or `unknown`.
    pub provider: &'a str,
    /// Optional trusted agent or principal identifier.
    pub principal: Option<&'a str>,
}

impl InvocationIdentity<'_> {
    pub const UNKNOWN: Self = Self {
        provider: "unknown",
        principal: None,
    };
}

/// Host-provided document access bound to one owner/workspace operation.
///
/// Atomic batches prevent partial writes, but do not provide compare-and-swap
/// or protect a read-modify-write sequence from concurrent invocations. Hosts
/// must serialize mutating tool calls for the same owner/workspace across the
/// full invocation, including nested capability calls. This execution policy
/// prevents races in read-modify-write operations such as sequence allocation,
/// revision checks, and writing a projection with its event. Membrick currently
/// provides this through its staged writer and service locks.
pub trait ScopedDocumentStore {
    fn get(&self, id: &str) -> Result<Option<ExtensionDocument>, ExtensionError>;
    fn list(&self, kind: Option<&str>) -> Result<Vec<ExtensionDocument>, ExtensionError>;
    /// Search the documents visible in this operation's bound scope.
    ///
    /// The default keeps existing adapters source-compatible while failing
    /// explicitly if they have not implemented search. Hosts should override
    /// it before exposing search to extensions.
    fn search(&self, _query: &DocumentQuery) -> Result<Vec<ExtensionDocument>, ExtensionError> {
        Err(ExtensionError::unsupported(
            "scoped document search is not implemented by this host",
        ))
    }
    /// Atomically upsert all documents in one bound owner/workspace scope.
    ///
    /// Success means every document was committed. An error means none were
    /// committed. Duplicate document IDs in one batch must be rejected. A
    /// host that cannot guarantee all-or-none behavior must return an
    /// `Unsupported` error instead of performing a partial write. Atomicity
    /// does not provide compare-and-swap semantics; see the host serialization
    /// requirement on this trait.
    fn put_batch(&mut self, documents: Vec<ExtensionDocument>) -> Result<(), ExtensionError>;
}

/// Per-operation services supplied by the host.
///
/// `invoke` resolves an operation on a provided capability. Hosts should only
/// make declared, registered capabilities available and must keep all storage
/// scoped to this context's owner/workspace. For mutating tool calls, the host
/// must serialize the full invocation per owner/workspace, including nested
/// capability calls. This protects read-modify-write flows such as sequence
/// allocation, revision checks, and projection-plus-event writes. The API
/// provides no compare-and-swap operation; locking is the host execution
/// policy.
pub trait ExtensionContext {
    fn owner(&self) -> &str;
    fn workspace(&self) -> &str;
    /// Trusted host attribution for the current invocation.
    ///
    /// The default preserves source compatibility for hosts that do not yet
    /// expose caller identity.
    fn invocation_identity(&self) -> InvocationIdentity<'_> {
        InvocationIdentity::UNKNOWN
    }
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
    /// Initialize this statically registered extension before the host begins
    /// serving requests. The host calls hooks in dependency order and should
    /// call [`Self::shutdown`] in reverse order during orderly shutdown.
    ///
    /// Implementations that need per-host mutable state should use
    /// synchronization internally. Per-instance handler factories are outside
    /// the compiled-in v1 contract. If initialization returns an error, this
    /// handler must clean up any resources it acquired before returning; the
    /// registry only shuts down handlers whose initialization succeeded.
    fn initialize(&self) -> Result<(), ExtensionError> {
        Ok(())
    }

    /// Release resources acquired by [`Self::initialize`].
    fn shutdown(&self) -> Result<(), ExtensionError> {
        Ok(())
    }

    /// Optional custom tool definitions. Every declared tool is still listed:
    /// the registry fills omitted definitions from the static descriptor and
    /// rejects duplicate or undeclared names.
    fn tool_definitions(&self) -> Vec<ToolDefinition> {
        Vec::new()
    }

    /// Whether a tool changes extension state and therefore needs the host's
    /// serialized mutation path. The host holds the owner/workspace scope for
    /// the full invocation, including nested capability calls, so reads and
    /// writes within it cannot race with another mutation in that scope.
    fn tool_is_mutating(&self, _tool: ToolId) -> bool {
        false
    }

    /// Project a successful tool result into an optional opaque host
    /// notification. The host forwards the returned JSON without interpreting
    /// extension-specific fields.
    fn change_notification(&self, _tool: ToolId, _output: &Value) -> Option<Value> {
        None
    }

    /// HTTP routes contributed by this extension. The host owns the HTTP
    /// server, authentication, workspace selection, and serialization policy.
    fn http_routes(&self) -> &'static [ExtensionRouteDescriptor] {
        &[]
    }

    /// Handle one route declared by [`Self::http_routes`].
    fn handle_http(
        &self,
        _context: &mut dyn ExtensionContext,
        route: RouteId,
        _request: ExtensionHttpRequest,
    ) -> Result<ExtensionHttpResponse, ExtensionError> {
        Err(ExtensionError::unsupported(format!(
            "extension does not implement HTTP route `{route}`"
        )))
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

/// A validated set of linked extensions and generic tool/HTTP routes.
/// Validated extension set for one host instance.
///
/// The host must call [`Self::initialize`] successfully before dispatching
/// extension tools, and stop dispatching before calling [`Self::shutdown`].
/// The registry records lifecycle state but does not gate dispatch, preserving
/// compatibility for existing hosts.
pub struct ExtensionRegistry {
    ordered: Vec<&'static ExtensionRegistration>,
    tools: HashMap<ToolId, &'static ExtensionRegistration>,
    capabilities: HashMap<CapabilityId, &'static ExtensionRegistration>,
    tool_definitions: Vec<ToolDefinition>,
    routes: Vec<RegisteredRoute>,
    routes_by_id: HashMap<RouteId, usize>,
    lifecycle: Mutex<LifecycleState>,
}

struct RegisteredRoute {
    descriptor: &'static ExtensionRouteDescriptor,
    registration: &'static ExtensionRegistration,
    segments: Vec<RouteSegment>,
}

#[derive(Clone, Copy)]
enum RouteSegment {
    Literal(&'static str),
    Parameter(&'static str),
}

fn parse_route_pattern(path: &'static str) -> Result<Vec<RouteSegment>, String> {
    if path == "/" {
        return Ok(Vec::new());
    }
    if !path.starts_with('/') || path.ends_with('/') || path.contains(['?', '#']) {
        return Err(format!("invalid route path pattern `{path}`"));
    }
    let mut parameter_names = BTreeSet::new();
    path[1..]
        .split('/')
        .map(|segment| {
            if segment.is_empty() || segment == "." || segment == ".." {
                return Err(format!("invalid route path pattern `{path}`"));
            }
            if let Some(parameter) = segment
                .strip_prefix('{')
                .and_then(|value| value.strip_suffix('}'))
            {
                let mut chars = parameter.chars();
                let valid_first = chars
                    .next()
                    .is_some_and(|character| character == '_' || character.is_ascii_alphabetic());
                if !valid_first
                    || !chars.all(|character| character == '_' || character.is_ascii_alphanumeric())
                {
                    return Err(format!("invalid route parameter in path pattern `{path}`"));
                }
                if !parameter_names.insert(parameter) {
                    return Err(format!(
                        "duplicate route parameter in path pattern `{path}`"
                    ));
                }
                Ok(RouteSegment::Parameter(parameter))
            } else if segment.contains(['{', '}']) {
                Err(format!("invalid route parameter in path pattern `{path}`"))
            } else {
                Ok(RouteSegment::Literal(segment))
            }
        })
        .collect()
}

fn route_patterns_conflict(left: &[RouteSegment], right: &[RouteSegment]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .filter(|segment| matches!(segment, RouteSegment::Literal(_)))
            .count()
            == right
                .iter()
                .filter(|segment| matches!(segment, RouteSegment::Literal(_)))
                .count()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| match (left, right) {
                (RouteSegment::Literal(left), RouteSegment::Literal(right)) => left == right,
                _ => true,
            })
}

fn split_route_path(path: &str) -> Option<Vec<&str>> {
    if path == "/" {
        return Some(Vec::new());
    }
    if !path.starts_with('/') || path.ends_with('/') || path.contains(['?', '#']) {
        return None;
    }
    let segments = path[1..].split('/').collect::<Vec<_>>();
    (!segments.iter().any(|segment| segment.is_empty())).then_some(segments)
}

fn decode_route_parameter(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = *bytes.get(index + 1)?;
            let low = *bytes.get(index + 2)?;
            let hex = |byte: u8| match byte {
                b'0'..=b'9' => Some(byte - b'0'),
                b'a'..=b'f' => Some(byte - b'a' + 10),
                b'A'..=b'F' => Some(byte - b'A' + 10),
                _ => None,
            };
            decoded.push((hex(high)? << 4) | hex(low)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

enum LifecycleState {
    NotStarted,
    Starting,
    Started,
    Failed(ExtensionError),
    Stopping,
    Stopped(Result<(), ExtensionError>),
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
        let mut route_ids = BTreeSet::new();
        let mut routes = Vec::<RegisteredRoute>::new();
        let mut definitions_by_extension = HashMap::<ExtensionId, Vec<ToolDefinition>>::new();

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
                let schema = serde_json::from_str::<Value>(tool.input_schema).map_err(|_| {
                    RegistryError::new(format!(
                        "tool `{}` in extension `{}` has invalid JSON Schema JSON",
                        tool.id.as_str(),
                        descriptor.id
                    ))
                })?;
                if !schema.is_object() {
                    return Err(RegistryError::new(format!(
                        "tool `{}` in extension `{}` has a JSON Schema that is not an object",
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

            for route in registration.handler.http_routes() {
                if route.id.as_str().trim().is_empty() {
                    return Err(RegistryError::new(format!(
                        "extension `{}` declares an empty HTTP route ID",
                        descriptor.id
                    )));
                }
                if !route_ids.insert(route.id) {
                    return Err(RegistryError::new(format!(
                        "HTTP route ID `{}` is registered more than once",
                        route.id
                    )));
                }
                let segments = parse_route_pattern(route.path).map_err(RegistryError::new)?;
                if let Some(existing) = routes.iter().find(|existing| {
                    existing.descriptor.method == route.method
                        && route_patterns_conflict(&existing.segments, &segments)
                }) {
                    return Err(RegistryError::new(format!(
                        "HTTP routes `{}` (`{} {}`) and `{}` (`{} {}`) overlap",
                        existing.descriptor.id,
                        existing.descriptor.method.as_str(),
                        existing.descriptor.path,
                        route.id,
                        route.method.as_str(),
                        route.path
                    )));
                }
                routes.push(RegisteredRoute {
                    descriptor: route,
                    registration: *registration,
                    segments,
                });
            }

            let declared_tool_ids = descriptor
                .tools
                .iter()
                .map(|tool| tool.id.as_str())
                .collect::<BTreeSet<_>>();
            let mut custom_definitions = HashMap::new();
            for definition in registration.handler.tool_definitions().into_iter() {
                if !definition.input_schema.is_object() {
                    return Err(RegistryError::new(format!(
                        "extension `{}` defines tool `{}` with a JSON Schema that is not an object",
                        descriptor.id, definition.name
                    )));
                }
                if !declared_tool_ids.contains(definition.name.as_str()) {
                    return Err(RegistryError::new(format!(
                        "extension `{}` exposes tool `{}` absent from its static descriptor",
                        descriptor.id, definition.name
                    )));
                }
                let name = definition.name.clone();
                if custom_definitions
                    .insert(name.clone(), definition)
                    .is_some()
                {
                    return Err(RegistryError::new(format!(
                        "extension `{}` defines tool `{name}` more than once",
                        descriptor.id
                    )));
                }
            }

            let definitions = descriptor
                .tools
                .iter()
                .map(|tool| {
                    custom_definitions
                        .remove(tool.id.as_str())
                        .unwrap_or_else(|| ToolDefinition {
                            name: tool.id.as_str().to_owned(),
                            description: tool.description.to_owned(),
                            input_schema: serde_json::from_str(tool.input_schema)
                                .expect("tool schemas are validated above"),
                        })
                })
                .collect();
            definitions_by_extension.insert(descriptor.id, definitions);
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

        let tool_definitions = sorted
            .iter()
            .flat_map(|registration| {
                definitions_by_extension
                    .remove(&registration.descriptor.id)
                    .unwrap_or_default()
            })
            .collect();

        routes.sort_by_key(|route| {
            std::cmp::Reverse(
                route
                    .segments
                    .iter()
                    .filter(|segment| matches!(segment, RouteSegment::Literal(_)))
                    .count(),
            )
        });
        let routes_by_id = routes
            .iter()
            .enumerate()
            .map(|(index, route)| (route.descriptor.id, index))
            .collect();

        Ok(Self {
            ordered: sorted,
            tools,
            capabilities,
            tool_definitions,
            routes,
            routes_by_id,
            lifecycle: Mutex::new(LifecycleState::NotStarted),
        })
    }

    /// Extensions in deterministic dependency order.
    pub fn extensions(&self) -> impl Iterator<Item = &'static ExtensionRegistration> + '_ {
        self.ordered.iter().copied()
    }

    /// Capabilities declared by registered extensions, in deterministic
    /// extension and descriptor order.
    pub fn capabilities(&self) -> impl Iterator<Item = CapabilityId> + '_ {
        self.ordered
            .iter()
            .flat_map(|registration| registration.descriptor.provides.iter().copied())
    }

    /// Initialize every extension exactly once for this registry instance, in
    /// dependency order.
    ///
    /// Repeated calls after success return `Ok(())` without rerunning hooks.
    /// If an initializer fails, already initialized extensions are shut down
    /// once in reverse order; the failure is recorded and returned again on
    /// later calls. The failing initializer must clean up its own partial
    /// resources. An initialization failure is terminal for this registry.
    pub fn initialize(&self) -> Result<(), ExtensionError> {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &*lifecycle {
            LifecycleState::NotStarted => *lifecycle = LifecycleState::Starting,
            LifecycleState::Started => return Ok(()),
            LifecycleState::Failed(error) => return Err(error.clone()),
            LifecycleState::Stopped(_) => {
                return Err(ExtensionError::with_kind(
                    ExtensionErrorKind::Conflict,
                    "extension registry has already been shut down",
                ));
            }
            LifecycleState::Starting | LifecycleState::Stopping => {
                return Err(ExtensionError::with_kind(
                    ExtensionErrorKind::Conflict,
                    "extension registry lifecycle hook is already in progress",
                ));
            }
        }

        let mut initialized: Vec<&'static ExtensionRegistration> = Vec::new();
        for registration in &self.ordered {
            if let Err(error) = registration.handler.initialize() {
                for initialized_registration in initialized.into_iter().rev() {
                    let _ = initialized_registration.handler.shutdown();
                }
                *lifecycle = LifecycleState::Failed(error.clone());
                return Err(error);
            }
            initialized.push(*registration);
        }
        *lifecycle = LifecycleState::Started;
        Ok(())
    }

    /// Shut down initialized extensions exactly once for this registry
    /// instance, in reverse dependency order. All hooks are attempted; the first error is recorded and
    /// returned. Repeated calls return the recorded result without rerunning
    /// hooks. Calling this before initialization marks the registry stopped
    /// without invoking any extension hooks. After a failed initialization,
    /// this is a no-op because successful extensions were already rolled back.
    pub fn shutdown(&self) -> Result<(), ExtensionError> {
        let mut lifecycle = self
            .lifecycle
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match &*lifecycle {
            LifecycleState::NotStarted => {
                *lifecycle = LifecycleState::Stopped(Ok(()));
                return Ok(());
            }
            LifecycleState::Failed(_) => return Ok(()),
            LifecycleState::Stopped(result) => return result.clone(),
            LifecycleState::Started => *lifecycle = LifecycleState::Stopping,
            LifecycleState::Starting | LifecycleState::Stopping => {
                return Err(ExtensionError::with_kind(
                    ExtensionErrorKind::Conflict,
                    "extension registry lifecycle hook is already in progress",
                ));
            }
        }

        let mut first_error = None;
        for registration in self.ordered.iter().rev() {
            if let Err(error) = registration.handler.shutdown() {
                if first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
        let result = first_error.map_or(Ok(()), Err);
        *lifecycle = LifecycleState::Stopped(result.clone());
        result
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

    /// Ask the owning extension for an opaque notification after a successful
    /// tool call. Unknown tools and handlers without notifications return None.
    pub fn change_notification(&self, name: &str, output: &Value) -> Option<Value> {
        let (tool, registration) = self.tools.iter().find(|(tool, _)| tool.as_str() == name)?;
        registration.handler.change_notification(*tool, output)
    }

    /// Registered HTTP routes ordered by specificity, preserving registration
    /// order among routes with equal specificity.
    pub fn http_routes(
        &self,
    ) -> impl Iterator<Item = (ExtensionId, &'static ExtensionRouteDescriptor)> + '_ {
        self.routes
            .iter()
            .map(|route| (route.registration.descriptor.id, route.descriptor))
    }

    /// Match a method and path against registered extension routes.
    pub fn match_http_route(&self, method: HttpMethod, path: &str) -> Option<ExtensionRouteMatch> {
        let path_segments = split_route_path(path)?;
        self.routes.iter().find_map(|route| {
            if route.descriptor.method != method || route.segments.len() != path_segments.len() {
                return None;
            }
            let mut path_params = std::collections::BTreeMap::new();
            for (pattern, value) in route.segments.iter().zip(path_segments.iter()) {
                match pattern {
                    RouteSegment::Literal(expected) if expected == value => {}
                    RouteSegment::Parameter(name) => {
                        path_params.insert((*name).to_owned(), decode_route_parameter(value)?);
                    }
                    RouteSegment::Literal(_) => return None,
                }
            }
            Some(ExtensionRouteMatch {
                route_id: route.descriptor.id,
                extension_id: route.registration.descriptor.id,
                method,
                mutates: route.descriptor.mutates,
                path_params,
            })
        })
    }

    /// Dispatch a previously matched HTTP route to its owning extension.
    pub fn call_http_route(
        &self,
        context: &mut dyn ExtensionContext,
        route: &ExtensionRouteMatch,
        mut request: ExtensionHttpRequest,
    ) -> Result<ExtensionHttpResponse, ExtensionError> {
        let index = self.routes_by_id.get(&route.route_id).ok_or_else(|| {
            ExtensionError::new(format!("unknown extension HTTP route `{}`", route.route_id))
        })?;
        let registered = &self.routes[*index];
        if registered.registration.descriptor.id != route.extension_id
            || registered.descriptor.method != route.method
            || registered.descriptor.mutates != route.mutates
        {
            return Err(ExtensionError::with_kind(
                ExtensionErrorKind::InvalidInput,
                "HTTP route match does not belong to this registry",
            ));
        }
        request.path_params = route.path_params.clone();
        registered
            .registration
            .handler
            .handle_http(context, registered.descriptor.id, request)
    }

    /// Complete tool definitions for registered extensions.
    ///
    /// Definitions are returned in dependency order and preserve each
    /// extension's descriptor order. Handler overrides are captured and
    /// validated when the registry is constructed; any omitted definitions
    /// are generated from the static descriptors.
    pub fn tool_definitions(&self) -> Vec<ToolDefinition> {
        self.tool_definitions.clone()
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
    kind: ExtensionErrorKind,
    message: String,
}

/// Stable, domain-neutral classification for extension and host failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtensionErrorKind {
    InvalidInput,
    NotFound,
    Conflict,
    Forbidden,
    Unavailable,
    Unsupported,
    Internal,
}

impl ExtensionError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            kind: ExtensionErrorKind::Internal,
            message: message.into(),
        }
    }

    pub fn with_kind(kind: ExtensionErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::with_kind(ExtensionErrorKind::Unsupported, message)
    }

    pub const fn kind(&self) -> ExtensionErrorKind {
        self.kind
    }

    pub fn message(&self) -> &str {
        &self.message
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
