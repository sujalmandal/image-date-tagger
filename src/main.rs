use axum::{
    body::Body,
    extract::{Path, State},
    http::{HeaderValue, StatusCode},
    response::{Html, IntoResponse, Json, Response},
    routing::{get, post, put},
    Router,
};
use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use chrono::NaiveDate;
use futures_util::stream;
use multer::Multipart as MulterMultipart;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tower_http::{cors::CorsLayer, services::ServeDir};
use tower_http::cors::Any;
use tracing::info;

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------
const DATA_DIR: &str = "data";
const ANNOTATIONS_FILE: &str = "data/annotations.json";
const ROOT_DIR: &str = "data/uploads";
const INDEX_HTML: &str = "templates/index.html";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FileAnnotation {
    filename: String,
    #[serde(default)]
    extracted_date: Option<String>,
    #[serde(default)]
    corrected_date: Option<String>,
    #[serde(default)]
    is_invalid: bool,
    #[serde(default)]
    sort_index: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct Annotations {
    files: Vec<FileAnnotation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OcrResult {
    filename: String,
    raw: String,
    extracted_date: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OcrJobState {
    id: Option<String>,
    status: String,
    total: usize,
    done: usize,
    failed: usize,
    current: Option<String>,
    message: String,
    cancelled: bool,
    started_at: Option<f64>,
    finished_at: Option<f64>,
    results: Vec<OcrResult>,
}

impl Default for OcrJobState {
    fn default() -> Self {
        Self {
            id: None,
            status: "idle".into(),
            total: 0,
            done: 0,
            failed: 0,
            current: None,
            message: String::new(),
            cancelled: false,
            started_at: None,
            finished_at: None,
            results: Vec::new(),
        }
    }
}

#[derive(Deserialize)]
struct AnnotationUpdate {
    corrected_date: Option<String>,
    is_invalid: Option<bool>,
}

#[derive(Deserialize)]
struct OcrBatchRequest {
    filenames: Vec<String>,
}

#[derive(Clone)]
struct AppState {
    inner: Arc<RwLock<AppStateInner>>,
}

struct AppStateInner {
    annotations: Annotations,
    ocr_job: OcrJobState,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn now_epoch() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

fn ensure_dirs() {
    std::fs::create_dir_all(DATA_DIR).ok();
    std::fs::create_dir_all(ROOT_DIR).ok();
}

fn ensure_annotations_file() {
    ensure_dirs();
    if !std::path::Path::new(ANNOTATIONS_FILE).exists() {
        let default = Annotations { files: Vec::new() };
        let json = serde_json::to_string_pretty(&default).unwrap();
        std::fs::write(ANNOTATIONS_FILE, &json).ok();
    }
}

fn load_annotations() -> Annotations {
    ensure_annotations_file();
    match std::fs::read_to_string(ANNOTATIONS_FILE) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Annotations::default(),
    }
}

fn save_annotations(data: &Annotations) {
    ensure_dirs();
    let json = serde_json::to_string_pretty(data).unwrap();
    let tmp = format!("{}.tmp", ANNOTATIONS_FILE);
    std::fs::write(&tmp, &json).ok();
    std::fs::rename(&tmp, ANNOTATIONS_FILE).ok();
}

fn list_image_files() -> Vec<String> {
    let dir = std::path::Path::new(ROOT_DIR);
    if !dir.exists() {
        return Vec::new();
    }
    let mut files: Vec<String> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if let Some(ext) = path.extension() {
                if ext.to_str().map(|e| e.eq_ignore_ascii_case("jpg")).unwrap_or(false) {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        files.push(name.to_string());
                    }
                }
            }
        }
    }
    files.sort();
    files
}

fn sync_annotations() -> Annotations {
    let mut data = load_annotations();
    let existing: std::collections::HashMap<String, FileAnnotation> = data
        .files
        .drain(..)
        .map(|f| (f.filename.clone(), f))
        .collect();

    let files = list_image_files();
    let mut new_files: Vec<FileAnnotation> = Vec::with_capacity(files.len());
    for (idx, fname) in files.iter().enumerate() {
        let mut entry = existing.get(fname).cloned().unwrap_or_else(|| FileAnnotation {
            filename: fname.clone(),
            extracted_date: None,
            corrected_date: None,
            is_invalid: false,
            sort_index: idx,
        });
        entry.sort_index = idx;
        new_files.push(entry);
    }
    data.files = new_files;
    save_annotations(&data);
    data
}

