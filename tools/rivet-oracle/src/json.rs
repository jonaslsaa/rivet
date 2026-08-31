//! Strict JSON helpers for controller evidence.
//!
//! `serde_json` deliberately follows the common "last duplicate key wins"
//! behavior.  That is suitable for ordinary configuration, but not for
//! verifier evidence: a producer could put an honest value first and a
//! verifier-favorable value second.  The controller therefore scans every
//! object before deserializing and rejects duplicate keys at every nesting
//! level.

use std::collections::BTreeSet;

pub const MAX_JSON_BYTES: usize = 1 << 20;

pub fn from_slice<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    if bytes.len() > MAX_JSON_BYTES {
        return Err(format!(
            "JSON evidence is {} bytes, above the {}-byte cap",
            bytes.len(),
            MAX_JSON_BYTES
        ));
    }
    let mut parser = Parser { bytes, offset: 0 };
    parser.value()?;
    parser.whitespace();
    if parser.offset != bytes.len() {
        return Err(format!(
            "JSON evidence has trailing bytes at offset {}",
            parser.offset
        ));
    }
    serde_json::from_slice(bytes).map_err(|error| error.to_string())
}

struct Parser<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Parser<'a> {
    fn whitespace(&mut self) {
        while matches!(
            self.bytes.get(self.offset),
            Some(b' ' | b'\n' | b'\r' | b'\t')
        ) {
            self.offset += 1;
        }
    }

    fn value(&mut self) -> Result<(), String> {
        self.whitespace();
        match self.bytes.get(self.offset).copied() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => {
                self.string()?;
                Ok(())
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(b't') => self.literal(b"true"),
            Some(b'f') => self.literal(b"false"),
            Some(b'n') => self.literal(b"null"),
            Some(byte) => Err(format!(
                "invalid JSON value byte 0x{byte:02x} at offset {}",
                self.offset
            )),
            None => Err("unexpected end of JSON evidence".to_string()),
        }
    }

    fn object(&mut self) -> Result<(), String> {
        self.expect(b'{')?;
        self.whitespace();
        let mut keys = BTreeSet::new();
        if self.take_if(b'}') {
            return Ok(());
        }
        loop {
            self.whitespace();
            let key_offset = self.offset;
            let key = self.string()?;
            if !keys.insert(key.clone()) {
                return Err(format!(
                    "duplicate JSON object key {key:?} at offset {key_offset}"
                ));
            }
            self.whitespace();
            self.expect(b':')?;
            self.value()?;
            self.whitespace();
            if self.take_if(b'}') {
                return Ok(());
            }
            self.expect(b',')?;
        }
    }

    fn array(&mut self) -> Result<(), String> {
        self.expect(b'[')?;
        self.whitespace();
        if self.take_if(b']') {
            return Ok(());
        }
        loop {
            self.value()?;
            self.whitespace();
            if self.take_if(b']') {
                return Ok(());
            }
            self.expect(b',')?;
        }
    }

    fn string(&mut self) -> Result<String, String> {
        let start = self.offset;
        self.expect(b'"')?;
        let mut escaped = false;
        while let Some(&byte) = self.bytes.get(self.offset) {
            self.offset += 1;
            if escaped {
                escaped = false;
                if byte == b'u' {
                    for _ in 0..4 {
                        let Some(hex) = self.bytes.get(self.offset).copied() else {
                            return Err("truncated JSON unicode escape".to_string());
                        };
                        if !hex.is_ascii_hexdigit() {
                            return Err("invalid JSON unicode escape".to_string());
                        }
                        self.offset += 1;
                    }
                }
                continue;
            }
            match byte {
                b'\\' => escaped = true,
                b'"' => {
                    let raw = std::str::from_utf8(&self.bytes[start..self.offset])
                        .map_err(|_| "JSON string is not UTF-8".to_string())?;
                    return serde_json::from_str(raw)
                        .map_err(|error| format!("invalid JSON string: {error}"));
                }
                0..=0x1f => return Err("unescaped control byte in JSON string".to_string()),
                _ => {}
            }
        }
        Err("unterminated JSON string".to_string())
    }

    fn number(&mut self) -> Result<(), String> {
        let start = self.offset;
        if self.take_if(b'-') && self.bytes.get(self.offset) == Some(&b'0') {
            self.offset += 1;
        } else {
            if self.bytes.get(self.offset) == Some(&b'0') {
                self.offset += 1;
            } else {
                self.take_digits();
            }
        }
        if self.take_if(b'.') {
            let before = self.offset;
            self.take_digits();
            if before == self.offset {
                return Err("JSON number is missing fraction digits".to_string());
            }
        }
        if matches!(self.bytes.get(self.offset), Some(b'e' | b'E')) {
            self.offset += 1;
            if matches!(self.bytes.get(self.offset), Some(b'+' | b'-')) {
                self.offset += 1;
            }
            let before = self.offset;
            self.take_digits();
            if before == self.offset {
                return Err("JSON number is missing exponent digits".to_string());
            }
        }
        if start == self.offset {
            return Err(format!("invalid JSON number at offset {start}"));
        }
        Ok(())
    }

    fn take_digits(&mut self) {
        while matches!(self.bytes.get(self.offset), Some(b'0'..=b'9')) {
            self.offset += 1;
        }
    }

    fn literal(&mut self, literal: &[u8]) -> Result<(), String> {
        let end = self
            .offset
            .checked_add(literal.len())
            .ok_or_else(|| "JSON literal length overflow".to_string())?;
        if self.bytes.get(self.offset..end) == Some(literal) {
            self.offset = end;
            Ok(())
        } else {
            Err(format!("invalid JSON literal at offset {}", self.offset))
        }
    }

    fn expect(&mut self, expected: u8) -> Result<(), String> {
        if self.take_if(expected) {
            Ok(())
        } else {
            Err(format!(
                "expected JSON byte 0x{expected:02x} at offset {}",
                self.offset
            ))
        }
    }

    fn take_if(&mut self, expected: u8) -> bool {
        if self.bytes.get(self.offset) == Some(&expected) {
            self.offset += 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_duplicate_keys_at_every_depth() {
        for input in [
            &br#"{"a":1,"a":2}"#[..],
            &br#"{"a":{"b":1,"b":2}}"#[..],
            &br#"{"a":[{"b":1,"b":2}]}"#[..],
        ] {
            let error = from_slice::<serde_json::Value>(input).expect_err("duplicate key");
            assert!(error.contains("duplicate JSON object key"), "{error}");
        }
    }

    #[test]
    fn accepts_distinct_keys_and_rejects_their_decoded_collision() {
        // Distinct keys that merely share escape encodings stay accepted.
        from_slice::<serde_json::Value>(br#"{"a":1,"b":2}"#).expect("distinct keys are accepted");

        // The scanner dedups on decoded key text: `"a"` decodes to "a", so a
        // raw spelling collision must be rejected exactly like a literal
        // duplicate even though the raw bytes differ.
        let error = from_slice::<serde_json::Value>(br#"{"a":1,"a":2}"#)
            .expect_err("literal duplicate key");
        assert!(error.contains("duplicate JSON object key"));

        // Genuine decoded-key collision: a literal key and its
        // Unicode-escape equivalent ("a" also decodes to "a") use
        // different raw spellings yet must collide after decoding. A scanner
        // that deduplicated on raw spelling would accept this and let
        // serde_json's last-wins policy pick the second value.
        let error = from_slice::<serde_json::Value>(br#"{"a":1,"\u0061":2}"#)
            .expect_err("decoded collision between literal and unicode-escaped key");
        assert!(error.contains("duplicate JSON object key"));

        // Same collision at nesting depth, where a per-top-level-object scan
        // would miss it.
        let error = from_slice::<serde_json::Value>(br#"{"outer":{"a":1,"\u0061":2}}"#)
            .expect_err("decoded collision nested one level deep");
        assert!(error.contains("duplicate JSON object key"));
    }

    #[test]
    fn rejects_trailing_and_oversized_json() {
        assert!(from_slice::<serde_json::Value>(br#"{} {}"#).is_err());
        let bytes = vec![b' '; MAX_JSON_BYTES + 1];
        assert!(from_slice::<serde_json::Value>(&bytes).is_err());
    }
}
