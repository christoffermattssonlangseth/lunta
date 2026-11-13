use serde::{Deserialize, Serialize};

/// Bibliographic metadata for one paper.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(default)]
pub struct PaperMeta {
    pub title: String,
    pub authors: Vec<String>,
    /// ISO date or year: "2024", "2024-03", "2024-03-12".
    pub date: String,
    pub journal: String,
    pub volume: String,
    pub issue: String,
    pub pages: String,
    pub publisher: String,
    pub doi: String,
    pub pmid: String,
    pub pmcid: String,
    pub arxiv: String,
    pub url: String,
    pub abstract_text: String,
    pub keywords: Vec<String>,
    pub license: String,
}

impl PaperMeta {
    pub fn year(&self) -> String {
        self.date.chars().take(4).collect()
    }
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct Paper {
    pub id: String,
    pub meta: PaperMeta,
    /// Unix time in milliseconds.
    pub added: i64,
    pub modified: i64,
    pub has_pdf: bool,
    pub has_epub: bool,
    /// A rendered neural narration (narration.m4a + narration.json) exists.
    pub has_audio: bool,
    pub read: bool,
    pub starred: bool,
    pub collections: Vec<String>,
    pub tags: Vec<String>,
    pub annotation_count: u32,
}

#[derive(uniffi::Enum, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnnotationKind {
    Highlight,
    Underline,
    Note,
}

/// A highlight or note. `location` and `position` are owned by the UI (e.g. "pdf:3" plus a JSON
/// list of rects, or "epub:ch05.xhtml" plus a text range); `quote`/`prefix`/`suffix` let an
/// annotation be re-anchored in either view if positions do not carry over.
#[derive(uniffi::Record, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Annotation {
    pub id: String,
    pub paper_id: String,
    pub kind: AnnotationKind,
    pub color: String,
    pub location: String,
    pub position: String,
    pub quote: String,
    pub prefix: String,
    pub suffix: String,
    pub note: String,
    pub created: i64,
    pub modified: i64,
}

#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct PaperCollection {
    pub id: String,
    pub name: String,
    pub parent: Option<String>,
    pub paper_count: u32,
}

#[derive(uniffi::Record, Clone, Debug)]
pub struct SearchHit {
    pub paper: Paper,
    /// Matching text with the hit wrapped in [ and ].
    pub snippet: String,
    pub score: f64,
}

#[derive(uniffi::Record, Clone, Debug, Default)]
pub struct PaperFilter {
    pub collection: Option<String>,
    pub tag: Option<String>,
    pub starred_only: bool,
    pub unread_only: bool,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    Added,
    Year,
    Title,
    FirstAuthor,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileKind {
    Pdf,
    Epub,
    Fulltext,
    /// Neural narration audio (AAC).
    Audio,
    /// Timing map for the narration: which paragraph plays when.
    AudioIndex,
}

impl FileKind {
    pub fn file_name(self) -> &'static str {
        match self {
            FileKind::Pdf => "paper.pdf",
            FileKind::Epub => "book.epub",
            FileKind::Fulltext => "fulltext.txt",
            FileKind::Audio => "narration.m4a",
            FileKind::AudioIndex => "narration.json",
        }
    }
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileStatus {
    Missing,
    /// In iCloud but not downloaded to this device yet.
    InCloud,
    Local,
}

#[derive(uniffi::Enum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CitationStyle {
    Apa,
    Vancouver,
    Nature,
    Harvard,
    Chicago,
    Mla,
    Cell,
    Ieee,
    Ama,
    Science,
    Pnas,
    Acs,
}

/// A citation style as the apps list it: `key` is what add-ins and agents pass ("apa").
#[derive(uniffi::Record, Clone, Debug)]
pub struct CitationStyleInfo {
    pub style: CitationStyle,
    pub key: String,
    pub name: String,
    /// Numbered references ([1], ¹) rather than author and year.
    pub numeric: bool,
}

impl CitationStyle {
    /// Every style, in the order menus show them: author–year first, then numbered.
    pub const ALL: [CitationStyle; 12] = [
        Self::Apa, Self::Harvard, Self::Chicago, Self::Cell, Self::Mla,
        Self::Vancouver, Self::Ieee, Self::Ama, Self::Nature, Self::Science, Self::Pnas, Self::Acs,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Apa => "apa", Self::Vancouver => "vancouver", Self::Nature => "nature", Self::Harvard => "harvard",
            Self::Chicago => "chicago", Self::Mla => "mla", Self::Cell => "cell", Self::Ieee => "ieee",
            Self::Ama => "ama", Self::Science => "science", Self::Pnas => "pnas", Self::Acs => "acs",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Apa => "APA 7", Self::Vancouver => "Vancouver", Self::Nature => "Nature", Self::Harvard => "Harvard",
            Self::Chicago => "Chicago (author-date)", Self::Mla => "MLA 9", Self::Cell => "Cell", Self::Ieee => "IEEE",
            Self::Ama => "AMA", Self::Science => "Science", Self::Pnas => "PNAS", Self::Acs => "ACS",
        }
    }

    pub fn numeric(self) -> bool {
        !matches!(self, Self::Apa | Self::Harvard | Self::Chicago | Self::Mla | Self::Cell)
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key.trim().to_lowercase())
    }
}

#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum CoreError {
    #[error("file error: {0}")]
    Io(String),
    #[error("database error: {0}")]
    Db(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("{0}")]
    Invalid(String),
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e.to_string())
    }
}

impl From<rusqlite::Error> for CoreError {
    fn from(e: rusqlite::Error) -> Self {
        CoreError::Db(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, CoreError>;

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
