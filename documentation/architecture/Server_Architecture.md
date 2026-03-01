# WeaveLang Meta-Architecture: AI-Driven Testing & Local API Server

## 1. The Core Philosophy
To enable robust, interactive, and automated testing by AI agents (and human scripts), WeaveLang adopts a **Local API Server / Headless Engine** architecture. 

Instead of relying on brittle terminal text scraping or interactive CLI piping, AI agents will interact with WeaveLang via a deterministic HTTP/JSON API. Because the core business logic (`Engine`) is fully decoupled from the presentation layers (GUI and CLI) via the Command Pattern (`AppCommand`), we can expose the `Engine` over a local network port.

## 2. Architectural Components

### A. The Shared Engine
The `Engine` struct holds the `AppState` and processes `AppCommand`s. It is the single source of truth.
*   **GUI Mode**: Instantiates the `Engine` locally for 60fps rendering (eframe/egui). It *also* spawns the API Server in a background thread, sharing the `Engine` via `Arc<Mutex<Engine>>`. This allows an AI to drive and inspect the GUI while a human watches.
*   **Daemon Mode (Headless)**: The CLI can spawn the `Engine` purely in memory, wrapped only by the API Server.

### B. The Local API Server
A lightweight HTTP server (e.g., using `axum` or `tiny_http`) that exposes the `Engine` to the local machine.
*   **Protocol**: HTTP + JSON.
*   **Endpoints**:
    *   `POST /api/v1/command`: Accepts a JSON-serialized `AppCommand`. Returns a success/error string.
    *   `GET /api/v1/state`: Returns the full `AppState` as JSON.
    *   `GET /api/v1/state/sentence/{id}`: Returns the state of a specific sentence.
    *   `POST /api/v1/shutdown`: Gracefully terminates the server process.

### C. Server Registry & Management
To manage multiple headless instances (e.g., for parallel testing or distinct test environments), WeaveLang maintains a local registry (e.g., in the system's temp or app data directory, like `~/.weavelang/servers.json`).
*   Tracks: `Name`, `Port`, `PID`, `Status`.

### D. CLI Daemon Commands
The CLI acts as the manager for headless instances:
*   `weavelang daemon start --name <name>`: Spawns a detached headless server, finds an open port, writes to the registry, and returns the port.
*   `weavelang daemon list`: Reads the registry, pings ports to verify they are alive, and lists active servers.
*   `weavelang daemon kill <name>`: Sends the `/shutdown` command to the specified server and removes it from the registry.
*   `weavelang send <name> <command_json>`: A utility to send a command to a named server without writing a custom HTTP script.

## 3. AI Testing Workflow
Future AI sessions will use this workflow to test new features or debug issues:

1.  **Setup**: The AI runs `weavelang daemon start --name ai_test_env`.
2.  **Action**: The AI sends an HTTP POST request with an `AppCommand` (e.g., `ImportSource { path: "test_case/source.txt" }`).
3.  **Verification**: The AI sends an HTTP GET request to `/api/v1/state/sentence/2` to verify the text was imported and parsed correctly.
4.  **Iteration**: The AI can trigger LLM generations, poll the state until the job completes, and verify the output tiers.
5.  **Teardown**: The AI runs `weavelang daemon kill ai_test_env`.

## 4. Implementation Roadmap
To realize this architecture, the following steps must be taken:
1.  **Serialization**: Ensure `AppCommand` and all its variants derive `Serialize` and `Deserialize`.
2.  **Server Module**: Create `src/app/server.rs` implementing the HTTP listener and routing logic, wrapping `Arc<Mutex<Engine>>`.
3.  **CLI Updates**: Add the `daemon` subcommand suite to `src/cli/main.rs` (or a dedicated CLI module).
4.  **GUI Integration**: Update `src/gui/app.rs` to optionally spawn the server thread on startup.
5.  **Test Scripts**: Create Python helper scripts in `test_case/` to easily wrap the HTTP calls for the AI.