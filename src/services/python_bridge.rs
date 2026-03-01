use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

// --- NEW STRUCT ---
#[derive(Debug, Deserialize, Clone)]
pub struct RawSpacyToken {
    pub text: String,
    pub lemma: String,
    pub pos: String,
    pub is_punct: bool,
    pub is_space: bool,
    pub whitespace: String,
}

#[derive(Serialize)]
struct TokenizeRequest {
    action: String,
    text: String,
    lang: String,
}

#[derive(Deserialize, Debug)]
struct TokenizeResponse {
    status: String,
    tokens: Option<Vec<RawSpacyToken>>, // Updated type
    message: Option<String>,
}

#[derive(Serialize)]
struct SegmentRequest {
    action: String,
    text: String,
    lang: String,
    engine: String,
}

#[derive(Deserialize, Debug)]
struct SegmentResponse {
    status: String,
    sentences: Option<Vec<String>>,
    message: Option<String>,
}

pub struct PythonBridge {
    process: Child,
}

impl PythonBridge {
    pub fn new(project_root: PathBuf) -> Result<Self, String> {
        let script_path = project_root.join("src/python/linguistic_engine.py");

        // Prefer venv Python if available
        let python_exe = Self::find_python(&project_root);

        let child = Command::new(&python_exe)
            .arg(script_path)
            .env("PYTHONUTF8", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Failed to spawn Python process ({python_exe}): {e}"))?;

        Ok(Self { process: child })
    }

    /// Look for a venv Python first, then fall back to system `python`.
    fn find_python(project_root: &Path) -> String {
        // Windows: .venv/Scripts/python.exe
        let venv_win = project_root.join(".venv/Scripts/python.exe");
        if venv_win.exists() {
            return venv_win.to_string_lossy().into_owned();
        }
        // Unix: .venv/bin/python
        let venv_unix = project_root.join(".venv/bin/python");
        if venv_unix.exists() {
            return venv_unix.to_string_lossy().into_owned();
        }
        "python".to_string()
    }

    // Updated Return Type
    pub fn tokenize(&mut self, text: &str, lang_code: &str) -> Result<Vec<RawSpacyToken>, String> {
        let req = TokenizeRequest {
            action: "tokenize".to_string(),
            text: text.to_string(),
            lang: lang_code.to_string(),
        };

        let json_input = serde_json::to_string(&req).map_err(|e| e.to_string())?;

        let stdin = self
            .process
            .stdin
            .as_mut()
            .ok_or("Failed to access Python stdin")?;
        writeln!(stdin, "{json_input}").map_err(|e| e.to_string())?;

        let stdout = self
            .process
            .stdout
            .as_mut()
            .ok_or("Failed to access Python stdout")?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();

        reader.read_line(&mut line).map_err(|e| e.to_string())?;

        if line.trim().is_empty() {
            return Err("Python process returned empty response (crashed?)".to_string());
        }

        let resp: TokenizeResponse = serde_json::from_str(&line)
            .map_err(|e| format!("Failed to parse Python response: {e}. Raw: {line}"))?;

        if resp.status == "success" {
            Ok(resp.tokens.unwrap_or_default())
        } else {
            Err(resp
                .message
                .unwrap_or_else(|| "Unknown Python error".to_string()))
        }
    }

    pub fn segment(&mut self, text: &str, lang_code: &str) -> Result<Vec<String>, String> {
        let req = SegmentRequest {
            action: "segment".to_string(),
            text: text.to_string(),
            lang: lang_code.to_string(),
            engine: "stanza".to_string(),
        };

        let json_input = serde_json::to_string(&req).map_err(|e| e.to_string())?;

        let stdin = self
            .process
            .stdin
            .as_mut()
            .ok_or("Failed to access Python stdin")?;
        writeln!(stdin, "{json_input}").map_err(|e| e.to_string())?;

        let stdout = self
            .process
            .stdout
            .as_mut()
            .ok_or("Failed to access Python stdout")?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();

        reader.read_line(&mut line).map_err(|e| e.to_string())?;

        if line.trim().is_empty() {
            return Err("Python process returned empty response (crashed?)".to_string());
        }

        let resp: SegmentResponse = serde_json::from_str(&line)
            .map_err(|e| format!("Failed to parse Python response: {e}. Raw: {line}"))?;

        if resp.status == "success" {
            Ok(resp.sentences.unwrap_or_default())
        } else {
            Err(resp
                .message
                .unwrap_or_else(|| "Unknown Python error".to_string()))
        }
    }
}

impl Drop for PythonBridge {
    fn drop(&mut self) {
        let _ = self.process.kill();
    }
}

#[derive(Clone)]
pub struct BridgeService {
    internal: Arc<Mutex<PythonBridge>>,
}

impl BridgeService {
    pub fn new(project_root: PathBuf) -> Result<Self, String> {
        let bridge = PythonBridge::new(project_root)?;
        Ok(Self {
            internal: Arc::new(Mutex::new(bridge)),
        })
    }

    pub fn tokenize(&self, text: &str, lang_code: &str) -> Result<Vec<RawSpacyToken>, String> {
        let mut guard = self
            .internal
            .lock()
            .map_err(|_| "Failed to lock Python Bridge")?;
        guard.tokenize(text, lang_code)
    }

    pub fn segment(&self, text: &str, lang_code: &str) -> Result<Vec<String>, String> {
        let mut guard = self
            .internal
            .lock()
            .map_err(|_| "Failed to lock Python Bridge")?;
        guard.segment(text, lang_code)
    }
}
