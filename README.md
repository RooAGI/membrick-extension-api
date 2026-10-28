# membrick-extension-api

The contract between a Membrick host and its compiled-in extensions.

An extension crate depends on this crate, implements `ExtensionHandler`, and
registers itself in the `EXTENSIONS` link-time slice. A host such as
[Membrick](https://github.com/RooAGI/Membrick) depends on the same version,
links the extension crates in, and drives them through `ExtensionRegistry`.
The host supplies scoped document storage, owner/workspace scope, caller
identity, and HTTP and MCP transport. The extension owns its domain behavior.
[Flowboard](https://github.com/RooAGI/flowboard) is an example.

## Use

```toml
[dependencies]
membrick-extension-api = { git = "https://github.com/RooAGI/membrick-extension-api.git", tag = "v0.2.0" }
```

Host and extensions must resolve to the same version of this crate.

## Versioning

Releases are tagged `vMAJOR.MINOR.PATCH`. Before 1.0, a minor release may
change the API. Adding an optional trait method with a default implementation
is a minor release that existing extensions build against unchanged.
