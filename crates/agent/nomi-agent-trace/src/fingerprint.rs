use std::io;

use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const DIGEST_HEX_CHARS: usize = 16;

/// Prefix-cache fingerprint of an exact request, computed before redaction or
/// truncation so it stays comparable when the captured body is elided.
///
/// `messages[i]` chains `system`, `tools`, and messages `0..=i`; the first
/// index where two requests differ is where a provider prefix cache breaks.
pub fn request_prefix_fingerprint<T: Serialize, M: Serialize>(
    system: &str,
    tools: &[T],
    messages: &[M],
) -> Value {
    let system_digest = Sha256::digest(system.as_bytes());
    let tools_digest = serialized_digest(tools);

    let mut chain = Sha256::new();
    chain.update(system_digest);
    chain.update(tools_digest);
    let mut link = chain.finalize();
    let messages: Vec<String> = messages
        .iter()
        .map(|message| {
            let mut next = Sha256::new();
            next.update(link);
            next.update(serialized_digest(message));
            link = next.finalize();
            short_hex(&link)
        })
        .collect();

    json!({
        "algorithm": "sha256-chain",
        "system": short_hex(&system_digest),
        "tools": short_hex(&tools_digest),
        "messages": messages,
    })
}

fn serialized_digest<T: Serialize + ?Sized>(value: &T) -> sha2::digest::Output<Sha256> {
    let mut writer = HashWriter(Sha256::new());
    if serde_json::to_writer(&mut writer, value).is_err() {
        return Sha256::new().finalize();
    }
    writer.0.finalize()
}

fn short_hex(digest: &[u8]) -> String {
    let mut hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    hex.truncate(DIGEST_HEX_CHARS);
    hex
}

struct HashWriter(Sha256);

impl io::Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(value: &Value) -> Vec<String> {
        value["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|hash| hash.as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn shared_prefix_hashes_match_until_first_divergent_message() {
        let tools = [json!({ "name": "Read" })];
        let before = request_prefix_fingerprint("sys", &tools, &["a", "b", "c"]);
        let after = request_prefix_fingerprint("sys", &tools, &["a", "B", "c", "d"]);

        let (before, after) = (chain(&before), chain(&after));
        assert_eq!(before[0], after[0]);
        assert_ne!(before[1], after[1]);
        assert_ne!(before[2], after[2], "a divergent message poisons the rest of the chain");
        assert_eq!(after.len(), 4);
        assert!(after.iter().all(|hash| hash.len() == DIGEST_HEX_CHARS));
    }

    #[test]
    fn system_or_tools_change_breaks_the_whole_chain() {
        let tools = [json!({ "name": "Read" })];
        let base = request_prefix_fingerprint("sys", &tools, &["a"]);
        let new_system = request_prefix_fingerprint("sys2", &tools, &["a"]);
        let new_tools = request_prefix_fingerprint("sys", &[json!({ "name": "Write" })], &["a"]);

        assert_ne!(base["system"], new_system["system"]);
        assert_eq!(base["tools"], new_system["tools"]);
        assert_ne!(chain(&base)[0], chain(&new_system)[0]);
        assert_eq!(base["system"], new_tools["system"]);
        assert_ne!(base["tools"], new_tools["tools"]);
        assert_ne!(chain(&base)[0], chain(&new_tools)[0]);
    }

    #[test]
    fn identical_requests_fingerprint_identically() {
        let tools = [json!({ "name": "Read", "input_schema": { "type": "object" } })];
        let messages = [json!({ "role": "user", "content": "hi" })];
        assert_eq!(
            request_prefix_fingerprint("sys", &tools, &messages),
            request_prefix_fingerprint("sys", &tools, &messages)
        );
    }
}
