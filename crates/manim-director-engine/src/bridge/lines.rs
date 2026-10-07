//! Bounded, cancel-safe line reading for the runtime's pipes.

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};

const STDERR_TAIL_BYTES: usize = 64 * 1024;

pub(super) enum Line {
    Complete(Vec<u8>),
    /// `max` bytes without a newline; the rest of the line follows.
    Overflow(Vec<u8>),
}

impl Line {
    pub(super) fn bytes(&self) -> &[u8] {
        match self {
            Self::Complete(bytes) | Self::Overflow(bytes) => bytes,
        }
    }
}

/// A partially read line survives a dropped `next()` future because it lives
/// in `partial`, so a reader can sit in a `select!` indefinitely.
pub(super) struct LineReader {
    reader: Option<BufReader<Box<dyn AsyncRead + Send + Unpin>>>,
    partial: Vec<u8>,
    max: usize,
    pub(super) open: bool,
}

impl LineReader {
    pub(super) fn new(pipe: Option<impl AsyncRead + Send + Unpin + 'static>, max: usize) -> Self {
        let reader =
            pipe.map(|pipe| BufReader::new(Box::new(pipe) as Box<dyn AsyncRead + Send + Unpin>));
        Self {
            open: reader.is_some(),
            reader,
            partial: Vec::new(),
            max,
        }
    }

    /// The next line without its terminator, or `None` at end of stream.
    pub(super) async fn next(&mut self) -> Option<Line> {
        let reader = self.reader.as_mut()?;
        loop {
            let buffer = match reader.fill_buf().await {
                Ok(buffer) => buffer,
                Err(_) => &[],
            };
            if buffer.is_empty() {
                self.open = false;
                return (!self.partial.is_empty())
                    .then(|| Line::Complete(std::mem::take(&mut self.partial)));
            }
            let room = self.max - self.partial.len();
            match buffer.iter().position(|byte| *byte == b'\n') {
                Some(index) if index <= room => {
                    self.partial.extend_from_slice(&buffer[..index]);
                    reader.consume(index + 1);
                    let mut line = std::mem::take(&mut self.partial);
                    if line.last() == Some(&b'\r') {
                        line.pop();
                    }
                    return Some(Line::Complete(line));
                }
                _ if buffer.len() > room => {
                    self.partial.extend_from_slice(&buffer[..room]);
                    reader.consume(room);
                    return Some(Line::Overflow(std::mem::take(&mut self.partial)));
                }
                _ => {
                    let count = buffer.len();
                    self.partial.extend_from_slice(buffer);
                    reader.consume(count);
                }
            }
        }
    }
}

/// The last 64 KiB of stderr, kept for failure data.
#[derive(Default)]
pub(super) struct StderrTail {
    pub(super) text: String,
}

impl StderrTail {
    pub(super) fn push(&mut self, line: &str) {
        self.text.push_str(line);
        self.text.push('\n');
        if self.text.len() > STDERR_TAIL_BYTES {
            let mut cut = self.text.len() - STDERR_TAIL_BYTES;
            while !self.text.is_char_boundary(cut) {
                cut += 1;
            }
            self.text.drain(..cut);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn line_reader_splits_overlong_lines_without_losing_bytes() {
        let source: &'static [u8] = b"abcdefghij\nxy\nlast";
        let mut reader = LineReader::new(Some(source), 4);
        let mut seen = Vec::new();
        while let Some(line) = reader.next().await {
            let tag = match line {
                Line::Complete(_) => "line",
                Line::Overflow(_) => "chunk",
            };
            seen.push((tag, String::from_utf8(line.bytes().to_vec()).unwrap()));
        }
        assert_eq!(
            seen,
            [
                ("chunk", "abcd".to_owned()),
                ("chunk", "efgh".to_owned()),
                ("line", "ij".to_owned()),
                ("line", "xy".to_owned()),
                ("line", "last".to_owned()),
            ]
        );
    }

    #[test]
    fn stderr_tail_keeps_the_last_64_kib() {
        let mut tail = StderrTail::default();
        for index in 0..2_000 {
            tail.push(&format!("{index:05} {}", "x".repeat(60)));
        }
        assert!(tail.text.len() <= STDERR_TAIL_BYTES);
        assert!(tail.text.ends_with(&format!("01999 {}\n", "x".repeat(60))));
    }
}
