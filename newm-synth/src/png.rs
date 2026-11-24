//! A small PNG writer: per-row filter choice, then deflate with fixed Huffman codes and greedy
//! LZ77 matching, so stills need no image dependency.

use std::path::Path;

pub fn write(path: &Path, px: &[u32], w: usize, h: usize) -> std::io::Result<()> {
    std::fs::write(path, encode(px, w, h))
}

/// `px` (RGBA pixels as [`crate::pack`] makes them, `w × h`, no padding) as PNG bytes.
pub fn encode(px: &[u32], w: usize, h: usize) -> Vec<u8> {
    let stride = w * 3;
    let mut raw = Vec::with_capacity((stride + 1) * h);
    let mut prev = vec![0u8; stride];
    let mut cur = vec![0u8; stride];
    let mut cand = [vec![0u8; stride], vec![0u8; stride], vec![0u8; stride]];
    for y in 0..h {
        for (x, p) in px[y * w..y * w + w].iter().enumerate() {
            cur[x * 3..x * 3 + 3].copy_from_slice(&p.to_le_bytes()[..3]);
        }
        for i in 0..stride {
            let left = if i >= 3 { cur[i - 3] } else { 0 };
            cand[0][i] = cur[i];
            cand[1][i] = cur[i].wrapping_sub(left);
            cand[2][i] = cur[i].wrapping_sub(prev[i]);
        }
        let cost = |v: &Vec<u8>| {
            v.iter()
                .map(|&b| u32::from((b as i8).unsigned_abs()))
                .sum::<u32>()
        };
        let best = (0..3).min_by_key(|&f| cost(&cand[f])).unwrap();
        raw.push(best as u8);
        raw.extend_from_slice(&cand[best]);
        std::mem::swap(&mut prev, &mut cur);
    }

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&(w as u32).to_be_bytes());
    ihdr.extend_from_slice(&(h as u32).to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &zlib(&raw));
    chunk(&mut png, b"IEND", &[]);
    png
}

fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let start = out.len();
    out.extend_from_slice(kind);
    out.extend_from_slice(data);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (n, t) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xedb8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    !data.iter().fold(!0u32, |c, &b| {
        table[((c ^ u32::from(b)) & 0xff) as usize] ^ (c >> 8)
    })
}

struct Bits {
    out: Vec<u8>,
    acc: u64,
    n: u32,
}

impl Bits {
    fn put(&mut self, v: u32, len: u32) {
        self.acc |= u64::from(v) << self.n;
        self.n += len;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// A Huffman code, which deflate stores most significant bit first.
    fn code(&mut self, code: u32, len: u32) {
        self.put(code.reverse_bits() >> (32 - len), len);
    }

    fn literal(&mut self, v: u32) {
        match v {
            0..=143 => self.code(0x30 + v, 8),
            144..=255 => self.code(0x190 + v - 144, 9),
            256..=279 => self.code(v - 256, 7),
            _ => self.code(0xc0 + v - 280, 8),
        }
    }
}

const LEN_BASE: [u32; 29] = [
    3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131,
    163, 195, 227, 258,
];
const LEN_EXTRA: [u32; 29] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0,
];
const DIST_BASE: [u32; 30] = [
    1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537,
    2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577,
];
const DIST_EXTRA: [u32; 30] = [
    0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13,
    13,
];

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut b = Bits {
        out: vec![0x78, 0x01],
        acc: 0,
        n: 0,
    };
    // One final block with fixed codes.
    b.put(1, 1);
    b.put(1, 2);

    const WINDOW: usize = 32768;
    let mut head = vec![usize::MAX; 1 << 15];
    let hash = |i: usize| {
        ((u32::from(data[i]) << 10 ^ u32::from(data[i + 1]) << 5 ^ u32::from(data[i + 2])) & 0x7fff)
            as usize
    };
    let mut i = 0;
    while i < data.len() {
        let mut best = (0, 0);
        if i + 3 <= data.len() {
            let h = hash(i);
            let max = (data.len() - i).min(258);
            for cand in [head[h], i.wrapping_sub(3), i.wrapping_sub(1)] {
                if cand >= i || i - cand > WINDOW {
                    continue;
                }
                let mut l = 0;
                while l < max && data[cand + l] == data[i + l] {
                    l += 1;
                }
                if l > best.0 {
                    best = (l, i - cand);
                }
            }
            head[h] = i;
        }
        if best.0 >= 3 {
            let (len, dist) = best;
            let lc = LEN_BASE.iter().rposition(|&x| x <= len as u32).unwrap();
            b.literal(257 + lc as u32);
            b.put(len as u32 - LEN_BASE[lc], LEN_EXTRA[lc]);
            let dc = DIST_BASE.iter().rposition(|&x| x <= dist as u32).unwrap();
            b.code(dc as u32, 5);
            b.put(dist as u32 - DIST_BASE[dc], DIST_EXTRA[dc]);
            for j in i + 1..(i + len).min(data.len().saturating_sub(2)) {
                head[hash(j)] = j;
            }
            i += len;
        } else {
            b.literal(u32::from(data[i]));
            i += 1;
        }
    }
    b.literal(256);
    if b.n > 0 {
        b.put(0, 8 - b.n);
    }

    let (mut s1, mut s2) = (1u32, 0u32);
    for &x in data {
        s1 = (s1 + u32::from(x)) % 65521;
        s2 = (s2 + s1) % 65521;
    }
    let mut out = b.out;
    out.extend_from_slice(&(s2 << 16 | s1).to_be_bytes());
    out
}
