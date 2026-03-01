use std::io::{self, Write};
// commands module used indirectly through app::terminal
use weavelang_rust_gui::app::engine::Engine;
use weavelang_rust_gui::app::server::{self, ServerConfig};
use weavelang_rust_gui::app::state::AppState;
use weavelang_rust_gui::services::python_bridge::BridgeService;
use weavelang_rust_gui::services::llm_client::LlmService;
use weavelang_rust_gui::services::prompt_manager::PromptManager;
use weavelang_rust_gui::services::llm_logger::LlmLogger;
use weavelang_rust_gui::simulation::frequency_manager;
use std::sync::{Arc, Mutex};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Dispatch based on first argument
    if args.len() > 1 {
        match args[1].as_str() {
            "daemon" => handle_daemon(&args[2..]),
            "send" => handle_send(&args[2..]),
            "repl" => run_repl(),
            "help" | "--help" | "-h" => print_usage(),
            _ => {
                eprintln!("Unknown subcommand: {}. Use 'help' for usage.", args[1]);
                std::process::exit(1);
            }
        }
    } else {
        // Default: interactive REPL
        run_repl();
    }
}

fn print_usage() {
    println!("WeaveLang CLI v0.1");
    println!();
    println!("USAGE:");
    println!("  weavelang_cli                          Interactive REPL (default)");
    println!("  weavelang_cli daemon start [OPTIONS]   Start a headless API server");
    println!("  weavelang_cli daemon list              List running servers");
    println!("  weavelang_cli daemon kill <name>       Shut down a named server");
    println!("  weavelang_cli send <target> <json>     Send a command to a server");
    println!();
    println!("DAEMON OPTIONS:");
    println!("  --name <name>    Server name (default: 'default')");
    println!("  --port <port>    Port to listen on (default: 3030)");
    println!();
    println!("SEND TARGET:");
    println!("  <name>           Server name (looks up port from registry)");
    println!("  :<port>          Direct port number (e.g. :3030)");
    println!();
    println!("EXAMPLES:");
    println!(r#"  weavelang_cli daemon start --name test --port 3031"#);
    println!(r#"  weavelang_cli send test '{{"ImportSource":{{"path":"test_case/source.txt"}}}}'"#);
    println!(r#"  weavelang_cli daemon kill test"#);
}

// --- Server Registry ---

fn registry_path() -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    p.push("weavelang_servers.json");
    p
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
struct ServerEntry {
    name: String,
    port: u16,
    pid: u32,
}

fn read_registry() -> Vec<ServerEntry> {
    let path = registry_path();
    if let Ok(content) = std::fs::read_to_string(&path) {
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        Vec::new()
    }
}

fn write_registry(entries: &[ServerEntry]) {
    let path = registry_path();
    if let Ok(json) = serde_json::to_string_pretty(entries) {
        let _ = std::fs::write(&path, json);
    }
}

fn register_server(name: &str, port: u16) {
    let mut entries = read_registry();
    // Remove any stale entry with the same name
    entries.retain(|e| e.name != name);
    entries.push(ServerEntry {
        name: name.to_string(),
        port,
        pid: std::process::id(),
    });
    write_registry(&entries);
}

fn unregister_server(name: &str) {
    let mut entries = read_registry();
    entries.retain(|e| e.name != name);
    write_registry(&entries);
}

fn find_server_port(name: &str) -> Option<u16> {
    let entries = read_registry();
    entries.iter().find(|e| e.name == name).map(|e| e.port)
}

// --- Daemon Commands ---

fn handle_daemon(args: &[String]) {
    if args.is_empty() {
        eprintln!("Usage: weavelang_cli daemon <start|list|kill> [OPTIONS]");
        std::process::exit(1);
    }

    match args[0].as_str() {
        "start" => daemon_start(&args[1..]),
        "list" => daemon_list(),
        "kill" => {
            if args.len() < 2 {
                eprintln!("Usage: weavelang_cli daemon kill <name>");
                std::process::exit(1);
            }
            daemon_kill(&args[1]);
        }
        _ => {
            eprintln!("Unknown daemon command: {}. Use start, list, or kill.", args[0]);
            std::process::exit(1);
        }
    }
}

fn daemon_start(args: &[String]) {
    let mut name = "default".to_string();
    let mut port: u16 = 3030;
    let mut test_mode_path: Option<String> = None;

    // Simple arg parsing
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--name" => {
                i += 1;
                if i < args.len() { name = args[i].clone(); }
            }
            "--port" => {
                i += 1;
                if i < args.len() { port = args[i].parse().unwrap_or(3030); }
            }
            "--test-mode" => {
                i += 1;
                if i < args.len() { test_mode_path = Some(args[i].clone()); }
            }
            other => {
                // Accept first positional arg as name
                if !other.starts_with('-') && name == "default" {
                    name = other.to_string();
                }
            }
        }
        i += 1;
    }

    if test_mode_path.is_some() {
        println!("[DAEMON] Starting server '{}' on port {} [TEST MODE]...", name, port);
    } else {
        println!("[DAEMON] Starting server '{}' on port {}...", name, port);
    }

    // Initialize services
    let state = init_state_with_services_opt(test_mode_path.as_deref());
    let engine = Engine::new(state);

    // Register in the server list
    register_server(&name, port);

    let config = ServerConfig { port, name: name.clone() };
    let engine_arc = Arc::new(Mutex::new(engine));

    // Run the server (blocks until shutdown)
    if let Err(e) = server::run_server(engine_arc, config) {
        eprintln!("[DAEMON] Server error: {}", e);
    }

    // Clean up registry on exit
    unregister_server(&name);
}

