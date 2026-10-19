/// Drop OSC/DCS/APC/PM strings before the terminal parser. No clipboard, title,
/// hyperlink, image, shell integration or window-control bridge is implemented.
/// CSI buffering is bounded; ordinary UTF-8 passes unchanged, including split
/// multi-byte characters. This filters 7-bit ESC strings, not Unicode bytes.
#[derive(Default, Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Filter {
    state: u8,
    osc: bool,
    escape: bool,
    csi: Vec<u8>,
    utf8: u8,
}
impl Filter {
    pub(crate) fn valid_checkpoint(&self) -> bool {
        self.state <= 4
            && self.utf8 <= 3
            && self.csi.len() <= 256
            && (self.state != 3 || self.csi.starts_with(b"\x1b["))
    }

    pub fn push(&mut self, input: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(input.len());
        for &b in input {
            match self.state {
                0 => {
                    if self.utf8 > 0 && (0x80..=0xbf).contains(&b) {
                        out.push(b);
                        self.utf8 -= 1;
                        continue;
                    }
                    self.utf8 = 0;
                    match b {
                        0x1b => self.state = 1,
                        0x90 | 0x9d | 0x9e | 0x9f => {
                            self.state = 2;
                            self.osc = b == 0x9d;
                            self.escape = false;
                        }
                        0x9b => {
                            self.state = 3;
                            self.csi.clear();
                            self.csi.extend_from_slice(b"\x1b[");
                        }
                        0x9c => {}
                        _ => {
                            self.utf8 = match b {
                                0xc2..=0xdf => 1,
                                0xe0..=0xef => 2,
                                0xf0..=0xf4 => 3,
                                _ => 0,
                            };
                            out.push(b);
                        }
                    }
                }
                1 => match b {
                    b']' | b'P' | b'_' | b'^' => {
                        self.state = 2;
                        self.osc = b == b']';
                        self.escape = false;
                    }
                    b'[' => {
                        self.state = 3;
                        self.csi.clear();
                        self.csi.extend_from_slice(b"\x1b[");
                    }
                    _ => {
                        out.extend_from_slice(&[0x1b, b]);
                        self.state = 0;
                    }
                },
                2 => {
                    if (self.osc && b == 7) || b == 0x9c || (self.escape && b == b'\\') {
                        self.state = 0;
                        self.escape = false;
                    } else {
                        self.escape = b == 0x1b;
                    }
                }
                3 => {
                    self.csi.push(b);
                    if (0x40..=0x7e).contains(&b) {
                        out.extend_from_slice(&self.csi);
                        self.csi.clear();
                        self.state = 0;
                    } else if self.csi.len() > 256 {
                        self.csi.clear();
                        self.state = 4;
                    }
                }
                4 => {
                    if (0x40..=0x7e).contains(&b) {
                        self.state = 0;
                    }
                }
                _ => self.state = 0,
            }
        }
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn osc_and_dcs_never_reach_parser() {
        let mut f = Filter::default();
        assert_eq!(f.push(b"a\x1b]52;c;secret\x07b\x1bPimage\x1b\\c"), b"abc");
    }
    #[test]
    fn string_can_cross_arbitrary_chunks() {
        let mut f = Filter::default();
        assert_eq!(f.push(b"x\x1b]"), b"x");
        assert!(f.push(&vec![b'z'; 1_000_000]).is_empty());
        assert_eq!(f.push(b"\x1b\\y"), b"y");
    }
    #[test]
    fn utf8_and_sgr_are_preserved() {
        let mut f = Filter::default();
        let b = "界\u{301}\x1b[31mred".as_bytes();
        let out = b.iter().flat_map(|x| f.push(&[*x])).collect::<Vec<_>>();
        assert_eq!(out, b);
    }
    #[test]
    fn raw_c1_strings_are_discarded() {
        let mut f = Filter::default();
        assert_eq!(f.push(b"a\x9dsecret\x9cb\x90image\x9cc"), b"abc");
    }
    #[test]
    fn utf8_c1_continuation_is_not_split() {
        let mut f = Filter::default();
        let original = "🙂œ界".as_bytes();
        let actual = original
            .iter()
            .flat_map(|b| f.push(&[*b]))
            .collect::<Vec<_>>();
        assert_eq!(actual, original);
    }
    #[test]
    fn oversized_csi_is_bounded() {
        let mut f = Filter::default();
        f.push(b"\x1b[");
        f.push(&vec![b'1'; 2048]);
        assert!(f.csi.len() <= 256);
        assert_eq!(f.push(b"mplain"), b"plain");
    }
}