fn parse_date(text: &str) -> Option<String> {
    let text = text.trim();

    // DD-MM-YYYY or DD/MM/YYYY
    let re_full = Regex::new(r"\b(\d{1,2})[-/](\d{1,2})[-/](\d{4})\b").unwrap();
    if let Some(caps) = re_full.captures(text) {
        let d: u32 = caps[1].parse().unwrap_or(99);
        let mo: u32 = caps[2].parse().unwrap_or(99);
        if (1..=31).contains(&d) && (1..=12).contains(&mo) {
            return Some(format!(
                "{:02}-{:02}-{}",
                caps[1].parse::<u32>().unwrap_or(0),
                caps[2].parse::<u32>().unwrap_or(0),
                &caps[3]
            ));
        }
    }

    // DD-MM-YY or DD/MM/YY
    let re_short = Regex::new(r"\b(\d{1,2})[-/](\d{1,2})[-/](\d{2})\b").unwrap();
    if let Some(caps) = re_short.captures(text) {
        let d: u32 = caps[1].parse().unwrap_or(99);
        let mo: u32 = caps[2].parse().unwrap_or(99);
        if (1..=31).contains(&d) && (1..=12).contains(&mo) {
            return Some(format!(
                "{:02}-{:02}-20{}",
                caps[1].parse::<u32>().unwrap_or(0),
                caps[2].parse::<u32>().unwrap_or(0),
                &caps[3]
            ));
        }
    }

    if Regex::new(r"NO_DATE|no date")
        .unwrap()
        .is_match(text)
    {
        return None;
    }

    None
}

fn mime_type(path: &std::path::Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if ext.eq_ignore_ascii_case("jpg") || ext.eq_ignore_ascii_case("jpeg") => "image/jpeg",
        Some(ext) if ext.eq_ignore_ascii_case("png") => "image/png",
        Some(ext) if ext.eq_ignore_ascii_case("webp") => "image/webp",
        _ => "image/jpeg",
    }
}

async fn ocr_image(path: &std::path::Path) -> String {
    let ocr_url = std::env::var("OCR_URL")
        .unwrap_or_else(|_| "http://localhost:1234/v1".into());
    let ocr_model = std::env::var("OCR_MODEL")
        .unwrap_or_else(|_| "gemma4-26b-a4b-qat-uncensored-hauhaucs-balanced-mtp".into());
    let ocr_api_key = std::env::var("OCR_API_KEY").unwrap_or_default();
    let ocr_prompt = "Read the date from this sales receipt image. Reply with ONLY the date in DD-MM-YYYY format. If you cannot find a date, reply with NO_DATE.";

    let image_bytes = match tokio::fs::read(path).await {
        Ok(b) => b,
        Err(e) => return format!("ERROR: {e}"),
    };
    let b64 = BASE64.encode(&image_bytes);
    let mime = mime_type(path);
    let url = format!("{}/chat/completions", ocr_url.trim_end_matches('/'));

    let payload = serde_json::json!({
        "model": ocr_model,
        "messages": [
            {
                "role": "user",
                "content": [
                    {"type": "text", "text": ocr_prompt},
                    {"type": "image_url", "image_url": {"url": format!("data:{};base64,{}", mime, b64)}}
                ]
            }
        ],
        "max_tokens": 50,
        "temperature": 0.1,
    });

    let client = reqwest::Client::new();
    let mut req = client.post(&url).json(&payload).timeout(std::time::Duration::from_secs(120));
    if !ocr_api_key.is_empty() {
        req = req.header("Authorization", format!("Bearer {ocr_api_key}"));
    }

    match req.send().await {
        Ok(resp) => {
            match resp.json::<serde_json::Value>().await {
                Ok(data) => {
                    let choices = data.get("choices").and_then(|c| c.as_array());
                    if let Some(choices) = choices {
                        if let Some(first) = choices.first() {
                            if let Some(content) = first.get("message").and_then(|m| m.get("content")) {
                                return content.as_str().unwrap_or("ERROR: empty content").to_string();
                            }
                        }
                    }
                    "ERROR: no choices in response".into()
                }
                Err(e) => format!("ERROR: {e}"),
            }
        }
        Err(e) => format!("ERROR: {e}"),
    }
}

