use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{header, StatusCode};
use axum::response::Response;

use crate::config::AppState;
use crate::error::AppError;
use crate::services::{combine, iso};

/// GET /content/iso/{release}/{*path}
/// Stream file from inside ISO
pub async fn get_iso_content(
    State(state): State<Arc<AppState>>,
    Path((release, path)): Path<(String, String)>,
) -> Result<Response, AppError> {
    let filename = state
        .aliases
        .get_filename(&release)
        .ok_or_else(|| AppError::NotFound(format!("Unknown release: {}", release)))?;

    let iso_path = state.config.iso_dir().join(filename);

    if !iso_path.exists() {
        return Err(AppError::NotFound(format!("ISO file not found: {}", filename)));
    }

    let (size, body) = iso::stream_file(&iso_path, &path).await?;

    let content_type = guess_content_type(&path);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_LENGTH, size)
        .header(header::CONTENT_TYPE, content_type)
        .body(body)
        .unwrap())
}

/// GET /content/combine/{name}
/// Stream concatenated files
pub async fn get_combined_content(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Response, AppError> {
    let entry = state
        .combine
        .get(&name)
        .ok_or_else(|| AppError::NotFound(format!("Unknown combine entry: {}", name)))?;

    let (size, body) = combine::stream_combined(entry, &state.config.iso_dir(), &state.aliases).await?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_LENGTH, size)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(body)
        .unwrap())
}

/// GET /content/raw/{release}/{filename}
/// Stream full ISO file (only if marked downloadable)
pub async fn get_raw_content(
    State(state): State<Arc<AppState>>,
    Path((release, filename)): Path<(String, String)>,
) -> Result<Response, AppError> {
    // Check if downloadable
    if !state.aliases.is_downloadable(&release) {
        return Err(AppError::Forbidden(format!(
            "Release '{}' is not marked as downloadable",
            release
        )));
    }

    // Verify filename matches
    let expected_filename = state
        .aliases
        .get_filename(&release)
        .ok_or_else(|| AppError::NotFound(format!("Unknown release: {}", release)))?;

    if filename != expected_filename {
        return Err(AppError::BadRequest(format!(
            "Filename mismatch: expected '{}', got '{}'",
            expected_filename, filename
        )));
    }

    let iso_path = state.config.iso_dir().join(&filename);

    if !iso_path.exists() {
        return Err(AppError::NotFound(format!("ISO file not found: {}", filename)));
    }

    let metadata = tokio::fs::metadata(&iso_path).await?;
    let size = metadata.len();

    let file = tokio::fs::File::open(&iso_path).await?;
    let stream = tokio_util::io::ReaderStream::new(tokio::io::BufReader::with_capacity(
        1024 * 1024, // 1MB buffer
        file,
    ));
    let body = Body::from_stream(stream);

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_LENGTH, size)
        .header(header::CONTENT_TYPE, "application/x-iso9660-image")
        .header(
            header::CONTENT_DISPOSITION,
            format!("attachment; filename=\"{}\"", filename),
        )
        .body(body)
        .unwrap())
}

fn guess_content_type(path: &str) -> &'static str {
    let lower = path.to_lowercase();
    if lower.ends_with(".gz") {
        "application/gzip"
    } else if lower.ends_with(".j2") || lower.ends_with(".cfg") || lower.ends_with(".txt") {
        "text/plain"
    } else if lower.ends_with(".ipxe") {
        "text/plain"
    } else {
        "application/octet-stream"
    }
}
