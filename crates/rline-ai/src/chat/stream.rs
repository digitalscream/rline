//! Server-Sent Events (SSE) parser for streaming chat completions.
//!
//! Parses `data: {...}` lines from an SSE stream and accumulates tool call
//! argument fragments into complete [`ToolCall`](super::types::ToolCall) values.

use crate::chat::types::{ChatStreamChunk, FunctionCall, ToolCall, ToolCallDelta};
use crate::error::AiError;

/// Accumulates streaming tool call fragments into complete tool calls.
#[derive(Debug, Default)]
pub struct ToolCallAccumulator {
    /// In-progress tool calls indexed by their stream index.
    entries: Vec<ToolCallEntry>,
}

/// A single in-progress tool call being accumulated.
#[derive(Debug, Clone)]
struct ToolCallEntry {
    id: String,
    name: String,
    arguments: String,
}

impl ToolCallAccumulator {
    /// Feed a tool call delta into the accumulator.
    pub fn feed(&mut self, delta: &ToolCallDelta) {
        // Grow the entries vector if needed.
        while self.entries.len() <= delta.index {
            self.entries.push(ToolCallEntry {
                id: String::new(),
                name: String::new(),
                arguments: String::new(),
            });
        }

        let entry = &mut self.entries[delta.index];

        if let Some(id) = &delta.id {
            entry.id.clone_from(id);
        }
        if let Some(func) = &delta.function {
            if let Some(name) = &func.name {
                entry.name.clone_from(name);
            }
            if let Some(args) = &func.arguments {
                entry.arguments.push_str(args);
            }
        }
    }

    /// Consume the accumulator and return all completed tool calls.
    pub fn finish(self) -> Vec<ToolCall> {
        self.entries
            .into_iter()
            .filter(|e| !e.id.is_empty())
            .map(|e| ToolCall {
                id: e.id,
                call_type: "function".to_owned(),
                function: FunctionCall {
                    name: e.name,
                    arguments: e.arguments,
                },
            })
            .collect()
    }

    /// Whether any tool calls are being accumulated.
    pub fn has_entries(&self) -> bool {
        self.entries.iter().any(|e| !e.id.is_empty())
    }

    /// Reset the accumulator for a new response.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Parse a single SSE data line into a stream chunk.
///
/// Returns `None` for the `[DONE]` sentinel, `Some(chunk)` for valid data,
/// or an error for malformed JSON.
pub fn parse_sse_line(line: &str) -> Result<Option<ChatStreamChunk>, AiError> {
    let data = line.strip_prefix("data: ").unwrap_or(line);
    let data = data.trim();

    if data.is_empty() {
        return Ok(None);
    }
    if data == "[DONE]" {
        return Ok(None);
    }

    let chunk: ChatStreamChunk = serde_json::from_str(data)?;
    Ok(Some(chunk))
}

/// Parse a raw SSE text buffer into individual lines, returning any incomplete
/// trailing data that should be carried over to the next buffer read.
///
/// Each complete line (terminated by `\n`) is returned in the output vector.
pub fn split_sse_lines(buffer: &str) -> (Vec<&str>, &str) {
    if let Some(last_newline) = buffer.rfind('\n') {
        let complete = &buffer[..last_newline];
        let remainder = &buffer[last_newline + 1..];
        let lines: Vec<&str> = complete
            .lines()
            .filter(|l| !l.is_empty() && l.starts_with("data: "))
            .collect();
        (lines, remainder)
    } else {
        // No complete line yet.
        (Vec::new(), buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_sse_line_text_delta() {
        let line = r#"data: {"choices":[{"delta":{"content":"Hi"},"finish_reason":null}]}"#;
        let chunk = parse_sse_line(line)
            .expect("should parse")
            .expect("should not be DONE");
        assert_eq!(chunk.choices[0].delta.content.as_deref(), Some("Hi"));
    }

    #[test]
    fn test_parse_sse_line_done() {
        let result = parse_sse_line("data: [DONE]").expect("should parse");
        assert!(result.is_none(), "DONE sentinel should return None");
    }

    #[test]
    fn test_parse_sse_line_empty() {
        let result = parse_sse_line("").expect("should parse");
        assert!(result.is_none(), "empty line should return None");
    }

    #[test]
    fn test_tool_call_accumulator_single() {
        let mut acc = ToolCallAccumulator::default();

        // First delta: id + name + partial args
        acc.feed(&ToolCallDelta {
            index: 0,
            id: Some("call_1".to_owned()),
            function: Some(crate::chat::types::FunctionCallDelta {
                name: Some("read_file".to_owned()),
                arguments: Some(r#"{"path":""#.to_owned()),
            }),
        });

        // Second delta: more args
        acc.feed(&ToolCallDelta {
            index: 0,
            id: None,
            function: Some(crate::chat::types::FunctionCallDelta {
                name: None,
                arguments: Some(r#"test.txt"}"#.to_owned()),
            }),
        });

        assert!(acc.has_entries());
        let calls = acc.finish();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].id, "call_1");
        assert_eq!(calls[0].function.name, "read_file");
        assert_eq!(calls[0].function.arguments, r#"{"path":"test.txt"}"#);
    }

    #[test]
    fn test_tool_call_accumulator_multiple() {
        let mut acc = ToolCallAccumulator::default();

        acc.feed(&ToolCallDelta {
            index: 0,
            id: Some("call_1".to_owned()),
            function: Some(crate::chat::types::FunctionCallDelta {
                name: Some("read_file".to_owned()),
                arguments: Some(r#"{"path":"a.txt"}"#.to_owned()),
            }),
        });
        acc.feed(&ToolCallDelta {
            index: 1,
            id: Some("call_2".to_owned()),
            function: Some(crate::chat::types::FunctionCallDelta {
                name: Some("list_files".to_owned()),
                arguments: Some(r#"{"path":"."}"#.to_owned()),
            }),
        });

        let calls = acc.finish();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].function.name, "read_file");
        assert_eq!(calls[1].function.name, "list_files");
    }

    #[test]
    fn test_split_sse_lines_complete() {
        let buffer = "data: {\"choices\":[]}\n\ndata: [DONE]\n";
        let (lines, remainder) = split_sse_lines(buffer);
        assert_eq!(lines.len(), 2);
        assert!(remainder.is_empty());
    }

    #[test]
    fn test_split_sse_lines_incomplete() {
        let buffer = "data: {\"choices\":[";
        let (lines, remainder) = split_sse_lines(buffer);
        assert!(lines.is_empty());
        assert_eq!(remainder, buffer);
    }
}