async fn run_ocr_single(state: &AppState, filename: &str) -> (String, Option<String>) {
    let path = std::path::Path::new(ROOT_DIR).join(filename);
    let raw = ocr_image(&path).await;
    let extracted = parse_date(&raw);

    let mut inner = state.inner.write().await;
    if let Some(entry) = inner.annotations.files.iter_mut().find(|f| f.filename == filename) {
        entry.extracted_date = extracted.clone();
    }
    save_annotations(&inner.annotations);

    (raw, extracted)
}

fn sort_dates_entries(data: &Annotations) -> Vec<serde_json::Value> {
    use std::collections::BTreeMap;
    let mut date_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in &data.files {
        let date = entry
            .corrected_date
            .as_deref()
            .or(entry.extracted_date.as_deref());
        let Some(date) = date else { continue };
        if entry.is_invalid {
            continue;
        }
        date_map
            .entry(date.to_string())
            .or_default()
            .push(entry.filename.clone());
    }

    // Sort chronologically by DD-MM-YYYY
    let mut entries: Vec<_> = date_map.into_iter().collect();
    entries.sort_by(|(a, _), (b, _)| {
        let a_date = NaiveDate::parse_from_str(a, "%d-%m-%Y").ok();
        let b_date = NaiveDate::parse_from_str(b, "%d-%m-%Y").ok();
        a_date.cmp(&b_date)
    });

    entries
        .into_iter()
        .map(|(date, files)| serde_json::json!({"date": date, "files": files}))
        .collect()
}

// ---------------------------------------------------------------------------
// Route handlers
// ---------------------------------------------------------------------------

async fn index() -> impl IntoResponse {
    match tokio::fs::read_to_string(INDEX_HTML).await {
        Ok(html) => Html(html).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")).into_response(),
    }
}

async fn get_files(State(state): State<AppState>) -> Json<serde_json::Value> {
    let inner = state.inner.read().await;
    Json(serde_json::to_value(&inner.annotations.files).unwrap_or_default())
}

async fn update_file(
    State(state): State<AppState>,
    Path(filename): Path<String>,
    Json(update): Json<AnnotationUpdate>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut inner = state.inner.write().await;
    let entry = inner
        .annotations
        .files
        .iter_mut()
        .find(|f| f.filename == filename)
        .ok_or_else(|| (StatusCode::NOT_FOUND, "File not found".into()))?;

    if let Some(ref d) = update.corrected_date {
        entry.corrected_date = if d.is_empty() { None } else { Some(d.clone()) };
    }
    if let Some(v) = update.is_invalid {
        entry.is_invalid = v;
    }
    let cloned = entry.clone();
    save_annotations(&inner.annotations);
    drop(inner);

    Ok(Json(serde_json::to_value(&cloned).unwrap_or_default()))
}

