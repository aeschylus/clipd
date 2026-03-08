use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The type of content stored in a clipboard entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ContentType {
    PlainText,
    RichText,
    Image,
    FilePath,
    Url,
    Code,
}

impl ContentType {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContentType::PlainText => "plain_text",
            ContentType::RichText => "rich_text",
            ContentType::Image => "image",
            ContentType::FilePath => "file_path",
            ContentType::Url => "url",
            ContentType::Code => "code",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "rich_text" => ContentType::RichText,
            "image" => ContentType::Image,
            "file_path" => ContentType::FilePath,
            "url" => ContentType::Url,
            "code" => ContentType::Code,
            _ => ContentType::PlainText,
        }
    }
}

impl std::fmt::Display for ContentType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// A single clipboard history entry with full metadata.
///
/// Inspired by Paste.app's card model — every clip carries provenance
/// (source app, timestamp) and is deduplicated by content hash.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipEntry {
    /// Auto-incremented SQLite rowid.
    pub id: i64,

    /// Raw clipboard content (text). For images, this holds a base64-encoded
    /// representation or a file path to the cached image.
    pub content: String,

    /// Semantic content type, determined heuristically at capture time.
    pub content_type: ContentType,

    /// SHA-256 of `content` (hex-encoded). Used for deduplication.
    pub hash: String,

    /// The application that owned focus when the copy was made.
    /// Platform-specific detection; None if unavailable.
    pub source_app: Option<String>,

    /// UTC timestamp of capture.
    pub created_at: DateTime<Utc>,

    /// Whether this entry is pinned (protected from eviction by max-history).
    pub pinned: bool,

    /// User-defined tags for organisation and filtering.
    pub tags: Vec<String>,

    /// Optional user-provided label (rename a clip, like Paste.app allows).
    pub label: Option<String>,
}

impl ClipEntry {
    /// Construct a new entry from raw clipboard content plus metadata.
    pub fn new(
        content: String,
        content_type: ContentType,
        hash: String,
        source_app: Option<String>,
    ) -> Self {
        Self {
            id: 0, // assigned by SQLite on insert
            content,
            content_type,
            hash,
            source_app,
            created_at: Utc::now(),
            pinned: false,
            tags: Vec::new(),
            label: None,
        }
    }

    /// A short preview of the content suitable for terminal display.
    pub fn preview(&self, max_len: usize) -> String {
        let s = self.content.trim();
        if s.len() <= max_len {
            s.to_string()
        } else {
            format!("{}…", &s[..max_len])
        }
    }
}
