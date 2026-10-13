//! Versioned, note-free material identity shared by warning-producing services.
use crate::local_values::LocalInterval;
use sha2::{Digest, Sha256};

pub struct MaterialFacts(Sha256);
impl MaterialFacts {
    pub fn new(operation: &str, code: &str) -> Self {
        let mut facts = Self(Sha256::new());
        facts.text("shift-happens-material-v1");
        facts.text(operation);
        facts.text(code);
        facts
    }
    pub fn text(&mut self, value: &str) {
        self.0.update((value.len() as u64).to_be_bytes());
        self.0.update(value.as_bytes());
    }
    pub fn interval(&mut self, value: LocalInterval) {
        self.text(&value.start().format());
        self.text(&value.end().format());
    }
    pub fn finish(self) -> String {
        let mut encoded = String::from("v1:");
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in self.0.finalize() {
            encoded.push(HEX[usize::from(byte >> 4)] as char);
            encoded.push(HEX[usize::from(byte & 15)] as char);
        }
        encoded
    }
}