async fn get_image(
    Path(filename): Path<String>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let path = std::path::Path::new(ROOT_DIR).join(&filename);
    if !path.exists() {
        return Err((StatusCode::NOT_FOUND, "Image not found".into()));
    }
    let mime = mime_type(&path);
    let data = tokio::fs::read(&path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;

    Ok((
        [(axum::http::header::CONTENT_TYPE, mime)],
        data,
    )
        .into_response())
}

async fn get_dates(State(state): State<AppState>) -> Json<Vec<serde_json::Value>> {
    let inner = state.inner.read().await;
    Json(sort_dates_entries(&inner.annotations))
}

async fn run_ocr(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let path = std::path::Path::new(ROOT_DIR).join(&filename);
    if !path.exists() {
        return Err((StatusCode::NOT_FOUND, "Image not found".into()));
    }

    let (raw, extracted) = run_ocr_single(&state, &filename).await;

    Ok(Json(serde_json::json!({
        "filename": filename,
        "raw": raw,
        "extracted_date": extracted,
    })))
}

async fn start_ocr_job(
    State(state): State<AppState>,
    Json(req): Json<OcrBatchRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let filenames: Vec<String> = req
        .filenames
        .into_iter()
        .filter(|f| std::path::Path::new(ROOT_DIR).join(f).exists())
        .collect();

    if filenames.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "No valid images provided".into()));
    }

    let job_id = uuid::Uuid::new_v4().to_string();
    let total = filenames.len();

    // Atomically check and set job state
    {
        let mut inner = state.inner.write().await;
        if inner.ocr_job.status == "running" {
            return Err((StatusCode::CONFLICT, "An OCR job is already running".into()));
        }
        inner.ocr_job = OcrJobState {
            id: Some(job_id.clone()),
            status: "running".into(),
            total,
            done: 0,
            failed: 0,
            current: None,
            message: "Starting OCR batch...".into(),
            cancelled: false,
            started_at: Some(now_epoch()),
            finished_at: None,
            results: Vec::new(),
        };
    }

    // Spawn background worker
    let state_clone = state.clone();
    let jid = job_id.clone();
    tokio::spawn(async move {
        ocr_worker(state_clone, jid, filenames).await;
    });

    Ok(Json(serde_json::json!({
        "job_id": job_id,
        "total": total,
        "status": "running",
    })))
}

async fn ocr_worker(state: AppState, _job_id: String, filenames: Vec<String>) {
    let total = filenames.len();
    let mut results: Vec<OcrResult> = Vec::new();

    for (i, fname) in filenames.iter().enumerate() {
        // Check cancellation
        {
            let inner = state.inner.read().await;
            if inner.ocr_job.cancelled {
                let mut inner = state.inner.write().await;
                inner.ocr_job.status = "cancelled".into();
                inner.ocr_job.message = "Cancelled by user.".into();
                inner.ocr_job.finished_at = Some(now_epoch());
                return;
            }
        }

        // Update current
        {
            let mut inner = state.inner.write().await;
            inner.ocr_job.current = Some(fname.clone());
            inner.ocr_job.message = format!("OCR {}/{}: {}", i + 1, total, fname);
        }

        // Run OCR
        let (raw, extracted) = run_ocr_single(&state, fname).await;

        let res = OcrResult {
            filename: fname.clone(),
            raw,
            extracted_date: extracted,
        };
        results.push(res);

        // Update progress
        {
            let mut inner = state.inner.write().await;
            inner.ocr_job.done = i + 1;
            inner.ocr_job.failed = results.iter().filter(|r| r.extracted_date.is_none()).count();
            inner.ocr_job.results = results.clone();
        }
    }

    // Mark done
    {
        let mut inner = state.inner.write().await;
        inner.ocr_job.status = "done".into();
        inner.ocr_job.current = None;
        inner.ocr_job.message = format!("OCR complete: {total} files processed.");
        inner.ocr_job.finished_at = Some(now_epoch());
        inner.ocr_job.results = results;
    }
}

async fn get_ocr_job(State(state): State<AppState>) -> Json<serde_json::Value> {
    let inner = state.inner.read().await;
    Json(serde_json::to_value(&inner.ocr_job).unwrap_or_default())
}

async fn cancel_ocr_job(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let mut inner = state.inner.write().await;
    if inner.ocr_job.status != "running" {
        return Err((StatusCode::CONFLICT, "No OCR job is running".into()));
    }
    inner.ocr_job.cancelled = true;
    Ok(Json(serde_json::json!({"status": "cancelling"})))
}

async fn export_file(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<Response<Body>, (StatusCode, String)> {
    let path = std::path::Path::new(ROOT_DIR).join(&filename);
    if !path.exists() {
        return Err((StatusCode::NOT_FOUND, "File not found".into()));
    }

    let data = tokio::fs::read(&path)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;

    let mime = mime_type(&path);

    let inner = state.inner.read().await;
    let date = inner
        .annotations
        .files
        .iter()
        .find(|f| f.filename == filename)
        .and_then(|f| f.corrected_date.as_deref().or(f.extracted_date.as_deref()));

    let download_name = match date {
        Some(d) => format!("{}_{}", d, filename),
        None => filename.clone(),
    };

    let mut resp = Response::new(Body::from(data));
    resp.headers_mut()
        .insert(axum::http::header::CONTENT_TYPE, HeaderValue::from_static(mime));
    resp.headers_mut().insert(
        axum::http::header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{}\"", download_name))
            .unwrap(),
    );
    Ok(resp)
}

