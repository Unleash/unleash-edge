use serde::Serialize;
use std::io::{self, Write};

pub const UPSTREAM_MAX_BODY_SIZE: usize = 100 * 1024; // default `express.json()` limit
pub const TARGET_BODY_SIZE: usize = 95 * 1024;

const _: () = assert!(TARGET_BODY_SIZE < UPSTREAM_MAX_BODY_SIZE);

pub fn serialized_len<T: Serialize>(value: &T) -> usize {
    let mut counter = ByteCounter { count: 0 };
    serde_json::to_writer(&mut counter, &value).unwrap(); // will panic if value cannot be serialized

    counter.count
}

struct ByteCounter {
    count: usize,
}

impl Write for ByteCounter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.count += buf.len();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_the_bytes_serde_json_would_have_written() {
        let value = vec!["one", "two"];
        assert_eq!(
            serialized_len(&value),
            serde_json::to_string(&value).unwrap().len()
        );
    }
}
