use agentx_application::{RuntimeError, RuntimeResult};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

pub struct SseDecoder {
    buffer: String,
    total_bytes: usize,
    max_event_bytes: usize,
    max_total_bytes: usize,
}

impl SseDecoder {
    #[must_use]
    pub fn new(max_event_bytes: usize, max_total_bytes: usize) -> Self {
        Self {
            buffer: String::new(),
            total_bytes: 0,
            max_event_bytes,
            max_total_bytes,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> RuntimeResult<Vec<SseEvent>> {
        self.total_bytes = self.total_bytes.saturating_add(chunk.len());
        if self.total_bytes > self.max_total_bytes {
            return Err(RuntimeError::new(
                "SSE_TOTAL_LIMIT",
                "SSE stream exceeded its total byte limit",
            )
            .partial(true));
        }
        self.buffer.push_str(&String::from_utf8_lossy(chunk));
        self.buffer = self.buffer.replace("\r\n", "\n").replace('\r', "\n");
        if self.buffer.len() > self.max_event_bytes && !self.buffer.contains("\n\n") {
            return Err(
                RuntimeError::new("SSE_EVENT_LIMIT", "SSE event exceeded its byte limit")
                    .partial(true),
            );
        }
        let mut result = Vec::new();
        while let Some(boundary) = self.buffer.find("\n\n") {
            if boundary > self.max_event_bytes {
                return Err(RuntimeError::new(
                    "SSE_EVENT_LIMIT",
                    "SSE event exceeded its byte limit",
                )
                .partial(true));
            }
            let raw = self.buffer[..boundary].to_owned();
            self.buffer.drain(..boundary + 2);
            let mut event = None;
            let mut data = Vec::new();
            for line in raw.lines() {
                if line.starts_with(':') {
                    continue;
                }
                if let Some(value) = line.strip_prefix("event:") {
                    event = Some(value.trim_start().to_owned());
                } else if let Some(value) = line.strip_prefix("data:") {
                    data.push(value.trim_start().to_owned());
                }
            }
            if data.is_empty() && raw.trim_start().starts_with('{') {
                data.push(raw.trim().to_owned());
            }
            if !data.is_empty() {
                result.push(SseEvent {
                    event,
                    data: data.join("\n"),
                });
            }
        }
        Ok(result)
    }

    pub fn finish(&self) -> RuntimeResult<()> {
        if self.buffer.trim().is_empty() {
            return Ok(());
        }
        Err(RuntimeError::new(
            "SSE_STREAM_INCOMPLETE",
            "SSE stream ended with an incomplete event",
        )
        .partial(true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_fragmented_crlf_and_multiline_events() {
        let mut decoder = SseDecoder::new(1024, 4096);
        assert!(
            decoder
                .push(b"event: message\r\ndata: one\r\n")
                .unwrap()
                .is_empty()
        );
        let events = decoder.push(b"data: two\r\n\r\n").unwrap();
        assert_eq!(
            events,
            vec![SseEvent {
                event: Some("message".into()),
                data: "one\ntwo".into()
            }]
        );
    }

    #[test]
    fn accepts_opensandbox_bare_json_blocks() {
        let mut decoder = SseDecoder::new(1024, 4096);
        let events = decoder
            .push(b"{\"type\":\"stdout\",\"text\":\"hello\"}\n\n")
            .unwrap();
        assert_eq!(events[0].data, "{\"type\":\"stdout\",\"text\":\"hello\"}");
    }

    #[test]
    fn rejects_single_event_and_total_stream_overflow_as_partial() {
        let mut event_limited = SseDecoder::new(8, 128);
        let error = event_limited.push(b"data: 123456789").unwrap_err();
        assert_eq!(error.code, "SSE_EVENT_LIMIT");
        assert!(error.partial);

        let mut total_limited = SseDecoder::new(64, 12);
        total_limited.push(b"data: a\n\n").unwrap();
        let error = total_limited.push(b"data: b\n\n").unwrap_err();
        assert_eq!(error.code, "SSE_TOTAL_LIMIT");
        assert!(error.partial);
    }

    #[test]
    fn reports_unterminated_final_event_as_partial() {
        let mut decoder = SseDecoder::new(1024, 4096);
        decoder.push(b"data: partial").unwrap();
        let error = decoder.finish().unwrap_err();
        assert_eq!(error.code, "SSE_STREAM_INCOMPLETE");
        assert!(error.partial);

        let mut complete = SseDecoder::new(1024, 4096);
        complete.push(b"data: complete\n\n").unwrap();
        assert!(complete.finish().is_ok());
    }
}
