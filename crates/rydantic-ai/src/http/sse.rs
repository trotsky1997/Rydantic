use crate::{Error, Result};
use serde_json::Value;

pub(super) enum Frame {
    Data(Value),
    Done,
}

/// Incremental SSE decoder: handles split UTF-8, CRLF, comments and multiline data.
#[derive(Default)]
pub(super) struct SseDecoder {
    buffer: Vec<u8>,
    data: Vec<u8>,
}
impl SseDecoder {
    fn line(&mut self, line: &[u8], output: &mut Vec<Frame>) -> Result<()> {
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if line.is_empty() {
            if !self.data.is_empty() {
                if self.data == b"[DONE]" {
                    output.push(Frame::Done);
                } else {
                    output.push(Frame::Data(
                        serde_json::from_slice(&self.data)
                            .map_err(|_| Error::Protocol("invalid SSE JSON".into()))?,
                    ));
                }
                self.data.clear();
            }
        } else if let Some(data) = line.strip_prefix(b"data:") {
            let data = data.strip_prefix(b" ").unwrap_or(data);
            if !self.data.is_empty() {
                self.data.push(b'\n');
            }
            self.data.extend_from_slice(data);
            if self.data.len() > 1024 * 1024 {
                return Err(Error::Limit("SSE event exceeds 1 MiB".into()));
            }
        }
        Ok(())
    }
    pub(super) fn feed(&mut self, bytes: &[u8]) -> Result<Vec<Frame>> {
        self.buffer.extend_from_slice(bytes);
        let mut output = Vec::new();
        while let Some(index) = self.buffer.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=index).collect();
            self.line(&line[..line.len() - 1], &mut output)?;
        }
        if self.buffer.len() > 1024 * 1024 {
            return Err(Error::Limit("SSE line exceeds 1 MiB".into()));
        }
        Ok(output)
    }
    pub(super) fn finish(mut self) -> Result<Vec<Frame>> {
        let mut result = Vec::new();
        let last = std::mem::take(&mut self.buffer);
        if !last.is_empty() {
            self.line(&last, &mut result)?;
        }
        self.line(b"", &mut result)?;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sse_handles_every_byte_boundary() {
        let bytes = ":ping\r\nevent: x\r\ndata: {\"type\":\"文字\",\r\ndata: \"delta\":\"😊\"}\r\n\r\ndata: [DONE]\n\n".as_bytes();
        for split in 0..bytes.len() {
            let mut decoder = SseDecoder::default();
            let mut out = decoder.feed(&bytes[..split]).unwrap();
            out.extend(decoder.feed(&bytes[split..]).unwrap());
            out.extend(decoder.finish().unwrap());
            assert_eq!(out.len(), 2);
            assert!(
                matches!(&out[0], Frame::Data(value) if value == &serde_json::json!({"type":"文字","delta":"😊"}))
            );
            assert!(matches!(&out[1], Frame::Done));
        }
    }
}
