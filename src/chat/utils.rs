use std::path::Path;

use endpoints::chat::{
    ChatCompletionRequest, ChatCompletionUserMessageContent, ContentPart, Image,
    ImageContentPart, TextContentPart,
};

/// Metadata about a file attachment extracted from the user message.
#[derive(Debug, Clone)]
pub struct FileAttachmentInfo {
    /// Full local file path (stored in `InputFile.filename`)
    pub filename: String,
    /// File basename (derived from `filename`)
    pub basename: String,
    /// MIME type guessed from file extension
    pub mime_type: String,
}

/// Guess MIME type from file extension.
fn guess_mime_type(path: &str) -> String {
    let ext = Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "csv" => "text/csv",
        "pdf" => "application/pdf",
        "json" => "application/json",
        "xml" => "application/xml",
        "js" => "text/javascript",
        "ts" => "text/typescript",
        "py" => "text/x-python",
        "rs" => "text/x-rust",
        "go" => "text/x-go",
        "java" => "text/x-java",
        "c" => "text/x-c",
        "cpp" => "text/x-c++",
        "h" => "text/x-c",
        "hpp" => "text/x-c++",
        "css" => "text/css",
        "html" => "text/html",
        "yaml" | "yml" => "text/yaml",
        "toml" => "text/toml",
        "sh" => "text/x-shellscript",
        "sql" => "text/x-sql",
        _ => "application/octet-stream",
    }
    .to_string()
}

/// Extract user message text and file attachments from the last user message.
///
/// Returns `(text, file_attachments)`. When attachments are present, an attachment
/// summary is appended to the text so the Planner can be aware of them.
pub(super) fn extract_user_message_with_files(
    request: &ChatCompletionRequest,
) -> (Option<String>, Vec<FileAttachmentInfo>) {
    let mut files: Vec<FileAttachmentInfo> = Vec::new();

    let text = request.messages.iter().rev().find_map(|msg| {
        match msg {
            endpoints::chat::ChatCompletionRequestMessage::User(user_msg) => {
                match user_msg.content() {
                    ChatCompletionUserMessageContent::Text(text) => Some(text.clone()),
                    ChatCompletionUserMessageContent::Parts(parts) => {
                        let mut text_parts: Vec<String> = Vec::new();

                        for part in parts {
                            match part {
                                ContentPart::Text(t) => {
                                    text_parts.push(t.text().to_string());
                                }
                                ContentPart::File(f) => {
                                    if let Some(path) = f.file().filename.as_ref() {
                                        let basename = Path::new(path)
                                            .file_name()
                                            .and_then(|n| n.to_str())
                                            .unwrap_or(path)
                                            .to_string();
                                        let mime_type = guess_mime_type(path);
                                        files.push(FileAttachmentInfo {
                                            filename: path.clone(),
                                            basename,
                                            mime_type,
                                        });
                                    }
                                }
                                // Ignore image / audio parts for now
                                _ => {}
                            }
                        }

                        if text_parts.is_empty() && files.is_empty() {
                            None
                        } else {
                            Some(text_parts.join("\n"))
                        }
                    }
                }
            }
            _ => None,
        }
    });

    // Append attachment summary to text so the Planner sees the file list
    let text = if !files.is_empty() {
        let summary = files
            .iter()
            .enumerate()
            .map(|(i, f)| format!("  {}. {} ({})", i + 1, f.basename, f.mime_type))
            .collect::<Vec<_>>()
            .join("\n");
        let attachment_note = format!(
            "\n\n[Attached files]\n{}",
            summary
        );
        Some(
            text.map(|t| format!("{}{}", t, attachment_note))
                .unwrap_or(attachment_note),
        )
    } else {
        text
    };

    (text, files)
}

/// Extract user message text from the chat request (compatibility wrapper).
///
/// Delegates to [`extract_user_message_with_files`] and discards file info.
pub(super) fn extract_user_message(request: &ChatCompletionRequest) -> Option<String> {
    let (text, _files) = extract_user_message_with_files(request);
    text
}

/// Extract system message from the chat request
pub(super) fn extract_system_message(request: &ChatCompletionRequest) -> Option<String> {
    request.messages.iter().find_map(|msg| match msg {
        endpoints::chat::ChatCompletionRequestMessage::System(system_msg) => {
            Some(system_msg.content().to_string())
        }
        _ => None,
    })
}

// File size limits
const MAX_TEXT_FILE_SIZE: u64 = 1_048_576; // 1 MB
const MAX_IMAGE_FILE_SIZE: u64 = 10_485_760; // 10 MB

