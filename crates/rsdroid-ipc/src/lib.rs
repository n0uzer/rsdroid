//! rsdroid-ipc: JSON-RPC API types, named-pipe transport and OpenRPC schema.
//!
//! M0 only reserves the crate boundary and the API version. The daemon versus
//! one-shot decision, the transport and the generated schema arrive in M3.

/// Version of the JSON-RPC API shared by the GUI, CLI and scripts.
///
/// Changing a method or its schema requires regenerating
/// `docs/api/openrpc.json` and bumping this version.
pub const API_VERSION: &str = "0.1.0";
