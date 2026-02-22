/// Memory category determines which file a memory entry is written to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryCategory {
    /// Daily log → `YYYY-MM-DD.md`
    Daily,
    /// Core knowledge → `MEMORY.md` (user preferences, key decisions, long-term facts)
    Core,
    /// Experience notes → `EXPERIENCE.md` (tool tips, problem-solving patterns)
    Experience,
}

/// Request to write a new memory entry.
pub struct MemoryWriteRequest {
    /// The content to save
    pub content: String,
    /// Which category / file to write to
    pub category: MemoryCategory,
    /// Optional heading for MEMORY.md / EXPERIENCE.md section organization
    pub heading: Option<String>,
}

/// A parsed markdown section (## heading + body).
pub(crate) struct MarkdownSection {
    /// The heading text (without the `## ` prefix)
    pub heading: String,
    /// Everything after the heading line until the next `## ` or EOF
    pub content: String,
}
