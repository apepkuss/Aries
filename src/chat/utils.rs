use endpoints::chat::{ChatCompletionRequest, ChatCompletionUserMessageContent};

/// Extract user messages from the chat request
pub(super) fn extract_user_message(request: &ChatCompletionRequest) -> Option<String> {
    request.messages.iter().rev().find_map(|msg| {
        match msg {
            endpoints::chat::ChatCompletionRequestMessage::User(user_msg) => {
                match user_msg.content() {
                    ChatCompletionUserMessageContent::Text(text) => Some(text.clone()),
                    ChatCompletionUserMessageContent::Parts(parts) => {
                        // Extract text parts
                        let text_parts: Vec<String> = parts
                            .iter()
                            .filter_map(|part| {
                                // Simplified handling: directly try to convert to string
                                // This might need to be handled based on actual part type
                                serde_json::to_string(part).ok()
                            })
                            .collect();
                        if text_parts.is_empty() {
                            None
                        } else {
                            Some(text_parts.join(" "))
                        }
                    }
                }
            }
            _ => None,
        }
    })
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
