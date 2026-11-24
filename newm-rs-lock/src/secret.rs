//! A password buffer that never reallocates and is wiped when shortened or dropped.

use std::fmt;
use std::sync::atomic::{compiler_fence, Ordering};

pub const CAPACITY: usize = 511;

pub struct Secret {
    buf: Vec<u8>,
}

impl Secret {
    pub fn new() -> Self {
        Self {
            buf: Vec::with_capacity(CAPACITY),
        }
    }

    pub fn push_str(&mut self, text: &str) -> bool {
        if self.buf.len() + text.len() > CAPACITY {
            return false;
        }
        self.buf.extend_from_slice(text.as_bytes());
        true
    }

    pub fn pop_char(&mut self) -> bool {
        let Some(start) = self.buf.iter().rposition(|b| b & 0xC0 != 0x80) else {
            return false;
        };
        wipe(&mut self.buf[start..]);
        self.buf.truncate(start);
        true
    }

    pub fn clear(&mut self) {
        wipe(&mut self.buf);
        self.buf.clear();
    }

    pub fn take(&mut self) -> Secret {
        std::mem::take(self)
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn char_count(&self) -> usize {
        self.buf.iter().filter(|b| **b & 0xC0 != 0x80).count()
    }
}

impl Default for Secret {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        let ptr = self.buf.as_mut_ptr();
        for i in 0..self.buf.capacity() {
            unsafe { std::ptr::write_volatile(ptr.add(i), 0) };
        }
        compiler_fence(Ordering::SeqCst);
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Secret({} chars)", self.char_count())
    }
}

pub fn wipe(bytes: &mut [u8]) {
    for b in bytes {
        unsafe { std::ptr::write_volatile(b, 0) };
    }
    compiler_fence(Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_pop_counts_chars_not_bytes() {
        let mut s = Secret::new();
        assert!(s.push_str("a"));
        assert!(s.push_str("é"));
        assert!(s.push_str("€"));
        assert_eq!(s.char_count(), 3);
        assert_eq!(s.as_bytes().len(), 6);
        assert!(s.pop_char());
        assert_eq!(s.as_bytes(), "aé".as_bytes());
        assert!(s.pop_char());
        assert!(s.pop_char());
        assert!(!s.pop_char());
        assert!(s.is_empty());
    }

    #[test]
    fn pop_wipes_the_removed_bytes() {
        let mut s = Secret::new();
        s.push_str("xy");
        s.pop_char();
        let spare = unsafe { *s.buf.as_ptr().add(1) };
        assert_eq!(spare, 0);
    }

    #[test]
    fn never_grows_past_capacity() {
        let mut s = Secret::new();
        let cap = s.buf.capacity();
        for _ in 0..CAPACITY {
            assert!(s.push_str("a"));
        }
        assert!(!s.push_str("a"));
        assert_eq!(s.buf.capacity(), cap);
    }

    #[test]
    fn take_leaves_an_empty_buffer() {
        let mut s = Secret::new();
        s.push_str("hunter2");
        let taken = s.take();
        assert!(s.is_empty());
        assert_eq!(taken.as_bytes(), b"hunter2");
    }
}
