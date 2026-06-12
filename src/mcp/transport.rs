use crate::error::{Error, Result};
use std::io::{self, BufRead, Write};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageFraming {
    JsonLines,
    ContentLength,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingMessage {
    pub body: String,
    pub framing: MessageFraming,
}

/// Read one JSON-RPC message from stdin.
///
/// MCP stdio uses newline-delimited JSON. Content-Length framing remains
/// accepted for compatibility with older integrations and release artifacts.
pub fn read_framed_message<R: BufRead>(reader: &mut R) -> Result<Option<IncomingMessage>> {
    let mut first_line = String::new();
    if reader.read_line(&mut first_line)? == 0 {
        return Ok(None);
    }
    let first = first_line.trim_end_matches(['\r', '\n']);

    if first.trim_start().starts_with('{') {
        return Ok(Some(IncomingMessage {
            body: first.to_string(),
            framing: MessageFraming::JsonLines,
        }));
    }

    let mut headers = String::from(first);
    headers.push('\n');
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        headers.push_str(trimmed);
        headers.push('\n');
    }

    if let Some(len) = parse_content_length(&headers) {
        let mut body = vec![0u8; len];
        reader.read_exact(&mut body)?;
        return Ok(Some(IncomingMessage {
            body: String::from_utf8(body).map_err(|e| Error::Other(e.to_string()))?,
            framing: MessageFraming::ContentLength,
        }));
    }

    Err(Error::Other("unsupported MCP stdio framing".into()))
}

pub fn read_message<R: BufRead>(reader: &mut R) -> Result<Option<String>> {
    Ok(read_framed_message(reader)?.map(|message| message.body))
}

fn parse_content_length(headers: &str) -> Option<usize> {
    for line in headers.lines() {
        let lower = line.to_lowercase();
        if lower.starts_with("content-length:") {
            return lower.split(':').nth(1)?.trim().parse().ok();
        }
    }
    None
}

/// Write JSON-RPC response with Content-Length framing.
pub fn write_message<W: Write>(writer: &mut W, body: &str) -> Result<()> {
    writeln!(writer, "Content-Length: {}", body.len())?;
    writeln!(writer)?;
    writer.write_all(body.as_bytes())?;
    writer.flush()?;
    Ok(())
}

pub fn write_framed_message<W: Write>(
    writer: &mut W,
    body: &str,
    framing: MessageFraming,
) -> Result<()> {
    match framing {
        MessageFraming::JsonLines => {
            writeln!(writer, "{body}")?;
            writer.flush()?;
            Ok(())
        }
        MessageFraming::ContentLength => write_message(writer, body),
    }
}

pub fn read_stdin_message() -> Result<Option<IncomingMessage>> {
    let stdin = io::stdin();
    let mut lock = stdin.lock();
    read_framed_message(&mut lock)
}

pub fn write_stdout_message(body: &str, framing: MessageFraming) -> Result<()> {
    let mut stdout = io::stdout();
    write_framed_message(&mut stdout, body, framing)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parses_content_length_frame() {
        let data = "Content-Length: 13\r\n\r\n{\"json\":true}";
        let mut cursor = Cursor::new(data.as_bytes());
        let msg = read_message(&mut cursor).unwrap().unwrap();
        assert_eq!(msg, "{\"json\":true}");
    }

    #[test]
    fn parses_each_json_line_without_consuming_the_next_message() {
        let data = "{\"id\":1}\n{\"id\":2}\n";
        let mut cursor = Cursor::new(data.as_bytes());

        let first = read_framed_message(&mut cursor).unwrap().unwrap();
        let second = read_framed_message(&mut cursor).unwrap().unwrap();

        assert_eq!(first.body, "{\"id\":1}");
        assert_eq!(second.body, "{\"id\":2}");
        assert_eq!(first.framing, MessageFraming::JsonLines);
        assert_eq!(second.framing, MessageFraming::JsonLines);
    }

    #[test]
    fn writes_json_line_response() {
        let mut output = Vec::new();
        write_framed_message(&mut output, "{\"id\":1}", MessageFraming::JsonLines).unwrap();
        assert_eq!(output, b"{\"id\":1}\n");
    }
}
