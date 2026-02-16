use super::types::{Chunk, content_hash};

/// Markdown 语义分块器
pub struct MarkdownChunker {
    max_chunk_lines: usize,
    min_chunk_lines: usize,
}

/// heading 层级信息
struct HeadingInfo {
    level: usize,
    title: String,
}

impl MarkdownChunker {
    pub fn new(max_chunk_lines: usize, min_chunk_lines: usize) -> Self {
        Self {
            max_chunk_lines,
            min_chunk_lines,
        }
    }

    /// 从 LantaiConfig 的 ChunkingConfig 创建
    pub fn from_config(config: &crate::config::ChunkingConfig) -> Self {
        Self::new(config.max_chunk_lines, config.min_chunk_lines)
    }

    /// 对 Markdown 文件内容进行语义分块
    pub fn chunk_file(&self, source_path: &str, content: &str) -> Vec<Chunk> {
        let lines: Vec<&str> = content.lines().collect();
        if lines.is_empty() {
            return vec![];
        }

        let mut chunks = Vec::new();
        let mut heading_stack: Vec<HeadingInfo> = Vec::new();
        let mut current_lines: Vec<&str> = Vec::new();
        let mut current_start: usize = 1; // 1-based

        for (idx, line) in lines.iter().enumerate() {
            let line_num = idx + 1; // 1-based

            if let Some((level, title)) = parse_heading(line) {
                // 将之前累积的内容保存为 chunk(s)
                if !current_lines.is_empty() {
                    let heading_path = build_heading_path(&heading_stack);
                    let end_line = line_num - 1;
                    let raw = build_raw_chunk(
                        source_path,
                        &heading_path,
                        &current_lines,
                        current_start,
                        end_line,
                    );
                    chunks.extend(self.maybe_split(raw));
                    current_lines.clear();
                    current_start = line_num;
                }

                // 更新 heading 层级栈：弹出所有 >= 当前 level 的
                while heading_stack.last().is_some_and(|h| h.level >= level) {
                    heading_stack.pop();
                }
                heading_stack.push(HeadingInfo {
                    level,
                    title: title.to_string(),
                });
            }

            current_lines.push(line);
        }

        // 处理最后一个 chunk
        if !current_lines.is_empty() {
            let heading_path = build_heading_path(&heading_stack);
            let end_line = lines.len();
            let raw = build_raw_chunk(
                source_path,
                &heading_path,
                &current_lines,
                current_start,
                end_line,
            );
            chunks.extend(self.maybe_split(raw));
        }

        // 后处理：小段合并
        self.merge_small_chunks(chunks)
    }

    /// 超过 max_chunk_lines 时在 "\n\n"（空行）处拆分
    fn maybe_split(&self, chunk: Chunk) -> Vec<Chunk> {
        let line_count = chunk.end_line - chunk.start_line + 1;
        if line_count <= self.max_chunk_lines {
            return vec![chunk];
        }

        let lines: Vec<&str> = chunk.content.lines().collect();
        let mut result = Vec::new();
        let mut seg_start = 0usize; // 0-based index into lines
        let mut last_empty = None;

        for (i, line) in lines.iter().enumerate() {
            let lines_so_far = i - seg_start + 1;

            if line.trim().is_empty() {
                last_empty = Some(i);
            }

            if lines_so_far >= self.max_chunk_lines {
                // 在最近的空行处拆分
                let split_at = last_empty.unwrap_or(i);
                let seg_lines = &lines[seg_start..=split_at];
                let seg_content = seg_lines.join("\n");
                let start_line = chunk.start_line + seg_start;
                let end_line = chunk.start_line + split_at;

                result.push(Chunk {
                    source_path: chunk.source_path.clone(),
                    heading_path: chunk.heading_path.clone(),
                    content: seg_content.clone(),
                    start_line,
                    end_line,
                    content_hash: content_hash(&seg_content),
                });

                seg_start = split_at + 1;
                last_empty = None;
            }
        }

        // 剩余部分
        if seg_start < lines.len() {
            let seg_lines = &lines[seg_start..];
            let seg_content = seg_lines.join("\n");
            let start_line = chunk.start_line + seg_start;
            let end_line = chunk.end_line;

            result.push(Chunk {
                source_path: chunk.source_path.clone(),
                heading_path: chunk.heading_path.clone(),
                content: seg_content.clone(),
                start_line,
                end_line,
                content_hash: content_hash(&seg_content),
            });
        }

        result
    }

    /// 小于 min_chunk_lines 的 chunk 向前合并
    fn merge_small_chunks(&self, chunks: Vec<Chunk>) -> Vec<Chunk> {
        if chunks.is_empty() {
            return chunks;
        }

        let mut result: Vec<Chunk> = Vec::new();

        for chunk in chunks {
            let line_count = chunk.end_line - chunk.start_line + 1;

            if line_count < self.min_chunk_lines && !result.is_empty() {
                // 向前合并到前一个 chunk
                let prev = result.last_mut().unwrap();
                let merged_content = format!("{}\n{}", prev.content, chunk.content);
                prev.end_line = chunk.end_line;
                prev.content_hash = content_hash(&merged_content);
                prev.content = merged_content;
            } else {
                result.push(chunk);
            }
        }

        result
    }
}

/// 解析 Markdown heading 行，返回 (level, title)
fn parse_heading(line: &str) -> Option<(usize, &str)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('#') {
        return None;
    }

    let level = trimmed.chars().take_while(|&c| c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }

    let rest = &trimmed[level..];
    // heading 后必须有空格
    if !rest.starts_with(' ') {
        return None;
    }

    let title = rest.trim();
    if title.is_empty() {
        return None;
    }

    Some((level, title))
}

/// 构建 heading 路径字符串
fn build_heading_path(stack: &[HeadingInfo]) -> String {
    stack
        .iter()
        .map(|h| format!("{} {}", "#".repeat(h.level), h.title))
        .collect::<Vec<_>>()
        .join(" > ")
}

/// 构建原始 chunk（未拆分、未合并）
fn build_raw_chunk(
    source_path: &str,
    heading_path: &str,
    lines: &[&str],
    start_line: usize,
    end_line: usize,
) -> Chunk {
    let content = lines.join("\n");
    let hash = content_hash(&content);
    Chunk {
        source_path: source_path.to_string(),
        heading_path: heading_path.to_string(),
        content,
        start_line,
        end_line,
        content_hash: hash,
    }
}
