use serde_json::Value;
use std::io::{self, BufRead, BufReader, Read, Write};
use tiny_http::Request;

// tiny_http buffers small writes. Flush each HTTP chunk so a short token arrives immediately.
pub(super) struct Stream(Box<dyn Write + Send>);

impl Stream {
    pub fn open(req: Request) -> io::Result<Self> {
        let mut writer = req.into_writer();
        writer.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/x-ndjson; charset=utf-8\r\nCache-Control: no-store, no-transform\r\nX-Accel-Buffering: no\r\nX-Content-Type-Options: nosniff\r\nTransfer-Encoding: chunked\r\n\r\n")?;
        writer.flush()?;
        Ok(Self(writer))
    }

    pub fn event(&mut self, event: Value) -> io::Result<()> {
        let data = event.to_string();
        write!(self.0, "{:x}\r\n{}\n\r\n", data.len() + 1, data)?;
        self.0.flush()
    }

    pub fn finish(mut self) -> io::Result<()> {
        self.0.write_all(b"0\r\n\r\n")?;
        self.0.flush()
    }
}

pub(super) fn read_completion(
    body: impl Read,
    structured: bool,
    mut emit: impl FnMut(&str) -> io::Result<()>,
) -> io::Result<String> {
    let mut reader = BufReader::new(body.take(512 * 1024));
    let mut line = String::new();
    let mut event = String::new();
    let mut text = String::new();
    let mut sent = 0;
    let mut finished = false;
    while reader.read_line(&mut line)? != 0 {
        let value = line.trim_end_matches(['\r', '\n']);
        if let Some(data) = value.strip_prefix("data:") {
            if !event.is_empty() {
                event.push('\n');
            }
            event.push_str(data.strip_prefix(' ').unwrap_or(data));
        } else if value.is_empty() && !event.is_empty() {
            if event == "[DONE]" {
                return Ok(text);
            }
            let frame: Value = serde_json::from_str(&event)?;
            if frame.get("error").is_some() {
                return Err(io::Error::other("Error del proveedor"));
            }
            match frame
                .pointer("/choices/0/finish_reason")
                .and_then(Value::as_str)
            {
                Some("stop") => finished = true,
                Some(_) => return Err(io::Error::other("Respuesta incompleta")),
                None => {}
            }
            if let Some(part) = frame
                .pointer("/choices/0/delta/content")
                .and_then(Value::as_str)
            {
                text.push_str(part);
                let visible = if structured {
                    response_prefix(&text)
                } else {
                    Some(text.clone())
                };
                if let Some(visible) = visible.filter(|visible| visible.len() > sent) {
                    emit(&visible[sent..])?;
                    sent = visible.len();
                }
            }
            event.clear();
        }
        line.clear();
    }
    if finished {
        Ok(text)
    } else {
        Err(io::Error::other("Flujo incompleto"))
    }
}

fn response_prefix(input: &str) -> Option<String> {
    let value = input
        .trim_start()
        .strip_prefix('{')?
        .trim_start()
        .strip_prefix("\"response\"")?
        .trim_start()
        .strip_prefix(':')?
        .trim_start()
        .strip_prefix('"')?;
    let mut escaped = false;
    let mut end = value.len();
    for (index, ch) in value.char_indices() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            end = index;
            break;
        }
    }
    // An escape can straddle chunks, including both halves of a Unicode surrogate pair.
    let mut partial = &value[..end];
    for _ in 0..=2 {
        if let Ok(text) = serde_json::from_str(&format!("\"{partial}\"")) {
            return Some(text);
        }
        partial = &partial[..partial.rfind('\\')?];
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_only_response_text_and_waits_for_complete_escapes() {
        let parts = [
            r#"{"response":"Hola "#,
            r#"\uD83D"#,
            r#"\uDE00\n"#,
            r#"mundo","changes":[]}"#,
        ];
        let mut input = String::new();
        for part in parts {
            input.push_str(&format!(
                "data: {}\r\n\r\n",
                serde_json::json!({"choices":[{"delta":{"content":part}}]})
            ));
        }
        input.push_str("data: [DONE]\r\n\r\n");
        let mut chunks = Vec::new();
        let text = read_completion(input.as_bytes(), true, |part| {
            chunks.push(part.to_owned());
            Ok(())
        })
        .unwrap();
        assert_eq!(chunks, ["Hola ", "😀\n", "mundo"]);
        assert!(super::super::chats::Reply::parse(&text).is_ok());
        assert!(read_completion(b"data: {}\n\n".as_slice(), false, |_| Ok(())).is_err());
    }
}
