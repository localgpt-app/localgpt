#[cfg(not(target_arch = "wasm32"))]
mod http;
#[cfg(not(target_arch = "wasm32"))]
mod openai_compat;
#[cfg(not(target_arch = "wasm32"))]
mod rate_limiter;
#[cfg(not(target_arch = "wasm32"))]
pub mod telegram;

#[cfg(not(target_arch = "wasm32"))]
pub mod security;
#[cfg(not(target_arch = "wasm32"))]
pub mod tls;

#[cfg(not(target_arch = "wasm32"))]
pub use http::Server;
#[cfg(not(target_arch = "wasm32"))]
pub use security::BridgeManager;
#[cfg(not(target_arch = "wasm32"))]
pub use security::HealthStatus;