async fn delete_file(
    State(state): State<AppState>,
    Path(filename): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let path = std::path::Path::new(ROOT_DIR).join(&filename);
    if path.exists() {
        tokio::fs::remove_file(&path)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;
    }

    let mut inner = state.inner.write().await;
    inner.annotations.files.retain(|f| f.filename != filename);
    save_annotations(&inner.annotations);

    Ok(Json(serde_json::json!({"deleted": true})))
}

async fn ocr_progress(State(state): State<AppState>) -> Json<serde_json::Value> {
    let inner = state.inner.read().await;
    let total = inner.annotations.files.len();
    let done = inner
        .annotations
        .files
        .iter()
        .filter(|f| f.extracted_date.is_some())
        .count();
    Json(serde_json::json!({
        "total": total,
        "done": done,
        "remaining": total - done,
        "job": inner.ocr_job,
    }))
}

async fn upload_files(
    State(state): State<AppState>,
    req: axum::extract::Request,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    std::fs::create_dir_all(ROOT_DIR)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;

    let content_type = req
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    if !content_type.contains("multipart/form-data") {
        return Err((
            StatusCode::BAD_REQUEST,
            format!("Expected multipart/form-data, got: {content_type}"),
        ));
    }

    let boundary = multer::parse_boundary(&content_type)
        .map_err(|e| (StatusCode::BAD_REQUEST, format!("Invalid boundary: {e}")))?;

    let body_bytes = axum::body::to_bytes(req.into_body(), 250 * 1024 * 1024)
        .await
        .map_err(|e| (StatusCode::PAYLOAD_TOO_LARGE, format!("{e}")))?;

    let stream = stream::once(async move { Ok::<_, std::convert::Infallible>(body_bytes.to_vec()) });
    let mut multipart = MulterMultipart::new(stream, boundary);

    let mut saved: Vec<String> = Vec::new();
    let mut skipped: Vec<String> = Vec::new();

    while let Ok(Some(field)) = multipart.next_field().await {
        let file_name = field
            .file_name()
            .unwrap_or("unknown")
            .to_string();

        let ext = std::path::Path::new(&file_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        if !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp") {
            skipped.push(file_name);
            continue;
        }

        let data = field.bytes().await.map_err(|e| {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}"))
        })?;

        let dest = std::path::Path::new(ROOT_DIR).join(&file_name);
        tokio::fs::write(&dest, &data)
            .await
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("{e}")))?;

        saved.push(file_name);
    }

    // Sync annotations
    {
        let mut inner = state.inner.write().await;
        inner.annotations = sync_annotations();
    }

    Ok(Json(serde_json::json!({
        "saved": saved,
        "skipped": skipped,
        "count": saved.len(),
    })))
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    // Load .env
    dotenvy::dotenv().ok();

    // Ensure directories and sync annotations
    ensure_annotations_file();
    let annotations = sync_annotations();

    let inner = AppStateInner {
        annotations,
        ocr_job: OcrJobState::default(),
    };

    let state = AppState {
        inner: Arc::new(RwLock::new(inner)),
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(index))
        .route("/api/files", get(get_files))
        .route("/api/files/{filename}", put(update_file).delete(delete_file))
        .route("/api/images/{filename}", get(get_image))
        .route("/api/export/{filename}", get(export_file))
        .route("/api/dates", get(get_dates))
        .route("/api/ocr/{filename}", post(run_ocr))
        .route("/api/ocr-job", post(start_ocr_job).get(get_ocr_job))
        .route("/api/ocr-job/cancel", post(cancel_ocr_job))
        .route("/api/ocr-progress", get(ocr_progress))
        .route("/api/upload", post(upload_files))
        .nest_service("/static", ServeDir::new("static"))
        .layer(cors)
        .with_state(state);

    let addr = "0.0.0.0:8000";
    info!("Listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
