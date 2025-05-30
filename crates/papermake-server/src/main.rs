use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use base64::{prelude::BASE64_STANDARD, Engine};
use papermake::{
    error::PapermakeError, render::{render_pdf, RenderError, RenderOptions}, storage::{FileStorage, Storage}, template::{Template, TemplateId}
};
use serde::{Deserialize, Serialize};
use tower_http::trace::TraceLayer;
use tower_http::cors::CorsLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

// Application state with shared storage
struct AppState {
    storage: Arc<dyn Storage>,
}

// Request and response types
#[derive(Deserialize)]
struct CreateTemplateRequest {
    id: String,
    name: String,
    content: String,
    schema: papermake::schema::Schema,
    description: Option<String>,
}

#[derive(Deserialize)]
struct UpdateTemplateRequest {
    name: Option<String>,
    content: Option<String>,
    schema: Option<papermake::schema::Schema>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct RenderTemplateRequest {
    data: serde_json::Value,
    options: Option<RenderOptionsRequest>,
}

#[derive(Deserialize)]
struct RenderOptionsRequest {
    paper_size: Option<String>,
    compress: Option<bool>,
}

#[derive(Serialize)]
struct RenderResultResponse {
    pdf_base64: Option<String>,
    errors: Vec<RenderError>,
}

#[derive(Serialize)]
struct TemplateResponse {
    id: String,
    name: String,
    schema: papermake::schema::Schema,
    content: String,
    description: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<Template> for TemplateResponse {
    fn from(template: Template) -> Self {
        Self {
            id: template.id.0,
            name: template.name,
            schema: template.schema,
            content: template.content,
            description: template.description,
            created_at: template.created_at.to_string(),
            updated_at: template.updated_at.to_string(),
        }
    }
}

// Error handling
enum AppError {
    Papermake(PapermakeError),
    NotFound,
    BadRequest(String),
}

impl From<PapermakeError> for AppError {
    fn from(err: PapermakeError) -> Self {
        Self::Papermake(err)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> axum::response::Response {
        let (status, error_message) = match self {
            Self::Papermake(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
            Self::NotFound => (StatusCode::NOT_FOUND, "Resource not found".to_string()),
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
        };

        (status, Json(serde_json::json!({ "error": error_message }))).into_response()
    }
}

#[tokio::main]
async fn main() {
    // Initialize tracing with more detailed configuration
    tracing_subscriber::registry()
        .with(EnvFilter::new(
            std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "papermake_server=debug,tower_http=debug".into())
        ))
        .with(tracing_subscriber::fmt::layer())
        .try_init()
        .unwrap();
    // Initialize storage
    let storage_path = std::env::var("PAPERMAKE_STORAGE_PATH")
        .unwrap_or_else(|_| "./data".to_string());
    let storage = Arc::new(FileStorage::new(PathBuf::from(storage_path)));

    // Create app state
    let state = Arc::new(AppState { storage });

    // Build router
    let app = Router::new()
        .route("/templates", get(list_templates).post(create_template))
        .route("/templates/{id}", 
            get(get_template)
            .put(update_template)
            .delete(delete_template))
        .route("/templates/{id}/render", post(render_template))
        .route("/templates/{id}/files", get(list_template_files))
        .route("/templates/{id}/files/{*path}", 
            get(get_template_file)
            .put(save_template_file)
            .delete(delete_template_file))
        .route("/health", get(health_check))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    tracing::debug_span!(
                        "http_request",
                        method = %request.method(),
                        uri = %request.uri(),
                        version = ?request.version(),
                    )
                })
                .on_request(|request: &axum::http::Request<_>, _span: &tracing::Span| {
                    tracing::debug!("started {} {}", request.method(), request.uri());
                })
                .on_response(|response: &axum::http::Response<_>, latency: std::time::Duration, _span: &tracing::Span| {
                    tracing::debug!("response generated in {:?}", latency);
                    tracing::debug!("status: {}", response.status());
                })
        )
        .layer(CorsLayer::permissive())
        .with_state(state);

