use std::process::{Command, Child, Stdio};
use std::io::{Write, BufReader, BufRead};
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use serde::{Deserialize, Serialize};

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

pub struct PythonBridge {
    process: Child,
}

impl PythonBridge {
    pub fn new(project_root: PathBuf) -> Result<Self, String> {
        let script_path = project_root.join("src/python/linguistic_engine.py");
        
        let child = Command::new("python")
            .arg(script_path)
            .env("PYTHONUTF8", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) 
            .spawn()
            .map_err(|e| format!("Failed to spawn Python process: {}", e))?;

        Ok(Self { process: child })
    }

    // Updated Return Type
    pub fn tokenize(&mut self, text: &str, lang_code: &str) -> Result<Vec<RawSpacyToken>, String> {
        let req = TokenizeRequest {
            action: "tokenize".to_string(),
            text: text.to_string(),
            lang: lang_code.to_string(),
        };

        let json_input = serde_json::to_string(&req).map_err(|e| e.to_string())?;
        
        let stdin = self.process.stdin.as_mut().ok_or("Failed to access Python stdin")?;
        writeln!(stdin, "{}", json_input).map_err(|e| e.to_string())?;
        
        let stdout = self.process.stdout.as_mut().ok_or("Failed to access Python stdout")?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        
        if line.trim().is_empty() {
            return Err("Python process returned empty response (crashed?)".to_string());
        }

        let resp: TokenizeResponse = serde_json::from_str(&line)
            .map_err(|e| format!("Failed to parse Python response: {}. Raw: {}", e, line))?;

        if resp.status == "success" {
            Ok(resp.tokens.unwrap_or_default())
        } else {
            Err(resp.message.unwrap_or_else(|| "Unknown Python error".to_string()))
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
        let mut guard = self.internal.lock().map_err(|_| "Failed to lock Python Bridge")?;
        guard.tokenize(text, lang_code)
    }
}