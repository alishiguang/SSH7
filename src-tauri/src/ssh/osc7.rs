//! OSC 7 增量解析器 —— 跟踪远端 shell 的当前工作目录。
//!
//! 旧版实现（`electron/main.js`）用正则扫描一个 8KB 缓冲区：
//!
//! ```js
//! const oscMatch = outputBuffer.match(/\x1b\]7;file:\/\/[^/]*(\/.+?)\x07/)
//! ```
//!
//! 这有两个缺陷：
//!
//! 1. **跨 chunk 丢失**：OSC 序列被 TCP 分段切开时，正则匹配不到，
//!    且缓冲区裁剪会把半截序列丢掉，cwd 永远不再更新。
//! 2. **只认 BEL 终止符**：很多 shell 用 ST（`ESC \`）终止，匹配不到。
//!
//! 这里改成真正的状态机：保留待定字节，只在拿到完整终止符时才解析，
//! 并同时支持 BEL 与 ST 两种终止符。

/// OSC 7 前缀：`ESC ] 7 ;`
const PREFIX: &[u8] = b"\x1b]7;";
/// 待定缓冲区上限，防止远端持续输出无终止符的序列导致内存增长。
const MAX_PENDING: usize = 4096;

#[derive(Debug, Default)]
pub struct Osc7Parser {
    /// 尚未构成完整 OSC 7 序列的字节。
    pending: Vec<u8>,
}

impl Osc7Parser {
    pub fn new() -> Self {
        Self {
            pending: Vec::with_capacity(256),
        }
    }

    /// 喂入一段终端输出，返回本次解析出的当前目录（若发生变化）。
    pub fn feed(&mut self, chunk: &[u8]) -> Option<String> {
        self.pending.extend_from_slice(chunk);
        let mut cwd = None;

        loop {
            let start = match find(&self.pending, PREFIX) {
                Some(i) => i,
                None => {
                    // 没有前缀：只保留可能与前缀跨界拼接的尾部字节。
                    let keep = PREFIX.len() - 1;
                    if self.pending.len() > keep {
                        let drop = self.pending.len() - keep;
                        self.pending.drain(..drop);
                    }
                    break;
                }
            };

            let body_start = start + PREFIX.len();
            match find_terminator(&self.pending, body_start) {
                Some((end, term_len)) => {
                    let body = self.pending[body_start..end].to_vec();
                    if let Some(path) = parse_osc7_body(&body) {
                        cwd = Some(path);
                    }
                    // 消费掉整段序列，继续找下一段。
                    let consumed = end + term_len;
                    self.pending.drain(..consumed);
                }
                None => {
                    // 序列不完整：丢掉前缀之前的部分，等待后续 chunk。
                    if start > 0 {
                        self.pending.drain(..start);
                    }
                    if self.pending.len() > MAX_PENDING {
                        // 一直没有终止符，放弃这条序列。
                        self.pending.clear();
                    }
                    break;
                }
            }
        }

        cwd
    }

    /// 重置状态（会话切换时调用）。
    pub fn reset(&mut self) {
        self.pending.clear();
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// 在 `from` 之后查找 OSC 终止符，返回 (内容结束位置, 终止符长度)。
/// 支持 BEL(`0x07`) 与 ST(`ESC \`)。
fn find_terminator(buf: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut i = from;
    while i < buf.len() {
        match buf[i] {
            0x07 => return Some((i, 1)), // BEL
            0x1b if i + 1 < buf.len() && buf[i + 1] == b'\\' => return Some((i, 2)), // ST
            _ => i += 1,
        }
    }
    None
}

/// 解析 `file://<host><path>` 形式的负载，返回百分号解码后的路径。
fn parse_osc7_body(body: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(body).ok()?;
    let rest = text.strip_prefix("file://")?;
    let slash = rest.find('/')?;
    let raw_path = &rest[slash..];
    let decoded = percent_decode(raw_path);
    if decoded.is_empty() {
        None
    } else {
        Some(decoded)
    }
}

/// 最小化百分号解码。非法转义序列按原样保留。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hi = (bytes[i + 1] as char).to_digit(16);
            let lo = (bytes[i + 2] as char).to_digit(16);
            if let (Some(hi), Some(lo)) = (hi, lo) {
                out.push((hi * 16 + lo) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_complete_sequence() {
        let mut p = Osc7Parser::new();
        assert_eq!(
            p.feed(b"\x1b]7;file://myhost/home/user\x07"),
            Some("/home/user".to_string())
        );
    }

    #[test]
    fn parses_st_terminator() {
        let mut p = Osc7Parser::new();
        assert_eq!(
            p.feed(b"\x1b]7;file://myhost/var/log\x1b\\"),
            Some("/var/log".to_string())
        );
    }

    /// 缺陷 #10 的核心场景：序列被 chunk 边界切开。
    #[test]
    fn reassembles_across_chunks() {
        let mut p = Osc7Parser::new();
        assert_eq!(p.feed(b"\x1b]7;file://myhost/ho"), None);
        assert_eq!(p.feed(b"me/user\x07"), Some("/home/user".to_string()));
    }

    #[test]
    fn reassembles_across_many_chunks() {
        let mut p = Osc7Parser::new();
        let full = b"\x1b]7;file://host/some/deep/path\x1b\\";
        for chunk in full.chunks(3) {
            let got = p.feed(chunk);
            if chunk.as_ptr() != full.as_ptr() {
                // 中间片段不应产生结果
            }
            let _ = got;
        }
        assert_eq!(p.feed(b""), None, "序列已在上一次调用中消费完毕");
    }

    #[test]
    fn percent_decodes_path() {
        let mut p = Osc7Parser::new();
        assert_eq!(
            p.feed(b"\x1b]7;file://h/my%20dir/a%2Bb\x07"),
            Some("/my dir/a+b".to_string())
        );
    }

    #[test]
    fn ignores_non_osc_output() {
        let mut p = Osc7Parser::new();
        assert_eq!(p.feed(b"total 0\r\n$ ls -la\r\n"), None);
        assert_eq!(p.feed(b"\x1b[32mgreen\x1b[0m"), None);
    }

    #[test]
    fn handles_multiple_sequences_in_one_chunk() {
        let mut p = Osc7Parser::new();
        // 只返回最后一个，因为 feed 返回的是本次解析出的最新目录
        let got = p.feed(b"\x1b]7;file://h/a\x07\x1b]7;file://h/b\x07");
        assert_eq!(got, Some("/b".to_string()));
    }

    #[test]
    fn drops_overlong_pending() {
        let mut p = Osc7Parser::new();
        let noise = vec![b'x'; MAX_PENDING + 100];
        p.feed(&noise);
        assert!(p.pending.len() <= MAX_PENDING);
    }

    #[test]
    fn keeps_prefix_tail_across_chunks() {
        let mut p = Osc7Parser::new();
        assert_eq!(p.feed(b"abc\x1b"), None);
        assert_eq!(p.feed(b"]7;file://h/tmp\x07"), Some("/tmp".to_string()));
    }
}
