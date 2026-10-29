//! Bounded result capture is independent of complete result syntax validation.
//! The validation worker consumes the entire pipe without building a Value or
//! retaining a large string. Its channel holds at most four 8 KiB read chunks.
use crate::config::OutputFormat;
use serde::Deserialize;
use std::io::{self, BufRead, Read};
use tokio::{sync::mpsc, task::JoinHandle};

pub(crate) const RETAIN_BYTES: usize = 1024 * 1024;

pub(crate) fn preview(text: &str) -> serde_json::Value {
    preview_with_limit(text, 64 * 1024)
}

fn preview_with_limit(text: &str, limit: usize) -> serde_json::Value {
    let mut end = text.len().min(limit);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    serde_json::json!({"truncated":true,"preview":&text[..end]})
}

/// Keep a value only while it fits its presentation/transport budget.
pub(crate) fn bounded_value(value: serde_json::Value, limit: usize) -> serde_json::Value {
    bounded_value_with_preview(value, limit, 64 * 1024)
}

pub(crate) fn bounded_value_with_preview(
    value: serde_json::Value,
    limit: usize,
    preview_limit: usize,
) -> serde_json::Value {
    if serde_json::to_vec(&value).is_ok_and(|encoded| encoded.len() <= limit) {
        return value;
    }
    preview_with_limit(
        &value
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| value.to_string()),
        preview_limit,
    )
}