fn daemon_list() {
    let entries = read_registry();
    if entries.is_empty() {
        println!("No registered servers.");
        return;
    }

    println!("{:<15} {:<8} {:<8} {}", "NAME", "PORT", "PID", "STATUS");
    println!("{}", "-".repeat(50));

    for entry in &entries {
        // Ping the server to check if it's alive
        let status = match ping_server(entry.port) {
            Ok(msg) => format!("ALIVE ({})", msg),
            Err(_) => "DEAD".to_string(),
        };
        println!("{:<15} {:<8} {:<8} {}", entry.name, entry.port, entry.pid, status);
    }
}

fn daemon_kill(name: &str) {
    if let Some(port) = find_server_port(name) {
        println!("Sending shutdown to server '{}' on port {}...", name, port);
        match send_to_server(port, "POST", "/api/v1/shutdown", None) {
            Ok(response) => {
                println!("{}", response);
                unregister_server(name);
            }
            Err(e) => {
                eprintln!("Failed to contact server: {}", e);
                eprintln!("Removing stale registry entry.");
                unregister_server(name);
            }
        }
    } else {
        eprintln!("No server found with name '{}'", name);
    }
}

// --- Send Command ---

fn handle_send(args: &[String]) {
    if args.len() < 2 {
        eprintln!("Usage: weavelang_cli send <name|:port> <subcommand>");
        eprintln!();
        eprintln!("Subcommands:");
        eprintln!("  command <terminal cmd>   Send a terminal command (same as interactive REPL)");
        eprintln!("  state                    GET full state as JSON");
        eprintln!("  state/sentence/N         GET a specific sentence as JSON");
        eprintln!("  ping                     Check if server is alive");
        eprintln!("  <json>                   Send a raw AppCommand as JSON");
        std::process::exit(1);
    }

    let target = &args[0];
    let command_str = args[1..].join(" ");

    // Resolve port
    let port = if target.starts_with(':') {
        target[1..].parse::<u16>().unwrap_or_else(|_| {
            eprintln!("Invalid port: {}", target);
            std::process::exit(1);
        })
    } else {
        find_server_port(target).unwrap_or_else(|| {
            eprintln!("No server found with name '{}'. Run 'daemon list' to see active servers.", target);
            std::process::exit(1);
        })
    };

    // Decide routing based on subcommand
    if command_str.starts_with("command ") {
        // Terminal command: send the raw text after "command " to /api/v1/terminal
        let terminal_input = &command_str["command ".len()..];
        match send_to_server(port, "POST", "/api/v1/terminal", Some(terminal_input)) {
            Ok(response) => println!("{}", response),
            Err(e) => eprintln!("Error: {}", e),
        }
    } else if command_str == "state" {
        match send_to_server(port, "GET", "/api/v1/state", None) {
            Ok(response) => println!("{}", response),
            Err(e) => eprintln!("Error: {}", e),
        }
    } else if command_str.starts_with("state/sentence/") {
        let path = format!("/api/v1/{}", command_str);
        match send_to_server(port, "GET", &path, None) {
            Ok(response) => println!("{}", response),
            Err(e) => eprintln!("Error: {}", e),
        }
    } else if command_str == "ping" {
        match ping_server(port) {
            Ok(msg) => println!("{}", msg),
            Err(e) => eprintln!("Error: {}", e),
        }
    } else {
        // Treat as AppCommand JSON
        match send_to_server(port, "POST", "/api/v1/command", Some(&command_str)) {
            Ok(response) => println!("{}", response),
            Err(e) => eprintln!("Error: {}", e),
        }
    }
}

