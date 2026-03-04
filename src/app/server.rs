// src/app/server.rs
//
// Local HTTP/JSON API server that wraps the Engine.
// Used by the CLI daemon mode and optionally by the GUI for AI-driven testing.

use crate::app::commands::AppCommand;
use crate::app::engine::Engine;
use serde::Serialize;
use std::sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}};
use tiny_http::{Server, Response, Header, Method, StatusCode};

/// Response envelope for all API calls.
#[derive(Serialize)]
struct ApiResponse<T: Serialize> {
    success: bool,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<T>,
}

/// Summary of a sentence for the state listing (avoids serializing full token streams).
#[derive(Serialize)]
struct SentenceSummary {
    index: usize,
    id: String,
    tier_count: usize,
    tiers: std::collections::HashMap<String, TierSummary>,
    mapping_count: usize,
}

#[derive(Serialize)]
struct TierSummary {
    state: String,
    full_text: String,
}

/// Top-level state summary returned by GET /api/v1/state.
#[derive(Serialize)]
struct StateSummary {
    sentence_count: usize,
    selected_sentence_idx: usize,
    selected_range: Option<(usize, usize)>,
    project_languages: (String, String),
    sentences: Vec<SentenceSummary>,
}

/// Configuration for starting the server.
pub struct ServerConfig {
    pub port: u16,
    pub name: String,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            port: 3030,
            name: "default".to_string(),
        }
    }
}

/// Start the API server on the given port, blocking the current thread.
/// Returns Ok(()) when the server is shut down gracefully via /api/v1/shutdown.
pub fn run_server(engine: Arc<Mutex<Engine>>, config: ServerConfig) -> Result<(), String> {
    let addr = format!("0.0.0.0:{}", config.port);
    let server = Server::http(&addr).map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;
    
    println!("[SERVER] '{}' listening on http://127.0.0.1:{}", config.name, config.port);

    let shutdown = Arc::new(AtomicBool::new(false));

    for mut request in server.incoming_requests() {
        if shutdown.load(Ordering::Relaxed) {
            let _ = request.respond(json_response(200, &ApiResponse::<()> {
                success: true,
                message: "Server shutting down".to_string(),
                data: None,
            }));
            break;
        }

        let url = request.url().to_string();
        let method = request.method().clone();

        // Route the request
        let result = match (method, url.as_str()) {
            (Method::Post, "/api/v1/command") => handle_command(&engine, &mut request),
            (Method::Post, "/api/v1/terminal") => handle_terminal(&engine, &mut request),
            (Method::Get, "/api/v1/state") => handle_get_state(&engine),
            (Method::Get, path) if path.starts_with("/api/v1/state/sentence/") => {
                let idx_str = path.trim_start_matches("/api/v1/state/sentence/");
                handle_get_sentence(&engine, idx_str)
            }
            (Method::Post, "/api/v1/shutdown") => {
                shutdown.store(true, Ordering::Relaxed);
                Ok(json_response(200, &ApiResponse::<()> {
                    success: true,
                    message: "Shutdown initiated".to_string(),
                    data: None,
                }))
            }
            (Method::Get, "/api/v1/ping") => {
                Ok(json_response(200, &ApiResponse::<()> {
                    success: true,
                    message: format!("Server '{}' is alive", config.name),
                    data: None,
                }))
            }
            _ => {
                Ok(json_response(404, &ApiResponse::<()> {
                    success: false,
                    message: format!("Not found: {}", request.url()),
                    data: None,
                }))
            }
        };

        match result {
            Ok(response) => { let _ = request.respond(response); }
            Err(e) => {
                let _ = request.respond(json_response(500, &ApiResponse::<()> {
                    success: false,
                    message: e,
                    data: None,
                }));
            }
        }
    }

    println!("[SERVER] '{}' shut down.", config.name);
    Ok(())
}

/// Start the server in a background thread. Returns a handle that can be used to 
/// join or check if the server is running.
pub fn start_server_thread(
    engine: Arc<Mutex<Engine>>,
    config: ServerConfig,
) -> std::thread::JoinHandle<Result<(), String>> {
    std::thread::spawn(move || run_server(engine, config))
}

// --- Request Handlers ---