pub(crate) struct Validation {
    sender: Option<mpsc::Sender<Vec<u8>>>,
    worker: JoinHandle<Result<(), String>>,
}
impl Validation {
    pub(crate) fn new(format: &OutputFormat) -> Option<Self> {
        let jsonl = match format {
            OutputFormat::Json => false,
            OutputFormat::Jsonl => true,
            _ => return None,
        };
        let (sender, receiver) = mpsc::channel(4);
        let worker = tokio::task::spawn_blocking(move || {
            let mut reader = io::BufReader::new(Chunks {
                receiver,
                current: io::Cursor::new(Vec::new()),
                syntax: Lexical::default(),
            });
            let result = if jsonl {
                validate_lines(&mut reader)
            } else {
                validate_value(&mut reader)
            };
            result.map_err(|e| format!("invalid JSON{} output: {e}", if jsonl { "L" } else { "" }))
        });
        Some(Self {
            sender: Some(sender),
            worker,
        })
    }
    pub(crate) async fn push(&mut self, bytes: &[u8]) {
        if let Some(sender) = &self.sender
            && sender.send(bytes.to_vec()).await.is_err()
        {
            // A semantic error ends validation, not stdout draining. Otherwise
            // the Agent could block on its full pipe before it can terminate.
            self.sender = None;
        }
    }
    pub(crate) async fn finish(mut self) -> Option<String> {
        self.sender.take();
        match self.worker.await {
            Ok(Ok(())) => None,
            Ok(Err(error)) => Some(error),
            Err(error) => Some(format!("output validation could not complete: {error}")),
        }
    }
}
fn validate_value(reader: impl Read) -> serde_json::Result<()> {
    let mut deserializer = serde_json::Deserializer::from_reader(reader);
    serde::de::IgnoredAny::deserialize(&mut deserializer)?;
    deserializer.end()
}
fn validate_lines(reader: &mut impl BufRead) -> serde_json::Result<()> {
    while !reader.fill_buf().map_err(serde_json::Error::io)?.is_empty() {
        validate_value(Line {
            reader,
            ended: false,
            available: 0,
            line_end: false,
        })?;
    }
    Ok(())
}
/// Presents exactly one JSONL line as a reader, without retaining the line.
struct Line<'a, R> {
    reader: &'a mut R,
    ended: bool,
    available: usize,
    line_end: bool,
}
impl<R: BufRead> Read for Line<'_, R> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if self.ended || output.is_empty() {
            return Ok(0);
        }
        let bytes = self.reader.fill_buf()?;
        if self.available == 0 {
            self.available = bytes
                .iter()
                .position(|b| *b == b'\n')
                .map_or(bytes.len(), |n| n + 1);
            self.line_end = self.available > 0 && bytes[self.available - 1] == b'\n';
        }
        let count = self.available.min(output.len());
        output[..count].copy_from_slice(&bytes[..count]);
        self.available -= count;
        self.ended = self.available == 0 && self.line_end;
        self.reader.consume(count);
        Ok(count)
    }
}
struct Chunks {
    receiver: mpsc::Receiver<Vec<u8>>,
    current: io::Cursor<Vec<u8>>,
    syntax: Lexical,
}
impl Read for Chunks {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        loop {
            let count = self.current.read(output)?;
            if count > 0 {
                return Ok(count);
            }
            match self.receiver.blocking_recv() {
                Some(bytes) => {
                    self.syntax.push(&bytes)?;
                    self.current = io::Cursor::new(bytes);
                }
                None => {
                    if self.syntax.utf8_remaining != 0 {
                        return Err(invalid("incomplete UTF-8 in JSON output"));
                    }
                    return Ok(0);
                }
            }
        }
    }
}
fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
#[derive(Default)]
enum Escape {
    #[default]
    None,
    Slash,
    Hex {
        remaining: u8,
        value: u16,
        low: bool,
    },
    LowSlash,
    LowU,
}
/// IgnoredAny intentionally permits arbitrary string bytes and lone surrogate
/// escapes, and stores one byte per nesting level. Enforce the same Unicode
/// string/depth validity as Value while keeping every validator buffer bounded.
#[derive(Default)]
struct Lexical {
    string: bool,
    escape: Escape,
    depth: usize,
    utf8_remaining: u8,
    utf8_min: u8,
    utf8_max: u8,
}
impl Lexical {
    fn push(&mut self, bytes: &[u8]) -> io::Result<()> {
        for &byte in bytes {
            if self.utf8_remaining > 0 {
                if !(self.utf8_min..=self.utf8_max).contains(&byte) {
                    return Err(invalid("invalid UTF-8 in JSON output"));
                }
                self.utf8_remaining -= 1;
                self.utf8_min = 0x80;
                self.utf8_max = 0xbf;
            } else if byte >= 0x80 {
                (self.utf8_remaining, self.utf8_min, self.utf8_max) = match byte {
                    0xc2..=0xdf => (1, 0x80, 0xbf),
                    0xe0 => (2, 0xa0, 0xbf),
                    0xe1..=0xec | 0xee..=0xef => (2, 0x80, 0xbf),
                    0xed => (2, 0x80, 0x9f),
                    0xf0 => (3, 0x90, 0xbf),
                    0xf1..=0xf3 => (3, 0x80, 0xbf),
                    0xf4 => (3, 0x80, 0x8f),
                    _ => return Err(invalid("invalid UTF-8 in JSON output")),
                };
            }
            if !self.string {
                match byte {
                    b'"' => self.string = true,
                    b'[' | b'{' => {
                        self.depth += 1;
                        if self.depth >= 128 {
                            return Err(invalid("JSON recursion limit exceeded"));
                        }
                    }
                    b']' | b'}' => self.depth = self.depth.saturating_sub(1),
                    _ => {}
                }
                continue;
            }
            self.escape = match std::mem::take(&mut self.escape) {
                Escape::None => match byte {
                    b'"' => {
                        self.string = false;
                        Escape::None
                    }
                    b'\\' => Escape::Slash,
                    _ => Escape::None,
                },
                Escape::Slash => {
                    if byte == b'u' {
                        Escape::Hex {
                            remaining: 4,
                            value: 0,
                            low: false,
                        }
                    } else {
                        Escape::None
                    }
                }
                Escape::LowSlash => {
                    if byte != b'\\' {
                        return Err(invalid("unpaired JSON surrogate"));
                    }
                    Escape::LowU
                }
                Escape::LowU => {
                    if byte != b'u' {
                        return Err(invalid("unpaired JSON surrogate"));
                    }
                    Escape::Hex {
                        remaining: 4,
                        value: 0,
                        low: true,
                    }
                }
                Escape::Hex {
                    remaining,
                    value,
                    low,
                } => {
                    let digit = (byte as char)
                        .to_digit(16)
                        .ok_or_else(|| invalid("invalid JSON Unicode escape"))?;
                    let value = (value << 4) | digit as u16;
                    if remaining > 1 {
                        Escape::Hex {
                            remaining: remaining - 1,
                            value,
                            low,
                        }
                    } else if low {
                        if !(0xdc00..=0xdfff).contains(&value) {
                            return Err(invalid("unpaired JSON surrogate"));
                        }
                        Escape::None
                    } else if (0xd800..=0xdbff).contains(&value) {
                        Escape::LowSlash
                    } else {
                        if (0xdc00..=0xdfff).contains(&value) {
                            return Err(invalid("unpaired JSON surrogate"));
                        }
                        Escape::None
                    }
                }
            };
        }
        Ok(())
    }
}
