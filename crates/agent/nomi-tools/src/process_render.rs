use nomi_process_runtime::{CleanupReport, OutputSnapshot, OutputStream, ProcessOutcome};

pub(crate) const NO_OUTPUT: &str = "OUTPUT:\n(no output)\n";

pub(crate) fn render_output(output: &OutputSnapshot, missed_bytes: Option<u64>) -> String {
    let has_content = output
        .chunks
        .iter()
        .any(|chunk| !strip_ansi_sequences(&chunk.text).trim().is_empty());
    let mut chunks = output.chunks.iter().collect::<Vec<_>>();
    chunks.sort_by_key(|chunk| chunk.seq);
    let mut rendered = String::new();
    let mut current_stream = None;
    for chunk in chunks.into_iter().filter(|_| has_content) {
        if current_stream != Some(chunk.stream) {
            if !rendered.is_empty() && !rendered.ends_with('\n') {
                rendered.push('\n');
            }
            rendered.push_str(match chunk.stream {
                OutputStream::Stdout => "STDOUT:\n",
                OutputStream::Stderr => "STDERR:\n",
                OutputStream::Pty => "PTY:\n",
            });
            current_stream = Some(chunk.stream);
        }
        rendered.push_str(&strip_ansi_sequences(&chunk.text));
    }
    if rendered.is_empty() {
        rendered.push_str(NO_OUTPUT);
    }
    if missed_bytes.unwrap_or(0) > 0
        || output.dropped_bytes > 0
        || output.encoding.decode_errors > 0
        || output.encoding.source_encoding != "utf-8"
    {
        if !rendered.ends_with('\n') {
            rendered.push('\n');
        }
        let missed = missed_bytes
            .map(|bytes| format!("missed_bytes={bytes}, "))
            .unwrap_or_default();
        rendered.push_str(&format!(
            "[output metadata: {missed}dropped_bytes={}, source_encoding={}, decode_errors={}]",
            output.dropped_bytes, output.encoding.source_encoding, output.encoding.decode_errors
        ));
    }
    rendered
}

pub(crate) fn append_cleanup(content: &mut String, cleanup: &CleanupReport) {
    if cleanup.errors.is_empty() {
        return;
    }
    content.push_str("\ncleanup diagnostics: ");
    content.push_str(&cleanup.errors.join("; "));
}

pub(crate) fn outcome_summary(outcome: &ProcessOutcome) -> String {
    match outcome {
        ProcessOutcome::Exited { code, signal, .. } => {
            format!("exited code={code:?} signal={signal:?}")
        }
        ProcessOutcome::Cancelled { cleanup, .. } => {
            format!("cancelled reaped={}", cleanup.reaped)
        }
        ProcessOutcome::TimedOut { cleanup, .. } => {
            format!("timed_out reaped={}", cleanup.reaped)
        }
        ProcessOutcome::Lost {
            last_known,
            cleanup,
            ..
        } => format!(
            "lost pid={} reaped={} errors={}",
            last_known.pid,
            cleanup.reaped,
            cleanup.errors.join("; ")
        ),
        ProcessOutcome::SpawnFailed(failure) => {
            format!("spawn_failed {}: {}", failure.code, failure.message)
        }
    }
}

fn strip_ansi_sequences(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars();

    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }

        let Some(kind) = chars.next() else {
            break;
        };
        match kind {
            '[' => {
                for c in chars.by_ref() {
                    let code = c as u32;
                    if (0x40..=0x7E).contains(&code) {
                        break;
                    }
                }
            }
            ']' => {
                let mut saw_esc = false;
                for c in chars.by_ref() {
                    if c == '\u{7}' {
                        break;
                    }
                    if saw_esc && c == '\\' {
                        break;
                    }
                    saw_esc = c == '\u{1b}';
                }
            }
            _ => {
                let mut code = kind as u32;
                if !(0x40..=0x7E).contains(&code) {
                    for c in chars.by_ref() {
                        code = c as u32;
                        if (0x40..=0x7E).contains(&code) {
                            break;
                        }
                    }
                }
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use nomi_process_runtime::{EncodingMetadata, OutputChunk, OutputCursor};

    use super::*;

    fn snapshot_of(chunks: &[(OutputStream, &str)]) -> OutputSnapshot {
        OutputSnapshot {
            chunks: chunks
                .iter()
                .enumerate()
                .map(|(index, (stream, text))| OutputChunk {
                    seq: index as u64,
                    start: 0,
                    stream: *stream,
                    bytes: text.as_bytes().to_vec(),
                    text: (*text).to_owned(),
                })
                .collect(),
            next_cursor: OutputCursor::new(0),
            retained_bytes: 0,
            dropped_bytes: 0,
            encoding: EncodingMetadata {
                source_encoding: "utf-8".to_owned(),
                decode_errors: 0,
            },
        }
    }

    #[test]
    fn strips_ansi_control_sequences() {
        let snapshot = snapshot_of(&[(
            OutputStream::Pty,
            "\u{1b}[?9001h\u{1b}[?25lhello\u{1b}]0;title\u{7}\u{1b}[?25h",
        )]);
        let rendered = render_output(&snapshot, None);
        assert!(rendered.contains("PTY:\nhello"), "{rendered}");
        assert!(!rendered.contains('\u{1b}'), "{rendered}");
    }

    #[test]
    fn marks_empty_capture_explicitly() {
        for snapshot in [
            snapshot_of(&[]),
            snapshot_of(&[(OutputStream::Stdout, "\n")]),
            snapshot_of(&[(OutputStream::Stdout, "\r\n"), (OutputStream::Stderr, "  \n")]),
            snapshot_of(&[(OutputStream::Pty, "\u{1b}[?25l\u{1b}[?25h")]),
        ] {
            let rendered = render_output(&snapshot, None);
            assert_eq!(rendered, NO_OUTPUT, "{rendered:?}");
        }
    }

    #[test]
    fn keeps_blank_chunks_between_content() {
        let snapshot = snapshot_of(&[
            (OutputStream::Stdout, "first"),
            (OutputStream::Stdout, "\n"),
            (OutputStream::Stdout, "second\n"),
        ]);
        assert_eq!(render_output(&snapshot, None), "STDOUT:\nfirst\nsecond\n");
    }

    #[test]
    fn metadata_reports_missed_bytes_only_when_provided() {
        let mut snapshot = snapshot_of(&[(OutputStream::Stdout, "x\n")]);
        snapshot.dropped_bytes = 3;
        let without = render_output(&snapshot, None);
        assert!(without.contains("[output metadata: dropped_bytes=3"), "{without}");
        assert!(!without.contains("missed_bytes"), "{without}");
        let with = render_output(&snapshot, Some(9));
        assert!(with.contains("missed_bytes=9, dropped_bytes=3"), "{with}");
    }

    #[test]
    fn missed_bytes_alone_triggers_metadata() {
        let snapshot = snapshot_of(&[(OutputStream::Stdout, "x\n")]);
        assert!(!render_output(&snapshot, Some(0)).contains("metadata"));
        assert!(render_output(&snapshot, Some(5)).contains("missed_bytes=5"));
    }
}
