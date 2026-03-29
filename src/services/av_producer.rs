// src/services/av_producer.rs
//
// AV Production service: manages the _av_manifest.toml, scans for woven text / audio / video
// file status, and spawns Python subprocesses for TTS audio and video generation.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ---------------------------------------------------------------------------
// Manifest types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvManifest {
    pub tts: TtsConfig,
    pub video: VideoConfig,
    #[serde(default)]
    pub illustrations: IllustrationsConfig,
    pub files: FilesConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsConfig {
    pub service: String,
    pub model: String,
    pub voices: Vec<String>,
    pub prompt_prefix: String,
    pub use_vertex_auth: bool,
    pub output_format: String,
    pub chunk_max_chars: u32,
    pub max_api_retries: u32,
    pub retry_delay: u32,
    pub concurrent_requests: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoConfig {
    pub image_duration: u32,
    pub frame_rate: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IllustrationsConfig {
    pub style_prefix: String,
    pub prompt_model: String,
    pub image_model: String,
    pub image_size: String,
    pub sentences_per_illustration: u32,
    pub minimum_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilesConfig {
    #[serde(default)]
    pub marked: Vec<String>,
}

impl Default for TtsConfig {
    fn default() -> Self {
        Self {
            service: "gemini".to_string(),
            model: "models/gemini-2.5-pro-preview-tts".to_string(),
            voices: vec![
                "Charon".to_string(),
                "aoede".to_string(),
                "Puck".to_string(),
                "Zephyr".to_string(),
                "Fenrir".to_string(),
                "Kore".to_string(),
                "Orus".to_string(),
                "Leda".to_string(),
            ],
            prompt_prefix: String::new(),
            use_vertex_auth: false,
            output_format: "wav".to_string(),
            chunk_max_chars: 4500,
            max_api_retries: 1,
            retry_delay: 10,
            concurrent_requests: 1,
        }
    }
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            image_duration: 8,
            frame_rate: 30,
        }
    }
}

impl Default for IllustrationsConfig {
    fn default() -> Self {
        Self {
            style_prefix: "fairy tale watercolor, storybook illustration, warm lighting".to_string(),
            prompt_model: "gemini-2.5-flash".to_string(),
            image_model: "imagen-4.0-generate-001".to_string(),
            image_size: "1792x1024".to_string(),
            sentences_per_illustration: 50,
            minimum_count: 3,
        }
    }
}

impl Default for FilesConfig {
    fn default() -> Self {
        Self { marked: Vec::new() }
    }
}

impl Default for AvManifest {
    fn default() -> Self {
        Self {
            tts: TtsConfig::default(),
            video: VideoConfig::default(),
            illustrations: IllustrationsConfig::default(),
            files: FilesConfig::default(),
        }
    }
}

pub const AV_MANIFEST_FILENAME: &str = "_av_manifest.toml";

impl AvManifest {
    /// Load manifest from `_av_manifest.toml` inside the given book directory.
    /// Returns `None` if the file doesn't exist.
    pub fn load(book_dir: &Path) -> Result<Option<Self>, String> {
        let path = book_dir.join(AV_MANIFEST_FILENAME);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        let manifest: AvManifest = toml::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;
        Ok(Some(manifest))
    }

    /// Save manifest to `_av_manifest.toml` inside the given book directory.
    pub fn save(&self, book_dir: &Path) -> Result<(), String> {
        let path = book_dir.join(AV_MANIFEST_FILENAME);
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize manifest: {}", e))?;
        fs::write(&path, content)
            .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
        Ok(())
    }

    /// Load existing manifest or create a default one and save it.
    ///
    /// When creating a new manifest for a chapter directory, the parent
    /// (book-level) directory is checked first.  If a `_av_manifest.toml`
    /// exists there it is used as the template instead of the built-in
    /// defaults, so users can set their preferences once at the book level.
    pub fn load_or_create(book_dir: &Path) -> Result<Self, String> {
        match Self::load(book_dir)? {
            Some(m) => Ok(m),
            None => {
                // Try parent directory as book-level template
                let m = book_dir.parent()
                    .and_then(|parent| Self::load(parent).ok().flatten())
                    .unwrap_or_default();
                m.save(book_dir)?;
                Ok(m)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// File status
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct AvFileStatus {
    pub stem: String,
    pub marked: bool,
    pub has_text: bool,
    pub has_audio: bool,
    pub has_video: bool,
}

/// Status of a single audio chunk within a stem's chunks directory.
#[derive(Debug, Clone)]
pub struct ChunkStatus {
    /// Zero-based chunk index (matches `temp_chunk_NNNN` naming).
    pub index: u32,
    /// `temp_chunk_NNNN.txt` exists.
    pub has_text: bool,
    /// `temp_chunk_NNNN.wav` (or `_silence.wav`) exists.
    pub has_audio: bool,
    /// `temp_chunk_NNNN.wav.bad` exists (user rejected this chunk).
    pub is_rejected: bool,
}

// ---------------------------------------------------------------------------
// AV Producer
// ---------------------------------------------------------------------------

pub struct AvProducer {
    pub book_dir: PathBuf,
    pub manifest: AvManifest,
}

impl AvProducer {
    /// Create a producer for the given book directory, loading or creating the manifest.
    pub fn new(book_dir: PathBuf) -> Result<Self, String> {
        if !book_dir.exists() {
            return Err(format!("Book directory does not exist: {}", book_dir.display()));
        }
        let manifest = AvManifest::load_or_create(&book_dir)?;
        Ok(Self { book_dir, manifest })
    }

    /// Resolve a stem to its text file path, checking tts_files/ as fallback.
    fn resolve_text_file(&self, stem: &str) -> PathBuf {
        let direct = self.book_dir.join(format!("{}.txt", stem));
        if direct.exists() {
            return direct;
        }
        let in_tts = self.book_dir.join("tts_files").join(format!("{}.txt", stem));
        if in_tts.exists() {
            return in_tts;
        }
        direct // fall back to direct path for error messages
    }

    /// Scan the book directory and return status for every woven text file found.
    pub fn scan(&self) -> Vec<AvFileStatus> {
        let audio_dir = self.book_dir.join("audio");
        let video_dir = self.book_dir.join("video");
        let audio_ext = &self.manifest.tts.output_format;

        // Collect all .txt files in the book dir and tts_files/ subdirectory
        // (excluding _ prefixed metadata files)
        let mut stems: Vec<String> = Vec::new();
        let dirs_to_scan = [self.book_dir.clone(), self.book_dir.join("tts_files")];
        for scan_dir in &dirs_to_scan {
            if let Ok(entries) = fs::read_dir(scan_dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(ext) = path.extension() {
                            if ext == "txt" {
                                if let Some(name) = path.file_stem() {
                                    let name = name.to_string_lossy().to_string();
                                    if !name.starts_with('_') && !stems.contains(&name) {
                                        stems.push(name);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        stems.sort();

        stems
            .into_iter()
            .map(|stem| {
                let marked = self.manifest.files.marked.contains(&stem);
                let has_text = true; // we found the .txt file
                let has_audio = audio_dir.join(format!("{}.{}", stem, audio_ext)).exists();
                let has_video = video_dir.join(format!("{}.mp4", stem)).exists();
                AvFileStatus {
                    stem,
                    marked,
                    has_text,
                    has_audio,
                    has_video,
                }
            })
            .collect()
    }

    /// Mark stems for AV production. Returns count of newly added stems.
    pub fn mark(&mut self, stems: &[String]) -> Result<usize, String> {
        let mut added = 0;
        for stem in stems {
            if !self.manifest.files.marked.contains(stem) {
                self.manifest.files.marked.push(stem.clone());
                added += 1;
            }
        }
        self.manifest.files.marked.sort();
        self.manifest.save(&self.book_dir)?;
        Ok(added)
    }

    /// Unmark stems. Returns count of removed stems.
    pub fn unmark(&mut self, stems: &[String]) -> Result<usize, String> {
        let before = self.manifest.files.marked.len();
        self.manifest.files.marked.retain(|s| !stems.contains(s));
        let removed = before - self.manifest.files.marked.len();
        self.manifest.save(&self.book_dir)?;
        Ok(removed)
    }

    /// Mark all text files found in the book directory.
    pub fn mark_all(&mut self) -> Result<usize, String> {
        let statuses = self.scan();
        let all_stems: Vec<String> = statuses.into_iter().map(|s| s.stem).collect();
        self.mark(&all_stems)
    }

    /// Clear all marks.
    pub fn clear_marks(&mut self) -> Result<usize, String> {
        let count = self.manifest.files.marked.len();
        self.manifest.files.marked.clear();
        self.manifest.save(&self.book_dir)?;
        Ok(count)
    }

    /// Resolve the book directory for a given output_dir + book_name (same logic as generate weave).
    pub fn resolve_book_dir(output_dir: &str, book_name: &str) -> PathBuf {
        let output_path = PathBuf::from(output_dir);
        if book_name.is_empty() {
            output_path
        } else {
            let sanitized = book_name
                .replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-' && c != ' ', "");
            let dir_name = sanitized.trim().replace(' ', "_");
            output_path.join(dir_name)
        }
    }

    /// Get the illustrations directory path.
    pub fn illustrations_dir(&self) -> PathBuf {
        self.book_dir.join("illustrations")
    }

    /// Get the audio directory path.
    pub fn audio_dir(&self) -> PathBuf {
        self.book_dir.join("audio")
    }

    /// Get the video directory path.
    pub fn video_dir(&self) -> PathBuf {
        self.book_dir.join("video")
    }

    /// Get the chunks directory for a specific stem.
    pub fn chunks_dir(&self, stem: &str) -> PathBuf {
        self.audio_dir().join("chunks").join(stem)
    }

    /// Scan the chunks directory for a stem and return status of each chunk.
    /// Returns empty Vec if the chunks directory does not exist.
    pub fn scan_chunks(&self, stem: &str) -> Vec<ChunkStatus> {
        let dir = self.chunks_dir(stem);
        if !dir.exists() {
            return Vec::new();
        }

        // Collect all chunk indices by scanning for temp_chunk_NNNN files
        let mut indices: std::collections::BTreeSet<u32> = std::collections::BTreeSet::new();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if let Some(idx) = parse_chunk_index(&name) {
                    indices.insert(idx);
                }
            }
        }

        indices
            .iter()
            .map(|&idx| {
                let prefix = format!("temp_chunk_{:04}", idx);
                let has_text = dir.join(format!("{}.txt", prefix)).exists();
                let has_audio = dir.join(format!("{}.wav", prefix)).exists()
                    || dir.join(format!("{}_silence.wav", prefix)).exists();
                let is_rejected = dir.join(format!("{}.wav.bad", prefix)).exists()
                    || dir.join(format!("{}_silence.wav.bad", prefix)).exists();
                ChunkStatus {
                    index: idx,
                    has_text,
                    has_audio,
                    is_rejected,
                }
            })
            .collect()
    }

    /// Count illustration files (png, jpg, jpeg) in the illustrations directory.
    pub fn count_illustrations(&self) -> usize {
        let dir = self.illustrations_dir();
        if !dir.exists() {
            return 0;
        }
        fs::read_dir(&dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| {
                        e.path()
                            .extension()
                            .map(|ext| {
                                let ext = ext.to_string_lossy().to_lowercase();
                                ext == "png" || ext == "jpg" || ext == "jpeg"
                            })
                            .unwrap_or(false)
                    })
                    .count()
            })
            .unwrap_or(0)
    }

    /// Find the next marked stem that lacks audio.
    pub fn next_stem_needing_audio(&self) -> Option<String> {
        let statuses = self.scan();
        statuses
            .into_iter()
            .find(|s| s.marked && s.has_text && !s.has_audio)
            .map(|s| s.stem)
    }

    /// Find the next marked stem that has audio but lacks video.
    pub fn next_stem_needing_video(&self) -> Option<String> {
        let statuses = self.scan();
        statuses
            .into_iter()
            .find(|s| s.marked && s.has_audio && !s.has_video)
            .map(|s| s.stem)
    }

    /// All marked stems that lack audio, in order.
    pub fn all_stems_needing_audio(&self) -> Vec<String> {
        let statuses = self.scan();
        statuses
            .into_iter()
            .filter(|s| s.marked && s.has_text && !s.has_audio)
            .map(|s| s.stem)
            .collect()
    }

    /// All marked stems that have audio but lack video, in order.
    pub fn all_stems_needing_video(&self) -> Vec<String> {
        let statuses = self.scan();
        statuses
            .into_iter()
            .filter(|s| s.marked && s.has_audio && !s.has_video)
            .map(|s| s.stem)
            .collect()
    }

    /// Generate audio for a single stem by spawning `book_to_audio.py`.
    ///
    /// `project_root` is the WeaveLang development/install directory (where the
    /// Python script lives). `api_key` is the Google API key retrieved from the
    /// app's keyring.
    pub fn generate_audio(
        &self,
        stem: &str,
        project_root: &Path,
        api_key: &str,
    ) -> Result<String, String> {
        let input_file = self.resolve_text_file(stem);
        if !input_file.exists() {
            return Err(format!("Input text file not found: {}", input_file.display()));
        }

        let audio_dir = self.audio_dir();
        fs::create_dir_all(&audio_dir)
            .map_err(|e| format!("Failed to create audio directory: {}", e))?;

        let script_path = project_root.join("book_to_audio.py");
        if !script_path.exists() {
            return Err(format!(
                "book_to_audio.py not found at {}",
                script_path.display()
            ));
        }

        let python_exe = find_python(project_root);
        validate_python(&python_exe)?;
        let tts = &self.manifest.tts;

        let mut cmd = Command::new(&python_exe);
        cmd.arg(&script_path)
            .arg("--input-file")
            .arg(&input_file)
            .arg("--output-dir")
            .arg(&audio_dir)
            .arg("--tts-service")
            .arg(&tts.service)
            .arg("--model-name")
            .arg(&tts.model)
            .arg("--output-audio-format")
            .arg(&tts.output_format)
            .arg("--chunk-max-chars")
            .arg(tts.chunk_max_chars.to_string())
            .arg("--max-api-retries")
            .arg(tts.max_api_retries.to_string())
            .arg("--retry-delay")
            .arg(tts.retry_delay.to_string())
            .arg("--concurrent-requests")
            .arg(tts.concurrent_requests.to_string());

        // Voice names (one or more)
        if !tts.voices.is_empty() {
            cmd.arg("--voice-name");
            for v in &tts.voices {
                cmd.arg(v);
            }
        }

        // Prompt prefix
        if !tts.prompt_prefix.is_empty() {
            cmd.arg("--tts-prompt-prefix").arg(&tts.prompt_prefix);
        }

        // Vertex auth flag
        if tts.use_vertex_auth {
            cmd.arg("--use-vertex-auth-for-gemini");
        }

        // Chunks go to audio/chunks/<stem>/ for review and rebuild
        let chunks_dir = self.chunks_dir(stem);
        cmd.arg("--chunks-dir").arg(&chunks_dir);

        // Pass API key via environment variable (not command-line, for security)
        cmd.env("GOOGLE_API_KEY", api_key);
        cmd.env("PYTHONUTF8", "1");

        // Auto-detect interleave files (ULi pattern) for multi-speaker TTS
        if is_interleave_stem(stem) {
            cmd.arg("--interleave");
        }

        let output = cmd
            .output()
            .map_err(|e| format!("Failed to spawn book_to_audio.py ({}): {}", python_exe, e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            let audio_file = audio_dir.join(format!("{}.{}", stem, tts.output_format));
            if audio_file.exists() {
                Ok(format!(
                    "Audio generated: {}\n{}",
                    audio_file.display(),
                    stderr.trim()
                ))
            } else {
                Err(format!(
                    "book_to_audio.py exited successfully but audio file not found: {}\nstdout: {}\nstderr: {}",
                    audio_file.display(),
                    stdout.trim(),
                    stderr.trim()
                ))
            }
        } else {
            let code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            Err(format!(
                "book_to_audio.py exited with code {}.\nstdout: {}\nstderr: {}",
                code,
                stdout.trim(),
                stderr.trim()
            ))
        }
    }

    /// Generate video for a single stem by spawning `create_video.py`.
    ///
    /// Requires the audio file to exist and the illustrations directory to contain
    /// at least one image. `project_root` is where the Python script lives.
    pub fn generate_video(
        &self,
        stem: &str,
        project_root: &Path,
    ) -> Result<String, String> {
        let audio_ext = &self.manifest.tts.output_format;
        let audio_file = self.audio_dir().join(format!("{}.{}", stem, audio_ext));
        if !audio_file.exists() {
            return Err(format!(
                "Audio file not found: {}. Generate audio first.",
                audio_file.display()
            ));
        }

        let illustrations_dir = self.illustrations_dir();
        if !illustrations_dir.exists() || self.count_illustrations() == 0 {
            return Err(format!(
                "No illustrations found in {}. Add images before generating video.",
                illustrations_dir.display()
            ));
        }

        let video_dir = self.video_dir();
        fs::create_dir_all(&video_dir)
            .map_err(|e| format!("Failed to create video directory: {}", e))?;

        let script_path = project_root.join("create_video.py");
        if !script_path.exists() {
            return Err(format!(
                "create_video.py not found at {}",
                script_path.display()
            ));
        }

        let python_exe = find_python(project_root);
        validate_python(&python_exe)?;
        validate_ffmpeg()?;
        let vid_cfg = &self.manifest.video;

        let output = Command::new(&python_exe)
            .arg(&script_path)
            .arg("--audio-file")
            .arg(&audio_file)
            .arg("--illustrations-dir")
            .arg(&illustrations_dir)
            .arg("--output-dir")
            .arg(&video_dir)
            .arg("--frame-rate")
            .arg(vid_cfg.frame_rate.to_string())
            .arg("--image-duration")
            .arg(vid_cfg.image_duration.to_string())
            .env("PYTHONUTF8", "1")
            .output()
            .map_err(|e| format!("Failed to spawn create_video.py ({}): {}", python_exe, e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            let video_file = video_dir.join(format!("{}.mp4", stem));
            if video_file.exists() {
                Ok(format!(
                    "Video generated: {}\n{}",
                    video_file.display(),
                    stdout.trim()
                ))
            } else {
                Err(format!(
                    "create_video.py exited successfully but video file not found: {}\nstdout: {}\nstderr: {}",
                    video_file.display(),
                    stdout.trim(),
                    stderr.trim()
                ))
            }
        } else {
            let code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            Err(format!(
                "create_video.py exited with code {}.\nstdout: {}\nstderr: {}",
                code,
                stdout.trim(),
                stderr.trim()
            ))
        }
    }

    /// Spawn audio generation as a child process with piped stdout/stderr.
    /// Returns the Child handle. Caller is responsible for reading output and waiting.
    pub fn spawn_audio(
        &self,
        stem: &str,
        project_root: &Path,
        api_key: &str,
    ) -> Result<std::process::Child, String> {
        let input_file = self.resolve_text_file(stem);
        if !input_file.exists() {
            return Err(format!("Input text file not found: {}", input_file.display()));
        }

        let audio_dir = self.audio_dir();
        fs::create_dir_all(&audio_dir)
            .map_err(|e| format!("Failed to create audio directory: {}", e))?;

        let script_path = project_root.join("book_to_audio.py");
        if !script_path.exists() {
            return Err(format!("book_to_audio.py not found at {}", script_path.display()));
        }

        let python_exe = find_python(project_root);
        validate_python(&python_exe)?;
        let tts = &self.manifest.tts;

        let mut cmd = Command::new(&python_exe);
        cmd.arg(&script_path)
            .arg("--input-file").arg(&input_file)
            .arg("--output-dir").arg(&audio_dir)
            .arg("--tts-service").arg(&tts.service)
            .arg("--model-name").arg(&tts.model)
            .arg("--output-audio-format").arg(&tts.output_format)
            .arg("--chunk-max-chars").arg(tts.chunk_max_chars.to_string())
            .arg("--max-api-retries").arg(tts.max_api_retries.to_string())
            .arg("--retry-delay").arg(tts.retry_delay.to_string())
            .arg("--concurrent-requests").arg(tts.concurrent_requests.to_string());

        if !tts.voices.is_empty() {
            cmd.arg("--voice-name");
            for v in &tts.voices { cmd.arg(v); }
        }
        if !tts.prompt_prefix.is_empty() {
            cmd.arg("--tts-prompt-prefix").arg(&tts.prompt_prefix);
        }
        if tts.use_vertex_auth {
            cmd.arg("--use-vertex-auth-for-gemini");
        }

        // Chunks go to audio/chunks/<stem>/ for review and rebuild
        let chunks_dir = self.chunks_dir(stem);
        cmd.arg("--chunks-dir").arg(&chunks_dir);

        cmd.env("GOOGLE_API_KEY", api_key);
        cmd.env("PYTHONUTF8", "1");
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        // Auto-detect interleave files (ULi pattern) for multi-speaker TTS
        if is_interleave_stem(stem) {
            cmd.arg("--interleave");
        }

        cmd.spawn()
            .map_err(|e| format!("Failed to spawn book_to_audio.py ({}): {}", python_exe, e))
    }

    /// Spawn video generation as a child process with piped stdout/stderr.
    /// Returns the Child handle. Caller is responsible for reading output and waiting.
    pub fn spawn_video(
        &self,
        stem: &str,
        project_root: &Path,
    ) -> Result<std::process::Child, String> {
        let audio_ext = &self.manifest.tts.output_format;
        let audio_file = self.audio_dir().join(format!("{}.{}", stem, audio_ext));
        if !audio_file.exists() {
            return Err(format!("Audio file not found: {}. Generate audio first.", audio_file.display()));
        }

        let illustrations_dir = self.illustrations_dir();
        if !illustrations_dir.exists() || self.count_illustrations() == 0 {
            return Err(format!("No illustrations found in {}.", illustrations_dir.display()));
        }

        let video_dir = self.video_dir();
        fs::create_dir_all(&video_dir)
            .map_err(|e| format!("Failed to create video directory: {}", e))?;

        let script_path = project_root.join("create_video.py");
        if !script_path.exists() {
            return Err(format!("create_video.py not found at {}", script_path.display()));
        }

        let python_exe = find_python(project_root);
        validate_python(&python_exe)?;
        validate_ffmpeg()?;
        let vid_cfg = &self.manifest.video;

        let mut cmd = Command::new(&python_exe);
        cmd.arg(&script_path)
            .arg("--audio-file").arg(&audio_file)
            .arg("--illustrations-dir").arg(&illustrations_dir)
            .arg("--output-dir").arg(&video_dir)
            .arg("--frame-rate").arg(vid_cfg.frame_rate.to_string())
            .arg("--image-duration").arg(vid_cfg.image_duration.to_string());
        cmd.env("PYTHONUTF8", "1");
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        cmd.spawn()
            .map_err(|e| format!("Failed to spawn create_video.py ({}): {}", python_exe, e))
    }

    /// Spawn illustration prompt generation as a child process.
    /// Calls `generate_illustration_prompts.py` with args from the manifest [illustrations] config.
    pub fn spawn_prompts(
        &self,
        book_name: &str,
        project_root: &Path,
        api_key: &str,
    ) -> Result<std::process::Child, String> {
        let tts_dir = self.book_dir.join("tts_files");
        if !tts_dir.exists() {
            return Err(format!("tts_files directory not found: {}", tts_dir.display()));
        }

        let chapter_name = self.book_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .ok_or("Cannot determine chapter name from book directory.")?;

        let illustrations_dir = self.illustrations_dir();
        fs::create_dir_all(&illustrations_dir)
            .map_err(|e| format!("Failed to create illustrations directory: {}", e))?;

        let script_path = project_root.join("generate_illustration_prompts.py");
        if !script_path.exists() {
            return Err(format!("generate_illustration_prompts.py not found at {}", script_path.display()));
        }

        let python_exe = find_python(project_root);
        validate_python(&python_exe)?;
        let ill = &self.manifest.illustrations;

        let mut cmd = Command::new(&python_exe);
        cmd.arg(&script_path)
            .arg(&tts_dir)
            .arg(&chapter_name)
            .arg("--book-name").arg(book_name)
            .arg("--sentences-per").arg(ill.sentences_per_illustration.to_string())
            .arg("--minimum").arg(ill.minimum_count.to_string())
            .arg("--style").arg(&ill.style_prefix)
            .arg("--model").arg(&ill.prompt_model)
            .arg("--output").arg(illustrations_dir.join("_prompts.toml"));
        cmd.env("GOOGLE_API_KEY", api_key);
        cmd.env("PYTHONUTF8", "1");
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        cmd.spawn()
            .map_err(|e| format!("Failed to spawn generate_illustration_prompts.py ({}): {}", python_exe, e))
    }

    /// Spawn illustration image generation as a child process.
    /// Calls `illustration_gen.py` with the _prompts.toml file.
    pub fn spawn_illustrations(
        &self,
        project_root: &Path,
        api_key: &str,
    ) -> Result<std::process::Child, String> {
        let prompts_file = self.illustrations_dir().join("_prompts.toml");
        if !prompts_file.exists() {
            return Err(format!(
                "No _prompts.toml found at {}. Run 'av generate prompts' first.",
                prompts_file.display()
            ));
        }

        let script_path = project_root.join("illustration_gen.py");
        if !script_path.exists() {
            return Err(format!("illustration_gen.py not found at {}", script_path.display()));
        }

        let python_exe = find_python(project_root);
        validate_python(&python_exe)?;
        let ill = &self.manifest.illustrations;

        let mut cmd = Command::new(&python_exe);
        cmd.arg(&script_path)
            .arg(&prompts_file)
            .arg("--size").arg(&ill.image_size)
            .arg("--model").arg(&ill.image_model)
            .arg("--output-dir").arg(self.illustrations_dir());
        cmd.env("GOOGLE_API_KEY", api_key);
        cmd.env("PYTHONUTF8", "1");
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        cmd.spawn()
            .map_err(|e| format!("Failed to spawn illustration_gen.py ({}): {}", python_exe, e))
    }

    /// Check if _prompts.toml exists in the illustrations directory.
    pub fn has_prompts(&self) -> bool {
        self.illustrations_dir().join("_prompts.toml").exists()
    }

    /// Rebuild final audio by concatenating all good chunks via `book_to_audio.py --concat-only`.
    ///
    /// This is a blocking call (concatenation is fast). No API key needed.
    pub fn rebuild_audio(
        &self,
        stem: &str,
        project_root: &Path,
    ) -> Result<String, String> {
        let chunks_dir = self.chunks_dir(stem);
        if !chunks_dir.exists() {
            return Err(format!("No chunks directory for '{}'.", stem));
        }

        // Verify there are some good chunks
        let chunks = self.scan_chunks(stem);
        let good = chunks.iter().filter(|c| c.has_audio && !c.is_rejected).count();
        if good == 0 {
            return Err(format!("No good audio chunks for '{}'. Nothing to concatenate.", stem));
        }

        let input_file = self.resolve_text_file(stem);
        if !input_file.exists() {
            return Err(format!("Input text file not found: {}", input_file.display()));
        }

        let audio_dir = self.audio_dir();
        fs::create_dir_all(&audio_dir)
            .map_err(|e| format!("Failed to create audio directory: {}", e))?;

        let script_path = project_root.join("book_to_audio.py");
        if !script_path.exists() {
            return Err(format!("book_to_audio.py not found at {}", script_path.display()));
        }

        let python_exe = find_python(project_root);
        validate_python(&python_exe)?;

        let output = Command::new(&python_exe)
            .arg(&script_path)
            .arg("--concat-only")
            .arg("--chunks-dir")
            .arg(&chunks_dir)
            .arg("--input-file")
            .arg(&input_file)
            .arg("--output-dir")
            .arg(&audio_dir)
            .arg("--output-audio-format")
            .arg(&self.manifest.tts.output_format)
            .env("PYTHONUTF8", "1")
            .output()
            .map_err(|e| format!("Failed to spawn book_to_audio.py ({}): {}", python_exe, e))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        if output.status.success() {
            let audio_file = audio_dir.join(format!(
                "{}.{}",
                stem,
                self.manifest.tts.output_format
            ));
            if audio_file.exists() {
                Ok(format!(
                    "Audio rebuilt from {} good chunks: {}\n{}",
                    good,
                    audio_file.display(),
                    stderr.trim()
                ))
            } else {
                Err(format!(
                    "Concat completed but audio file not found: {}\nstdout: {}\nstderr: {}",
                    audio_file.display(),
                    stdout.trim(),
                    stderr.trim()
                ))
            }
        } else {
            let code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "unknown".to_string());
            Err(format!(
                "book_to_audio.py --concat-only exited with code {}.\nstdout: {}\nstderr: {}",
                code,
                stdout.trim(),
                stderr.trim()
            ))
        }
    }

    /// Format a status table as a string for terminal output.
    pub fn format_status_table(statuses: &[AvFileStatus]) -> String {
        if statuses.is_empty() {
            return "No woven text files found in the book directory.".to_string();
        }

        // Find max stem width for alignment
        let max_stem = statuses.iter().map(|s| s.stem.len()).max().unwrap_or(20);
        let col_w = max_stem.max(20);

        let mut out = String::new();
        out.push_str(&format!(
            "{:<width$}  {:>5}  {:>5}  {:>5}  Mark\n",
            "File", "Text", "Audio", "Video",
            width = col_w
        ));
        out.push_str(&format!(
            "{}\n",
            "-".repeat(col_w + 5 + 5 + 5 + 6 + 8)
        ));

        for s in statuses {
            let mark = if s.marked { " [x]" } else { " [ ]" };
            let txt = if s.has_text { "  ✓" } else { "  -" };
            let aud = if s.has_audio { "  ✓" } else if s.marked { "  ✗" } else { "  -" };
            let vid = if s.has_video { "  ✓" } else if s.marked && s.has_audio { "  ✗" } else { "  -" };
            out.push_str(&format!(
                "{:<width$}  {:>5}  {:>5}  {:>5}  {}\n",
                s.stem, txt, aud, vid, mark,
                width = col_w
            ));
        }

        // Summary
        let total = statuses.len();
        let marked = statuses.iter().filter(|s| s.marked).count();
        let audio_done = statuses.iter().filter(|s| s.marked && s.has_audio).count();
        let video_done = statuses.iter().filter(|s| s.marked && s.has_video).count();
        out.push_str(&format!(
            "\nTotal: {} files, {} marked | Audio: {}/{} | Video: {}/{}\n",
            total, marked, audio_done, marked, video_done, marked
        ));

        out
    }
}

// ---------------------------------------------------------------------------
// YouTube config (_youtube.toml)
// ---------------------------------------------------------------------------

pub const YOUTUBE_CONFIG_FILENAME: &str = "_youtube.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YouTubeConfig {
    pub metadata: YouTubeMetadata,
    #[serde(default)]
    pub auth: YouTubeAuth,
    #[serde(default)]
    pub variables: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub uploads: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YouTubeMetadata {
    pub title_template: String,
    pub description_template: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub category_id: String,
    pub privacy: String,
    pub language: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct YouTubeAuth {
    pub client_secret_file: String,
}

impl Default for YouTubeMetadata {
    fn default() -> Self {
        Self {
            title_template: "{book_name} - {chapter_name} ({stem})".to_string(),
            description_template: "Language learning audio with illustrated video.\n\nBook: {book_name}\nChapter: {chapter_name}\nLevel: {stem}".to_string(),
            tags: vec![
                "language learning".to_string(),
                "audiobook".to_string(),
                "illustrated".to_string(),
            ],
            category_id: "27".to_string(), // Education
            privacy: "unlisted".to_string(),
            language: "en".to_string(),
        }
    }
}

impl Default for YouTubeConfig {
    fn default() -> Self {
        Self {
            metadata: YouTubeMetadata::default(),
            auth: YouTubeAuth::default(),
            variables: std::collections::BTreeMap::new(),
            uploads: std::collections::BTreeMap::new(),
        }
    }
}

impl YouTubeConfig {
    pub fn load(book_dir: &Path) -> Result<Option<Self>, String> {
        let path = book_dir.join(YOUTUBE_CONFIG_FILENAME);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
        let config: YouTubeConfig = toml::from_str(&content)
            .map_err(|e| format!("Failed to parse {}: {}", path.display(), e))?;
        Ok(Some(config))
    }

    pub fn save(&self, book_dir: &Path) -> Result<(), String> {
        let path = book_dir.join(YOUTUBE_CONFIG_FILENAME);
        let content = toml::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize YouTube config: {}", e))?;
        fs::write(&path, content)
            .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;
        Ok(())
    }

    /// Load existing config or create a new one and save it.
    ///
    /// When creating, checks the parent (book-level) directory first so
    /// users can place a template `_youtube.toml` at the book root.
    pub fn load_or_create(book_dir: &Path) -> Result<Self, String> {
        match Self::load(book_dir)? {
            Some(c) => Ok(c),
            None => {
                let c = book_dir.parent()
                    .and_then(|parent| Self::load(parent).ok().flatten())
                    .unwrap_or_default();
                c.save(book_dir)?;
                Ok(c)
            }
        }
    }

    pub fn is_uploaded(&self, stem: &str) -> bool {
        self.uploads.contains_key(stem)
    }

    /// Resolve template variables for display / dry-run.
    pub fn resolve_title(&self, vars: &std::collections::BTreeMap<String, String>) -> String {
        resolve_template(&self.metadata.title_template, vars)
    }

    pub fn resolve_description(&self, vars: &std::collections::BTreeMap<String, String>) -> String {
        resolve_template(&self.metadata.description_template, vars)
    }
}

fn resolve_template(template: &str, vars: &std::collections::BTreeMap<String, String>) -> String {
    let mut result = template.to_string();
    for (key, val) in vars {
        result = result.replace(&format!("{{{}}}", key), val);
    }
    result
}

impl AvProducer {
    /// Get the YouTube config file path.
    pub fn youtube_config_path(&self) -> PathBuf {
        self.book_dir.join(YOUTUBE_CONFIG_FILENAME)
    }

    /// Spawn the YouTube authentication flow (opens browser).
    pub fn spawn_youtube_auth(
        &self,
        project_root: &Path,
        client_secret_file: Option<&str>,
    ) -> Result<std::process::Child, String> {
        let yt_config_path = self.youtube_config_path();
        if !yt_config_path.exists() {
            return Err("No _youtube.toml found. Run 'av youtube init' first.".to_string());
        }

        let script_path = project_root.join("youtube_upload.py");
        if !script_path.exists() {
            return Err(format!("youtube_upload.py not found at {}", script_path.display()));
        }

        let python_exe = find_python(project_root);
        validate_python(&python_exe)?;

        let video_dir = self.video_dir();
        let mut cmd = Command::new(&python_exe);
        cmd.arg(&script_path)
            .arg(&video_dir)
            .arg(&yt_config_path)
            .arg("__auth__")  // dummy stem
            .arg("--book-dir").arg(&self.book_dir)
            .arg("--auth-only");
        cmd.env("PYTHONUTF8", "1");
        if let Some(secret_path) = client_secret_file {
            cmd.env("YOUTUBE_CLIENT_SECRET_FILE", secret_path);
        }
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        cmd.spawn()
            .map_err(|e| format!("Failed to spawn youtube_upload.py ({}): {}", python_exe, e))
    }

    /// Spawn a YouTube upload for a specific stem.
    pub fn spawn_youtube_upload(
        &self,
        stem: &str,
        project_root: &Path,
        extra_vars: &str,
        client_secret_file: Option<&str>,
    ) -> Result<std::process::Child, String> {
        let yt_config_path = self.youtube_config_path();
        if !yt_config_path.exists() {
            return Err("No _youtube.toml found. Run 'av youtube init' first.".to_string());
        }

        let video_dir = self.video_dir();
        let script_path = project_root.join("youtube_upload.py");
        if !script_path.exists() {
            return Err(format!("youtube_upload.py not found at {}", script_path.display()));
        }

        let python_exe = find_python(project_root);
        validate_python(&python_exe)?;

        let mut cmd = Command::new(&python_exe);
        cmd.arg(&script_path)
            .arg(&video_dir)
            .arg(&yt_config_path)
            .arg(stem)
            .arg("--book-dir").arg(&self.book_dir)
            .arg("--illustrations-dir").arg(self.illustrations_dir());
        if !extra_vars.is_empty() {
            cmd.arg("--variables").arg(extra_vars);
        }
        cmd.env("PYTHONUTF8", "1");
        if let Some(secret_path) = client_secret_file {
            cmd.env("YOUTUBE_CLIENT_SECRET_FILE", secret_path);
        }
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        cmd.spawn()
            .map_err(|e| format!("Failed to spawn youtube_upload.py ({}): {}", python_exe, e))
    }

    /// Find the next stem that has video but hasn't been uploaded yet.
    pub fn next_stem_needing_upload(&self, yt_config: &YouTubeConfig) -> Option<String> {
        let statuses = self.scan();
        statuses.iter()
            .filter(|s| s.marked && s.has_video && !yt_config.is_uploaded(&s.stem))
            .map(|s| s.stem.clone())
            .next()
    }

    /// Get all stems that need uploading.
    pub fn all_stems_needing_upload(&self, yt_config: &YouTubeConfig) -> Vec<String> {
        let statuses = self.scan();
        statuses.iter()
            .filter(|s| s.marked && s.has_video && !yt_config.is_uploaded(&s.stem))
            .map(|s| s.stem.clone())
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Return true if the stem matches the ULi interleave naming pattern (e.g. "Book_ULi34").
fn is_interleave_stem(stem: &str) -> bool {
    if let Some(pos) = stem.find("ULi") {
        stem[pos + 3..].starts_with(|c: char| c.is_ascii_digit())
    } else {
        false
    }
}

/// Find a Python executable, preferring a local venv.
pub fn find_python(project_root: &Path) -> String {
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

/// Verify the Python executable can actually run.
pub fn validate_python(python_exe: &str) -> Result<(), String> {
    match Command::new(python_exe).arg("--version").output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(format!(
            "Python at '{}' returned an error: {}",
            python_exe,
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(e) => Err(format!(
            "Python not found at '{}': {}. Install Python or create a .venv in the project root.",
            python_exe, e
        )),
    }
}

/// Verify that ffmpeg is available on PATH (required for video generation).
pub fn validate_ffmpeg() -> Result<(), String> {
    match Command::new("ffmpeg").arg("-version").output() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(_) => Err("ffmpeg returned an error. Reinstall ffmpeg and ensure it is on PATH.".to_string()),
        Err(_) => Err("ffmpeg not found. Install ffmpeg and add it to PATH before generating video.".to_string()),
    }
}

/// Extract the zero-based chunk index from a chunk filename.
/// Matches filenames like `temp_chunk_0000.txt`, `temp_chunk_0001.wav`,
/// `temp_chunk_0002.wav.bad`, `temp_chunk_0003_silence.wav`.
pub fn parse_chunk_index(filename: &str) -> Option<u32> {
    let rest = filename.strip_prefix("temp_chunk_")?;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn make_temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("weavelang_av_test_{}", name));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn manifest_roundtrip() {
        let dir = make_temp_dir("roundtrip");
        let mut m = AvManifest::default();
        m.tts.service = "vertex".to_string();
        m.tts.voices = vec!["Charon".to_string()];
        m.files.marked = vec!["Book_UL7".to_string(), "Book_UL8".to_string()];
        m.save(&dir).unwrap();

        let loaded = AvManifest::load(&dir).unwrap().unwrap();
        assert_eq!(loaded.tts.service, "vertex");
        assert_eq!(loaded.tts.voices, vec!["Charon"]);
        assert_eq!(loaded.files.marked, vec!["Book_UL7", "Book_UL8"]);
        assert_eq!(loaded.video.frame_rate, 30);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn manifest_load_missing_returns_none() {
        let dir = make_temp_dir("missing");
        let result = AvManifest::load(&dir).unwrap();
        assert!(result.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_or_create_creates_default() {
        let dir = make_temp_dir("create_default");
        let m = AvManifest::load_or_create(&dir).unwrap();
        assert_eq!(m.tts.service, "gemini");
        assert!(dir.join(AV_MANIFEST_FILENAME).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_finds_text_files() {
        let dir = make_temp_dir("scan");
        fs::write(dir.join("Book_UL7.txt"), "text").unwrap();
        fs::write(dir.join("Book_UL8.txt"), "text").unwrap();
        fs::write(dir.join("_av_manifest.toml"), "").unwrap(); // should be ignored (starts with _)

        let mut m = AvManifest::default();
        m.files.marked = vec!["Book_UL7".to_string()];
        m.save(&dir).unwrap();

        let producer = AvProducer::new(dir.clone()).unwrap();
        let statuses = producer.scan();
        assert_eq!(statuses.len(), 2);
        assert!(statuses[0].stem == "Book_UL7");
        assert!(statuses[0].marked);
        assert!(statuses[0].has_text);
        assert!(!statuses[0].has_audio);
        assert!(!statuses[1].marked);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_detects_audio_and_video() {
        let dir = make_temp_dir("av_detect");
        fs::write(dir.join("Book_UL7.txt"), "text").unwrap();
        fs::create_dir_all(dir.join("audio")).unwrap();
        fs::write(dir.join("audio").join("Book_UL7.wav"), "audio").unwrap();
        fs::create_dir_all(dir.join("video")).unwrap();
        fs::write(dir.join("video").join("Book_UL7.mp4"), "video").unwrap();

        let producer = AvProducer::new(dir.clone()).unwrap();
        let statuses = producer.scan();
        assert_eq!(statuses.len(), 1);
        assert!(statuses[0].has_audio);
        assert!(statuses[0].has_video);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mark_and_unmark() {
        let dir = make_temp_dir("mark");
        fs::write(dir.join("A.txt"), "").unwrap();
        fs::write(dir.join("B.txt"), "").unwrap();
        fs::write(dir.join("C.txt"), "").unwrap();

        let mut producer = AvProducer::new(dir.clone()).unwrap();
        let added = producer.mark(&["A".to_string(), "B".to_string()]).unwrap();
        assert_eq!(added, 2);
        assert_eq!(producer.manifest.files.marked, vec!["A", "B"]);

        // Duplicate mark should not add again
        let added2 = producer.mark(&["A".to_string()]).unwrap();
        assert_eq!(added2, 0);

        let removed = producer.unmark(&["A".to_string()]).unwrap();
        assert_eq!(removed, 1);
        assert_eq!(producer.manifest.files.marked, vec!["B"]);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mark_all_and_clear() {
        let dir = make_temp_dir("mark_all");
        fs::write(dir.join("X.txt"), "").unwrap();
        fs::write(dir.join("Y.txt"), "").unwrap();

        let mut producer = AvProducer::new(dir.clone()).unwrap();
        let added = producer.mark_all().unwrap();
        assert_eq!(added, 2);
        assert_eq!(producer.manifest.files.marked.len(), 2);

        let cleared = producer.clear_marks().unwrap();
        assert_eq!(cleared, 2);
        assert!(producer.manifest.files.marked.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_book_dir_sanitizes() {
        let p = AvProducer::resolve_book_dir("/output", "My Book!");
        assert_eq!(p, PathBuf::from("/output/My_Book"));

        let p2 = AvProducer::resolve_book_dir("/output", "");
        assert_eq!(p2, PathBuf::from("/output"));
    }

    #[test]
    fn count_illustrations_works() {
        let dir = make_temp_dir("illustrations");
        let ill_dir = dir.join("illustrations");
        fs::create_dir_all(&ill_dir).unwrap();
        fs::write(ill_dir.join("001.png"), "").unwrap();
        fs::write(ill_dir.join("002.jpg"), "").unwrap();
        fs::write(ill_dir.join("readme.txt"), "").unwrap(); // not an image

        let producer = AvProducer::new(dir.clone()).unwrap();
        assert_eq!(producer.count_illustrations(), 2);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_chunks_empty_dir() {
        let dir = make_temp_dir("chunks_empty");
        fs::write(dir.join("Book_UL7.txt"), "text").unwrap();
        let producer = AvProducer::new(dir.clone()).unwrap();
        let chunks = producer.scan_chunks("Book_UL7");
        assert!(chunks.is_empty()); // no chunks dir yet
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_chunks_finds_statuses() {
        let dir = make_temp_dir("chunks_scan");
        fs::write(dir.join("Book_UL7.txt"), "text").unwrap();
        let chunks_dir = dir.join("audio").join("chunks").join("Book_UL7");
        fs::create_dir_all(&chunks_dir).unwrap();

        // Chunk 0: complete (text + audio)
        fs::write(chunks_dir.join("temp_chunk_0000.txt"), "text 0").unwrap();
        fs::write(chunks_dir.join("temp_chunk_0000.wav"), "audio 0").unwrap();

        // Chunk 1: text only (audio missing)
        fs::write(chunks_dir.join("temp_chunk_0001.txt"), "text 1").unwrap();

        // Chunk 2: rejected
        fs::write(chunks_dir.join("temp_chunk_0002.txt"), "text 2").unwrap();
        fs::write(chunks_dir.join("temp_chunk_0002.wav.bad"), "bad audio").unwrap();

        // Chunk 3: silence chunk
        fs::write(chunks_dir.join("temp_chunk_0003.txt"), "").unwrap();
        fs::write(chunks_dir.join("temp_chunk_0003_silence.wav"), "silence").unwrap();

        let producer = AvProducer::new(dir.clone()).unwrap();
        let chunks = producer.scan_chunks("Book_UL7");
        assert_eq!(chunks.len(), 4);

        assert_eq!(chunks[0].index, 0);
        assert!(chunks[0].has_text);
        assert!(chunks[0].has_audio);
        assert!(!chunks[0].is_rejected);

        assert_eq!(chunks[1].index, 1);
        assert!(chunks[1].has_text);
        assert!(!chunks[1].has_audio);
        assert!(!chunks[1].is_rejected);

        assert_eq!(chunks[2].index, 2);
        assert!(chunks[2].has_text);
        assert!(!chunks[2].has_audio);
        assert!(chunks[2].is_rejected);

        assert_eq!(chunks[3].index, 3);
        assert!(chunks[3].has_text);
        assert!(chunks[3].has_audio); // silence counts as audio
        assert!(!chunks[3].is_rejected);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn parse_chunk_index_works() {
        assert_eq!(super::parse_chunk_index("temp_chunk_0000.txt"), Some(0));
        assert_eq!(super::parse_chunk_index("temp_chunk_0001.wav"), Some(1));
        assert_eq!(super::parse_chunk_index("temp_chunk_0002.wav.bad"), Some(2));
        assert_eq!(super::parse_chunk_index("temp_chunk_0003_silence.wav"), Some(3));
        assert_eq!(super::parse_chunk_index("other_file.txt"), None);
        assert_eq!(super::parse_chunk_index("_metadata.json"), None);
    }

    #[test]
    fn is_interleave_stem_works() {
        assert!(super::is_interleave_stem("grimms_The_Golden_Bird_ULi34"));
        assert!(super::is_interleave_stem("Book_ULi7"));
        assert!(!super::is_interleave_stem("Book_UL7"));
        assert!(!super::is_interleave_stem("Book_ULb12"));
        assert!(!super::is_interleave_stem("Book_ULi"));
        assert!(!super::is_interleave_stem("something_else"));
    }
}
