# Command-Driven Architecture (Headless Core)

## Overview
This document outlines the architectural shift from a GUI-centric application to a **Command-Driven** architecture. The goal is to separate the core business logic and state management from the presentation layer (GUI/CLI), enabling headless testing, scripting, and dual-interface control (GUI + Terminal).

## Core Concepts

### 1. The Engine (Model)
*   **State Owner:** Holds the `AppState` (Document, Settings, UI Context like selection).
*   **Command Processor:** Exposes a single entry point `execute(command: Command) -> Result`.
*   **Platform Agnostic:** Knows nothing about `egui`, `crossterm`, or pixels.

### 2. The Command Layer
*   **Intent Definition:** All user actions are represented as data structures (Enums/Structs).
*   **Serialization:** Commands can be serialized to/from string/JSON, enabling scripting and CLI driving.
*   **Scope:** Commands cover all business logic (Navigation, Editing, Generation, Configuration).

### 3. The Views (Presentation)
*   **GUI (`egui`):** Renders the state visually. Buttons construct `Command` objects and send them to the Engine.
*   **Terminal (CLI):** Renders the state textually. Parses text input into `Command` objects and sends them to the Engine.
*   **Solver/Test Script:** A python script that spawns the CLI process, writes commands to `stdin`, parses `stdout`, and asserts state changes.

## Architecture Diagram

```mermaid
graph TD
    UserGUI[User (GUI)] -->|Click| GUI[GUI Layer]
    UserCLI[User (Terminal)] -->|Type| CLI[CLI Layer]
    Agent[AI Agent] -->|Script| CLI

    GUI -->|Construct| Cmd[Command Object]
    CLI -->|Construct| Cmd

    Cmd --> Engine[WeaveLang Engine]
    Engine -->|Modify| State[App State]
    
    State -->|Render| GUI
    State -->|Print| CLI
```

## Implementation Strategy

1.  **Refactor `AppState`:** Ensure it contains all necessary context (selection indices, active tiers) that is currently scattered in UI code.
2.  **Define `Command` Enum:** Create a comprehensive Enum covering all actions.
3.  **Extract Logic:** Move logic from `app.rs` / `text_view.rs` into `Engine::handle_command()`.
4.  **Build CLI REPL:** Create a simple read-eval-print loop that accepts text commands.
5.  **Connect GUI:** Update GUI to use the Command system instead of direct mutation.
