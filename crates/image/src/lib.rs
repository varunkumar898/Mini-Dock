#![forbid(unsafe_code)]

//! `alphen-image`: OCI image store, layer extractor, and registry distribution client.

pub mod layer;
pub mod registry;
pub mod store;

pub use layer::extract_layer;
pub use registry::RegistryClient;
pub use store::{GcReport, ImageStore};