fn handle_command(
    engine: &Arc<Mutex<Engine>>,
    request: &mut tiny_http::Request,
) -> Result<Response<std::io::Cursor<Vec<u8>>>, String> {
    // Read request body
    let mut body = String::new();
    request.as_reader().read_to_string(&mut body).map_err(|e| e.to_string())?;

    // Parse as AppCommand
    let command: AppCommand = serde_json::from_str(&body)
        .map_err(|e| format!("Invalid command JSON: {}. Body was: {}", e, body))?;

    // Execute
    let mut eng = engine.lock().map_err(|_| "Engine lock poisoned".to_string())?;
    match eng.execute(command) {
        Ok(msg) => Ok(json_response(200, &ApiResponse::<()> {
            success: true,
            message: msg,
            data: None,
        })),
        Err(e) => Ok(json_response(400, &ApiResponse::<()> {
            success: false,
            message: e,
            data: None,
        })),
    }
}

fn handle_terminal(
    engine: &Arc<Mutex<Engine>>,
    request: &mut tiny_http::Request,
) -> Result<Response<std::io::Cursor<Vec<u8>>>, String> {
    // Read the raw terminal command text from the body
    let mut body = String::new();
    request.as_reader().read_to_string(&mut body).map_err(|e| e.to_string())?;
    let input = body.trim();

    let mut eng = engine.lock().map_err(|_| "Engine lock poisoned".to_string())?;

    match crate::app::terminal::run_terminal_command(&mut eng, input) {
        Ok(Some(output)) => Ok(text_response(200, &output)),
        Ok(None) => {
            // Exit command — return acknowledgment (server won't actually exit from this)
            Ok(text_response(200, "Exit requested (use /api/v1/shutdown to stop the server)"))
        }
        Err(e) => Ok(text_response(400, &format!("Error: {}", e))),
    }
}

fn handle_get_state(
    engine: &Arc<Mutex<Engine>>,
) -> Result<Response<std::io::Cursor<Vec<u8>>>, String> {
    let eng = engine.lock().map_err(|_| "Engine lock poisoned".to_string())?;
    
    let summary = build_state_summary(&eng);
    Ok(json_response(200, &ApiResponse {
        success: true,
        message: format!("{} sentences loaded", summary.sentence_count),
        data: Some(summary),
    }))
}

fn handle_get_sentence(
    engine: &Arc<Mutex<Engine>>,
    idx_str: &str,
) -> Result<Response<std::io::Cursor<Vec<u8>>>, String> {
    let eng = engine.lock().map_err(|_| "Engine lock poisoned".to_string())?;
    
    let idx: usize = idx_str.parse().map_err(|_| format!("Invalid index: {}", idx_str))?;
    
    if let Some(sentence) = eng.state.document.get(idx) {
        let summary = build_sentence_summary(idx, sentence);
        Ok(json_response(200, &ApiResponse {
            success: true,
            message: format!("Sentence {} (index {})", sentence.id, idx),
            data: Some(summary),
        }))
    } else {
        Ok(json_response(404, &ApiResponse::<()> {
            success: false,
            message: format!("Sentence index {} out of range (document has {} sentences)", idx, eng.state.document.len()),
            data: None,
        }))
    }
}

// --- Helpers ---

fn build_state_summary(eng: &Engine) -> StateSummary {
    let sentences: Vec<SentenceSummary> = eng.state.document.iter().enumerate()
        .map(|(i, s)| build_sentence_summary(i, s))
        .collect();

    StateSummary {
        sentence_count: eng.state.document.len(),
        selected_sentence_idx: eng.state.selected_sentence_idx,
        selected_range: eng.state.selected_range,
        project_languages: eng.state.project_languages.clone(),
        sentences,
    }
}

fn build_sentence_summary(index: usize, sentence: &crate::domain::sentence::Sentence) -> SentenceSummary {
    let mut tiers = std::collections::HashMap::new();
    for (tier_id, tier) in &sentence.tiers {
        tiers.insert(tier_id.clone(), TierSummary {
            state: format!("{:?}", tier.state),
            full_text: tier.full_text(),
        });
    }

    SentenceSummary {
        index,
        id: sentence.id.clone(),
        tier_count: sentence.tiers.len(),
        tiers,
        mapping_count: sentence.mappings.len(),
    }
}

fn json_response<T: Serialize>(status: u16, body: &T) -> Response<std::io::Cursor<Vec<u8>>> {
    let json = serde_json::to_string_pretty(body).unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string());
    let data = json.into_bytes();
    let header = Header::from_bytes("Content-Type", "application/json").unwrap();
    Response::new(
        StatusCode(status),
        vec![header],
        std::io::Cursor::new(data.clone()),
        Some(data.len()),
        None,
    )
}

fn text_response(status: u16, body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let data = body.as_bytes().to_vec();
    let header = Header::from_bytes("Content-Type", "text/plain; charset=utf-8").unwrap();
    Response::new(
        StatusCode(status),
        vec![header],
        std::io::Cursor::new(data.clone()),
        Some(data.len()),
        None,
    )
}