// --- HTTP Client Helpers ---

fn ping_server(port: u16) -> Result<String, String> {
    send_to_server(port, "GET", "/api/v1/ping", None)
}

fn send_to_server(port: u16, method: &str, path: &str, body: Option<&str>) -> Result<String, String> {
    let url = format!("http://127.0.0.1:{}{}", port, path);
    
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let response = match method {
        "GET" => client.get(&url).send().map_err(|e| e.to_string())?,
        "POST" => {
            let mut req = client.post(&url).header("Content-Type", "application/json");
            if let Some(b) = body {
                req = req.body(b.to_string());
            }
            req.send().map_err(|e| e.to_string())?
        }
        _ => return Err(format!("Unsupported HTTP method: {}", method)),
    };

    response.text().map_err(|e| e.to_string())
}

// --- Interactive REPL ---

fn init_state_with_services() -> AppState {
    init_state_with_services_opt(None)
}

fn init_state_with_services_opt(test_mode_dir: Option<&str>) -> AppState {
    let mut state = AppState::default();

    if let Ok(cwd) = std::env::current_dir() {
        // Load frequency list
        let freq_list_path = cwd.join("assets/frequency_lists/es_master_frequency_list.txt");
        if freq_list_path.exists() {
            if let Err(e) = frequency_manager::load_master_frequency_list(&freq_list_path) {
                eprintln!("[WARN] Failed to load frequency list: {}", e);
            }
        } else {
            eprintln!("[WARN] Frequency list not found at {:?}", freq_list_path);
        }

        match BridgeService::new(cwd.clone()) {
            Ok(b) => {
                eprintln!("[INFO] Python Bridge initialized.");
                state.bridge = Some(b);
            }
            Err(e) => eprintln!("[WARN] Bridge Error: {}", e),
        }

        // LLM Service: use MockLlmProvider in test mode, RealLlmProvider otherwise
        if let Some(test_dir) = test_mode_dir {
            let responses_dir = std::path::PathBuf::from(test_dir).join("LLM_responses");
            let mock = weavelang_rust_gui::services::mock_llm::MockLlmProvider::new(responses_dir);
            let svc = LlmService::from_provider(Box::new(mock));
            eprintln!("[INFO] LLM Service initialized (TEST MODE: {:?})", test_dir);
            state.llm = Some(svc);
        } else {
            match LlmService::new(Some(cwd.clone())) {
                Ok(s) => {
                    eprintln!("[INFO] LLM Service initialized.");
                    state.llm = Some(s);
                }
                Err(e) => eprintln!("[WARN] LLM Service Error: {}", e),
            }
        }

        state.prompts = Some(PromptManager::new(cwd.clone()));
        state.logger = Some(LlmLogger::new(cwd.clone()));
        
        // Load Config
        let config_path = cwd.join("config.toml");
        match weavelang_rust_gui::config::load_config_from_file(config_path.to_str().unwrap_or("config.toml")) {
            Ok(cfg) => {
                 eprintln!("[INFO] Config loaded.");
                 state.config = Some(cfg);
            }
            Err(e) => eprintln!("[WARN] Config Load Error: {}", e),
        }
    }

    state
}

fn run_repl() {
    println!("WeaveLang CLI v0.1");
    println!("Type 'help' for commands.");

    let state = init_state_with_services();
    let mut engine = Engine::new(state);

    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut input_buffer = String::new();

    loop {
        print!("> ");
        stdout.flush().unwrap();
        input_buffer.clear();
        
        match stdin.read_line(&mut input_buffer) {
            Ok(0) => break, // EOF
            Ok(_) => {
                let input = input_buffer.trim();
                if input.is_empty() {
                    continue;
                }
                
                match weavelang_rust_gui::app::terminal::run_terminal_command(&mut engine, input) {
                    Ok(Some(output)) => println!("{}", output),
                    Ok(None) => std::process::exit(0), // Exit command
                    Err(e) => println!("Error: {}", e),
                }
            }
            Err(e) => {
                println!("Error reading input: {}", e);
                break;
            }
        }
    }
}
