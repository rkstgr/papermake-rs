//! PDF rendering functionality

use serde::Serialize;
use typst::WorldExt;
use typst::World;
use typst_pdf::PdfOptions;

use crate::error::Result;
use crate::template::Template;
use crate::typst::TypstWorld;
use crate::PapermakeError;

/// Options for PDF rendering
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Paper size (e.g., "a4", "letter")
    pub paper_size: String,
    
    /// Whether to compress the output PDF
    pub compress: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            paper_size: "a4".to_string(),
            compress: true,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct RenderError {
    pub message: String,
    pub start: usize,
    pub end: usize
}

#[derive(Debug, Serialize)]
pub struct RenderResult {
    pub pdf: Option<Vec<u8>>,
    pub errors: Vec<RenderError>,
}

/// Render a template with data to a PDF
pub fn render_pdf(
    template: &Template,
    data: &serde_json::Value,
    _options: Option<RenderOptions>,
) -> Result<RenderResult> {
    // Validate data against schema
    template.validate_data(data)?;
    
    let world = TypstWorld::new(
        template.content.clone(),
        serde_json::to_string(&data).map_err(|e| PapermakeError::Rendering(e.to_string()))?,
    );

    let compile_result = typst::compile(&world);
    
    let mut errors = Vec::new();
    let mut pdf = None;

    match compile_result.output {
        Ok(document) => {
            pdf = Some(typst_pdf::pdf(&document, &PdfOptions::default()).unwrap());
        }
        Err(diagnostics) => {
            for diagnostic in diagnostics {
                let span = diagnostic.span;
                if let Some(id) = span.id() {
                    if let Ok(_file) = world.source(id) {
                        if let Some(range) = world.range(span) {
                            errors.push(RenderError {
                                message: diagnostic.message.to_string(),
                                start: range.start,
                                end: range.end,});
                        }
                    }
                }
            }
        }
    }

    Ok(RenderResult {
        pdf,
        errors,
    })
}

pub fn render_pdf_with_cache(
    template: &Template,
    data: &serde_json::Value,
    world_cache: Option<&mut TypstWorld>, // Add a cache parameter
    _options: Option<RenderOptions>,
) -> Result<RenderResult> {
    // Validate data against schema
    template.validate_data(data)?;
    
    // Either use the cached world or create a new one
    let world = match world_cache {
        Some(cached_world) => {
            // Update the inputs in the existing world
            cached_world.update_data(
                serde_json::to_string(&data).map_err(|e| PapermakeError::Rendering(e.to_string()))?,
            ).map_err(|e| PapermakeError::Rendering(e.to_string()))?;
            // Make sure to reset tracking state
            // cached_world.reset(); TODO: Implement this
            cached_world
        }
        None => &mut TypstWorld::new(
            template.content.clone(),
            serde_json::to_string(&data).map_err(|e| PapermakeError::Rendering(e.to_string()))?,
        ),
    };

    let compile_result = typst::compile(world as &dyn World);

    let mut errors = Vec::new();
    let mut pdf = None;

    match compile_result.output {
        Ok(document) => {
            pdf = Some(typst_pdf::pdf(&document, &PdfOptions::default()).unwrap());
        }
        Err(diagnostics) => {
            for diagnostic in diagnostics {
                let span = diagnostic.span;
                if let Some(id) = span.id() {
                    if let Ok(_file) = world.source(id) {
                        if let Some(range) = world.range(span) {
                            errors.push(RenderError {
                                message: diagnostic.message.to_string(),
                                start: range.start,
                                end: range.end,});
                        }
                    }
                }
            }
        }
    }

    Ok(RenderResult {
        pdf,
        errors,
    })
}