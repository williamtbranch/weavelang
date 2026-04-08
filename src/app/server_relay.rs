// src/app/server_relay.rs
//
// Relay server for co-pilot mode: an HTTP server that runs in a background thread
// and relays commands to the GUI thread via channels.  The GUI drains requests each
// frame, executes them on its owned Engine, and sends responses back.
//
// This avoids putting the Engine behind Arc<Mutex<>> — the GUI keeps its simple
// ownership model and the server just acts as a message bridge.

use std::sync::mpsc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tiny_http::{Server, Response, Header, Method, StatusCode};

// ── Types ──────────────────────────────────────────────────────────────────

/// What the HTTP server asks the GUI to do.
#[derive(Debug)]
pub enum RelayRequest {
    /// Execute a terminal command string (same as the interactive REPL).
    Terminal(String),
    /// Serialize full document state as JSON.
    GetState,
    /// Serialize a single sentence.
    GetSentence(usize),
    /// Health check.
    Ping,
    /// Shut down the relay server.
    Shutdown,
}

/// What the GUI sends back.
pub struct RelayResponse {
    pub status: u16,
    pub content_type: ContentType,
    pub body: String,
}

#[derive(Clone, Copy)]
pub enum ContentType {
    Json,
    Text,
}

/// Configuration for the relay server.
pub struct RelayConfig {
    pub port: u16,
    pub name: String,
}

/// A pending request waiting for the GUI to handle it.
pub struct PendingRelayRequest {
    pub request: RelayRequest,
    pub response_tx: mpsc::Sender<RelayResponse>,
}

/// Receiver the GUI drains each frame.
pub type RelayReceiver = mpsc::Receiver<PendingRelayRequest>;

// ── Public API ─────────────────────────────────────────────────────────────

/// Start the relay server in a background thread.
/// Returns the receiver the GUI should drain and the thread handle.
pub fn start_relay_server(
    config: RelayConfig,
) -> Result<(RelayReceiver, std::thread::JoinHandle<()>), String> {
    let (tx, rx) = mpsc::channel::<PendingRelayRequest>();

    let handle = std::thread::Builder::new()
        .name(format!("copilot-server-{}", config.name))
        .spawn(move || {
            run_relay_loop(tx, config);
        })
        .map_err(|e| format!("Failed to spawn relay server thread: {}", e))?;

    Ok((rx, handle))
}

// ── Server Loop ────────────────────────────────────────────────────────────

fn run_relay_loop(
    relay_tx: mpsc::Sender<PendingRelayRequest>,
    config: RelayConfig,
) {
    let addr = format!("127.0.0.1:{}", config.port);
    let server = match Server::http(&addr) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[COPILOT] Failed to bind to {}: {}", addr, e);
            return;
        }
    };

    eprintln!(
        "[COPILOT] Server '{}' listening on http://127.0.0.1:{}",
        config.name, config.port
    );

    let shutdown = Arc::new(AtomicBool::new(false));

    for mut request in server.incoming_requests() {
        if shutdown.load(Ordering::Relaxed) {
            let _ = request.respond(make_text_response(200, "Server shutting down"));
            break;
        }

        let url = request.url().to_string();
        let method = request.method().clone();

        // ── Route to a RelayRequest ────────────────────────────────────
        let relay_req = match (&method, url.as_str()) {
            (Method::Post, "/api/v1/terminal") => {
                let mut body = String::new();
                if request.as_reader().read_to_string(&mut body).is_err() {
                    let _ = request.respond(make_text_response(400, "Failed to read body"));
                    continue;
                }
                RelayRequest::Terminal(body.trim().to_string())
            }
            (Method::Get, "/api/v1/state") => RelayRequest::GetState,
            (Method::Get, path) if path.starts_with("/api/v1/state/sentence/") => {
                let idx_str = path.trim_start_matches("/api/v1/state/sentence/");
                match idx_str.parse::<usize>() {
                    Ok(idx) => RelayRequest::GetSentence(idx),
                    Err(_) => {
                        let _ = request.respond(make_text_response(
                            400,
                            &format!("Invalid index: {}", idx_str),
                        ));
                        continue;
                    }
                }
            }
            (Method::Get, "/api/v1/ping") => RelayRequest::Ping,
            (Method::Post, "/api/v1/shutdown") => {
                shutdown.store(true, Ordering::Relaxed);
                RelayRequest::Shutdown
            }
            _ => {
                let _ = request.respond(make_text_response(
                    404,
                    &format!("Not found: {} {}", method, url),
                ));
                continue;
            }
        };

        // ── Send to GUI and await response ─────────────────────────────
        let (resp_tx, resp_rx) = mpsc::channel::<RelayResponse>();
        if relay_tx
            .send(PendingRelayRequest {
                request: relay_req,
                response_tx: resp_tx,
            })
            .is_err()
        {
            let _ = request.respond(make_text_response(503, "GUI not available"));
            break; // main thread dropped the receiver — shut down
        }

        // 10-minute timeout: commands like `calibrate` can block the GUI thread
        // for extended periods. The copilot agent uses `job_status` / `wait` for
        // LLM jobs, but synchronous commands need a generous timeout here.
        match resp_rx.recv_timeout(std::time::Duration::from_secs(600)) {
            Ok(response) => {
                let http_resp = match response.content_type {
                    ContentType::Json => make_json_response(response.status, &response.body),
                    ContentType::Text => make_text_response(response.status, &response.body),
                };
                let _ = request.respond(http_resp);
            }
            Err(_) => {
                let _ = request.respond(make_text_response(
                    504,
                    "Timeout: GUI did not respond within 600 seconds",
                ));
            }
        }

        if shutdown.load(Ordering::Relaxed) {
            break;
        }
    }

    eprintln!("[COPILOT] Server '{}' shut down.", config.name);
}

// ── HTTP Helpers ───────────────────────────────────────────────────────────

/// Standard security headers applied to every response.
fn security_headers() -> Vec<Header> {
    vec![
        Header::from_bytes("Access-Control-Allow-Origin", "http://127.0.0.1").unwrap(),
        Header::from_bytes("Access-Control-Allow-Methods", "GET, POST").unwrap(),
        Header::from_bytes("Access-Control-Allow-Headers", "Content-Type").unwrap(),
        Header::from_bytes("X-Content-Type-Options", "nosniff").unwrap(),
    ]
}

fn make_json_response(
    status: u16,
    json_body: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let data = json_body.as_bytes().to_vec();
    let mut headers = security_headers();
    headers.push(Header::from_bytes("Content-Type", "application/json").unwrap());
    Response::new(
        StatusCode(status),
        headers,
        std::io::Cursor::new(data.clone()),
        Some(data.len()),
        None,
    )
}

fn make_text_response(
    status: u16,
    body: &str,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let data = body.as_bytes().to_vec();
    let mut headers = security_headers();
    headers.push(Header::from_bytes("Content-Type", "text/plain; charset=utf-8").unwrap());
    Response::new(
        StatusCode(status),
        headers,
        std::io::Cursor::new(data.clone()),
        Some(data.len()),
        None,
    )
}