/// Resolve a file attachment into ContentPart(s) for LLM consumption (Plan Mode).
///
/// - Images → base64 data URI as `ContentPart::Image`
/// - Text/Code → file content wrapped in a code block as `ContentPart::Text`
/// - PDF → informational text noting the file path
/// - Errors → descriptive text so the LLM knows what went wrong
pub(super) fn resolve_file_for_llm(file: &FileAttachmentInfo) -> Vec<ContentPart> {
    let path = Path::new(&file.filename);

    // Check file exists
    if !path.exists() {
        return vec![ContentPart::Text(TextContentPart::new(format!(
            "[File not found: {}]",
            file.basename
        )))];
    }

    // Read metadata
    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => {
            return vec![ContentPart::Text(TextContentPart::new(format!(
                "[Cannot read file {}: {}]",
                file.basename, e
            )))];
        }
    };

    if file.mime_type.starts_with("image/") {
        // Image file
        if metadata.len() > MAX_IMAGE_FILE_SIZE {
            return vec![ContentPart::Text(TextContentPart::new(format!(
                "[Image {} is too large ({:.1} MB, limit is 10 MB)]",
                file.basename,
                metadata.len() as f64 / 1_048_576.0
            )))];
        }
        match std::fs::read(path) {
            Ok(data) => {
                use base64::{Engine as _, engine::general_purpose::STANDARD};
                let b64 = STANDARD.encode(&data);
                let data_uri = format!("data:{};base64,{}", file.mime_type, b64);
                vec![ContentPart::Image(ImageContentPart::new(Image {
                    url: data_uri,
                    detail: None,
                }))]
            }
            Err(e) => vec![ContentPart::Text(TextContentPart::new(format!(
                "[Failed to read image {}: {}]",
                file.basename, e
            )))],
        }
    } else if file.mime_type == "application/pdf" {
        // PDF file — not directly readable as text; inform LLM
        vec![ContentPart::Text(TextContentPart::new(format!(
            "[PDF file attached: {} ({}). PDF content extraction is not yet supported. Path: {}]",
            file.basename,
            format_size(metadata.len()),
            file.filename
        )))]
    } else if file.mime_type.starts_with("text/")
        || file.mime_type == "application/json"
        || file.mime_type == "application/xml"
    {
        // Text / code file
        if metadata.len() > MAX_TEXT_FILE_SIZE {
            return vec![ContentPart::Text(TextContentPart::new(format!(
                "[Text file {} is too large ({:.1} MB, limit is 1 MB)]",
                file.basename,
                metadata.len() as f64 / 1_048_576.0
            )))];
        }
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let ext = Path::new(&file.basename)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                vec![ContentPart::Text(TextContentPart::new(format!(
                    "File: {}\n```{}\n{}\n```",
                    file.basename, ext, content
                )))]
            }
            Err(e) => vec![ContentPart::Text(TextContentPart::new(format!(
                "[Failed to read file {}: {}]",
                file.basename, e
            )))],
        }
    } else {
        // Unsupported type
        vec![ContentPart::Text(TextContentPart::new(format!(
            "[Unsupported file type: {} ({})]",
            file.basename, file.mime_type
        )))]
    }
}

/// Resolve a file attachment as plain text (Agent Swarm Mode).
///
/// Images are described by path (cannot be embedded in a text-only context).
/// Text/Code files have their content embedded. PDF and unsupported types are noted.
pub(super) fn resolve_file_as_text(file: &FileAttachmentInfo) -> String {
    let path = Path::new(&file.filename);

    if !path.exists() {
        return format!("[File not found: {}]", file.basename);
    }

    let metadata = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) => return format!("[Cannot read file {}: {}]", file.basename, e),
    };

    if file.mime_type.starts_with("image/") {
        format!(
            "[Image file: {} ({}) — path: {}]",
            file.basename,
            format_size(metadata.len()),
            file.filename
        )
    } else if file.mime_type == "application/pdf" {
        format!(
            "[PDF file: {} ({}) — path: {}]",
            file.basename,
            format_size(metadata.len()),
            file.filename
        )
    } else if file.mime_type.starts_with("text/")
        || file.mime_type == "application/json"
        || file.mime_type == "application/xml"
    {
        if metadata.len() > MAX_TEXT_FILE_SIZE {
            return format!(
                "[Text file {} is too large ({:.1} MB, limit is 1 MB)]",
                file.basename,
                metadata.len() as f64 / 1_048_576.0
            );
        }
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let ext = Path::new(&file.basename)
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                format!("File: {}\n```{}\n{}\n```", file.basename, ext, content)
            }
            Err(e) => format!("[Failed to read file {}: {}]", file.basename, e),
        }
    } else {
        format!("[Unsupported file type: {} ({})]", file.basename, file.mime_type)
    }
}

/// Format a byte size for display.
fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    }
}

/// Intelligently chunk text while maintaining word integrity and formatting
///
/// # Parameters
/// * `text` - The text to be chunked
/// * `chunk_size` - Target character count per chunk
///
/// # Returns
/// Vector of chunked strings, preserving original formatting and whitespace characters
pub(super) fn gen_chunks_with_formatting(text: impl AsRef<str>, chunk_size: usize) -> Vec<String> {
    let content = text.as_ref();
    let mut chunks: Vec<String> = Vec::new();

    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // Accumulate characters until reaching chunk_size or encountering a natural split point
        let mut temp_chunk = String::new();

        // Accumulate characters until reaching chunk_size
        while i < chars.len() && temp_chunk.len() < chunk_size {
            temp_chunk.push(chars[i]);
            i += 1;
        }

        // If not at the end of text, try to split at word boundaries
        if i < chars.len() {
            // Look forward until finding space, newline, or other appropriate split points
            while i < chars.len() && !chars[i].is_whitespace() {
                temp_chunk.push(chars[i]);
                i += 1;
            }

            // Include immediately following whitespace characters (but not newlines)
            while i < chars.len() && chars[i].is_whitespace() && chars[i] != '\n' {
                temp_chunk.push(chars[i]);
                i += 1;
            }

            // If the next character is a newline, include it
            if i < chars.len() && chars[i] == '\n' {
                temp_chunk.push(chars[i]);
                i += 1;
            }
        }

        if !temp_chunk.is_empty() {
            chunks.push(temp_chunk);
        }
    }

    chunks
}
