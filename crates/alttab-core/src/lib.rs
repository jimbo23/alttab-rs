//! Deterministic window-switching policy. Native handles and OS calls stay in the adapter.
pub mod config;
pub mod filter;
pub mod layout;
pub mod model;
pub mod order;
pub mod session;
pub mod thumbnail;

pub use config::*;
pub use filter::*;
pub use layout::*;
pub use model::*;
pub use order::*;
pub use session::*;
pub use thumbnail::*;