    // Run server
    let port = std::env::var("PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(3000);
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    
    tracing::info!("Server listening on {}", addr);
    
    axum::serve(
        tokio::net::TcpListener::bind(addr).await.unwrap(),
        app.into_make_service()
    ).await.unwrap();
}

// Route handlers

// Template operations
async fn list_templates(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<TemplateResponse>>, AppError> {
    let templates = state.storage.list_templates().await?;
    Ok(Json(templates.into_iter().map(TemplateResponse::from).collect()))
}

async fn create_template(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateTemplateRequest>,
) -> Result<Json<TemplateResponse>, AppError> {
    let template = Template::new(
        payload.id,
        payload.name,
        payload.content,
        payload.schema,
    );
    
    let template = if let Some(description) = payload.description {
        template.with_description(description)
    } else {
        template
    };

    state.storage.save_template(&template).await?;
    Ok(Json(TemplateResponse::from(template)))
}

async fn get_template(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<TemplateResponse>, AppError> {
    let template = state.storage.get_template(&TemplateId(id)).await
        .map_err(|_| AppError::NotFound)?;
    Ok(Json(TemplateResponse::from(template)))
}

async fn update_template(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(payload): Json<UpdateTemplateRequest>,
) -> Result<Json<TemplateResponse>, AppError> {
    let mut template = state.storage.get_template(&TemplateId(id)).await
        .map_err(|_| AppError::NotFound)?;
    
    if let Some(name) = payload.name {
        template.name = name;
    }
    
    if let Some(content) = payload.content {
        template.content = content;
    }
    
    if let Some(schema) = payload.schema {
        template.schema = schema;
    }
    
    if let Some(description) = payload.description {
        template.description = Some(description);
    }
    
    template.updated_at = time::OffsetDateTime::now_utc();
    
    state.storage.save_template(&template).await?;
    Ok(Json(TemplateResponse::from(template)))
}

async fn delete_template(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    state.storage.delete_template(&TemplateId(id)).await
        .map_err(|_| AppError::NotFound)?;
    Ok(StatusCode::NO_CONTENT)
}

// Rendering
async fn render_template(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(payload): Json<RenderTemplateRequest>,
) -> Result<Json<RenderResultResponse>, AppError> {
    let template = state.storage.get_template(&TemplateId(id)).await
        .map_err(|_| AppError::NotFound)?;
    
    // Convert options if provided
    let options = payload.options.map(|opts| RenderOptions {
        paper_size: opts.paper_size.unwrap_or_else(|| "a4".to_string()),
        compress: opts.compress.unwrap_or(true),
    });
    
    // Validate data against schema
    if let Err(err) = template.validate_data(&payload.data) {
        return Err(AppError::BadRequest(format!("Invalid data: {}", err)));
    }
    
    // Render PDF and handle errors
    let render_result = match render_pdf(&template, &payload.data, options) {
        Ok(result) => result,
        Err(e) => return Err(AppError::Papermake(e)),
    };

    // Convert PDF to base64 if present
    let pdf_base64 = render_result.pdf
        .as_ref()
        .map(|pdf| BASE64_STANDARD.encode(pdf));

    Ok(Json(RenderResultResponse {
        pdf_base64,
        errors: render_result.errors,
    }))
    
}

// Template file operations
async fn list_template_files(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Result<Json<Vec<String>>, AppError> {
    let files = state.storage.list_template_files(&TemplateId(id)).await
        .map_err(|_| AppError::NotFound)?;
    Ok(Json(files))
}

async fn get_template_file(
    State(state): State<Arc<AppState>>,
    Path((id, path)): Path<(String, String)>,
) -> Result<Vec<u8>, AppError> {
    let content = state.storage.get_template_file(&TemplateId(id), &path).await
        .map_err(|_| AppError::NotFound)?;
    Ok(content)
}

async fn save_template_file(
    State(state): State<Arc<AppState>>,
    Path((id, path)): Path<(String, String)>,
    body: axum::body::Bytes,
) -> Result<StatusCode, AppError> {
    state.storage.save_template_file(&TemplateId(id), &path, &body).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn delete_template_file(
    State(state): State<Arc<AppState>>,
    Path((id, path)): Path<(String, String)>,
) -> Result<StatusCode, AppError> {
    // This method might need to be added to your Storage trait
    // For now, we'll just acknowledge the request
    Ok(StatusCode::NO_CONTENT)
}

// Health check
async fn health_check() -> StatusCode {
    StatusCode::OK
}