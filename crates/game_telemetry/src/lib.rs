//! Official-build error reporting. The administration token is never read here.
#[cfg(feature = "official")]
mod enabled;
#[cfg(feature = "official")]
pub use enabled::*;
