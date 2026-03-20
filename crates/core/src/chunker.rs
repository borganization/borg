use crate::tokenizer::estimate_tokens;

/// A chunk of content with line position metadata.
#[derive(Debug, Clone)]
pub struct Chunk {
    pub content: String,
    pub start_line: usize,
    pub end_line: usize,
}

/// Split content into chunks of approximately `chunk_size_tokens` tokens,
/// with `overlap_tokens` of trailing content from the previous chunk prepended.
///
/// Algorithm: split by double-newline into paragraphs, greedily accumulate
/// until `chunk_size_tokens`, then start a new chunk with overlap from the
/// previous chunk's trailing content.
pub fn chunk_content(text: &str, chunk_size_tokens: usize, overlap_tokens: usize) -> Vec<Chunk> {
    if text.is_empty() {
        return Vec::new();
    }

    let lines: Vec<&str> = text.lines().collect();
    if lines.is_empty() {
        return Vec::new();
    }

    // Split into paragraphs (separated by blank lines)
    let mut paragraphs: Vec<(String, usize, usize)> = Vec::new(); // (content, start_line, end_line)
    let mut current_para = String::new();
    let mut para_start = 1usize; // 1-indexed
    let mut line_idx = 0usize;

    for (i, line) in lines.iter().enumerate() {
        if line.trim().is_empty() && !current_para.is_empty() {
            let end_line = i; // exclusive, but we use i (0-indexed) so end_line = i means line i is blank
            paragraphs.push((current_para.clone(), para_start, end_line));
            current_para.clear();
            para_start = i + 2; // next non-blank line (1-indexed)
        } else if !line.trim().is_empty() {
            if current_para.is_empty() {
                para_start = i + 1; // 1-indexed
            } else {
                current_para.push('\n');
            }
            current_para.push_str(line);
            line_idx = i;
        }
    }
    // Don't forget the last paragraph
    if !current_para.is_empty() {
        paragraphs.push((current_para, para_start, line_idx + 1));
    }

    if paragraphs.is_empty() {
        return Vec::new();
    }

    // Greedily accumulate paragraphs into chunks
    let mut chunks: Vec<Chunk> = Vec::new();
    let mut current_content = String::new();
    let mut current_start = paragraphs[0].1;
    let mut current_end = paragraphs[0].2;
    let mut current_tokens = 0usize;

    for (para_text, para_start, para_end) in &paragraphs {
        let para_tokens = estimate_tokens(para_text);

        if current_tokens > 0 && current_tokens + para_tokens > chunk_size_tokens {
            // Emit current chunk
            chunks.push(Chunk {
                content: current_content.clone(),
                start_line: current_start,
                end_line: current_end,
            });

            // Start new chunk with overlap
            if overlap_tokens > 0 {
                let overlap_content = extract_trailing_tokens(&current_content, overlap_tokens);
                current_content = overlap_content;
                if !current_content.is_empty() {
                    current_content.push_str("\n\n");
                }
                current_tokens = estimate_tokens(&current_content);
            } else {
                current_content.clear();
                current_tokens = 0;
            }
            current_start = *para_start;
        }

        if !current_content.is_empty() && current_tokens > 0 {
            current_content.push_str("\n\n");
        }
        current_content.push_str(para_text);
        current_tokens = estimate_tokens(&current_content);
        current_end = *para_end;
    }

    // Emit final chunk
    if !current_content.is_empty() {
        chunks.push(Chunk {
            content: current_content,
            start_line: current_start,
            end_line: current_end,
        });
    }

    chunks
}

/// Extract trailing content from text that is approximately `target_tokens` tokens.
fn extract_trailing_tokens(text: &str, target_tokens: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let mut result_lines: Vec<&str> = Vec::new();
    let mut tokens = 0;

    for line in lines.iter().rev() {
        let line_tokens = estimate_tokens(line);
        if tokens + line_tokens > target_tokens && !result_lines.is_empty() {
            break;
        }
        result_lines.push(line);
        tokens += line_tokens;
    }

    result_lines.reverse();
    result_lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_empty_content() {
        let chunks = chunk_content("", 400, 80);
        assert!(chunks.is_empty());
    }

    #[test]
    fn chunk_short_content_single_chunk() {
        let text = "Hello world.\n\nThis is a short memory.";
        let chunks = chunk_content(text, 400, 80);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].start_line, 1);
    }

    #[test]
    fn chunk_respects_size_limit() {
        // Build content with many paragraphs that exceed 400 tokens
        let paragraph = "A ".repeat(100); // ~100 tokens
        let text = (0..10)
            .map(|_| paragraph.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        let chunks = chunk_content(&text, 400, 80);
        assert!(chunks.len() > 1, "should split into multiple chunks");
        for chunk in &chunks {
            let tokens = estimate_tokens(&chunk.content);
            assert!(
                tokens <= 500,
                "chunk should not greatly exceed limit: {tokens}"
            );
        }
    }

    #[test]
    fn chunk_overlap_shares_content() {
        let paragraph = "Word ".repeat(120); // ~120 tokens per paragraph
        let text = (0..5)
            .map(|_| paragraph.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        let chunks = chunk_content(&text, 200, 50);
        assert!(chunks.len() >= 2);
    }

    #[test]
    fn chunk_line_numbers_are_monotonic() {
        // Use fewer, larger paragraphs to keep test fast (estimate_tokens is O(n))
        let paragraph = "Word ".repeat(60); // ~60 tokens each
        let text = (0..8)
            .map(|_| paragraph.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        let chunks = chunk_content(&text, 200, 0);
        assert!(chunks.len() >= 2, "should have multiple chunks");
        for i in 1..chunks.len() {
            assert!(
                chunks[i].start_line >= chunks[i - 1].start_line,
                "start_line should be monotonically increasing"
            );
        }
    }

    #[test]
    fn chunk_zero_overlap() {
        let paragraph = "Word ".repeat(120);
        let text = (0..5)
            .map(|_| paragraph.clone())
            .collect::<Vec<_>>()
            .join("\n\n");
        let chunks = chunk_content(&text, 200, 0);
        assert!(chunks.len() >= 2);
    }
}
