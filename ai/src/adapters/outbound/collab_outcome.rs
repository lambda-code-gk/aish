use std::io::{self, BufRead, BufReader, IsTerminal, Write};

use crate::domain::{parse_collab_outcome_status, CollabOutcome};
use crate::ports::outbound::{CollabOutcomeCollectionError, CollabOutcomeCollector};

const STATUS_PROMPT: &str = "\
Human Shellを終了しました。
作業結果を選択してください。

  [d] done       作業を完了した
  [b] blocked    作業を完了できなかった
  [c] cancelled  作業を中止した

> ";

#[derive(Debug, Default)]
pub struct TerminalCollabOutcomeCollector;

impl CollabOutcomeCollector for TerminalCollabOutcomeCollector {
    fn collect(&self) -> Result<CollabOutcome, CollabOutcomeCollectionError> {
        let stdin = io::stdin();
        if !stdin.is_terminal() {
            return Err(CollabOutcomeCollectionError::NonInteractiveStdin);
        }
        let mut reader = BufReader::new(stdin.lock());
        let stderr = io::stderr();
        collect_collab_outcome_from_streams(&mut reader, &mut stderr.lock())
    }
}

pub fn collect_collab_outcome_from_streams(
    reader: &mut dyn BufRead,
    writer: &mut dyn Write,
) -> Result<CollabOutcome, CollabOutcomeCollectionError> {
    writer.write_all(STATUS_PROMPT.as_bytes())?;
    writer.flush()?;
    loop {
        let input = read_line(reader)?;
        match parse_collab_outcome_status(&input) {
            Ok(status) => return Ok(CollabOutcome::new(status)),
            Err(_) => {
                writer.write_all("d、b、cのいずれかを入力してください。\n> ".as_bytes())?;
                writer.flush()?;
            }
        }
    }
}

fn read_line(reader: &mut dyn BufRead) -> Result<String, CollabOutcomeCollectionError> {
    let mut line = String::new();
    if reader.read_line(&mut line)? == 0 {
        return Err(CollabOutcomeCollectionError::UnexpectedEof);
    }
    Ok(line)
}
