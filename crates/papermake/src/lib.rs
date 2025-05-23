//! Papermake is a PDF generation library that uses Typst templates
//! with associated schemas to render PDFs from structured data.

pub mod error;
pub mod schema;
pub mod template;
pub mod render;
pub mod storage;
pub mod typst;
// Re-export core types
pub use error::{PapermakeError, Result};
pub use schema::{Schema, SchemaField, FieldType};
pub use template::{Template, TemplateId};
pub use render::{render_pdf, RenderOptions};
pub use storage::Storage;

/// Get the library version
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}