-- Runs of startup tasks, in order: see `indexer_common::startup`. Each row is a run that completed,
-- with the task version it reached and the state it returned, as JSON. Times are Unix
-- milliseconds.
CREATE TABLE journal (
  seq INTEGER PRIMARY KEY,
  id TEXT NOT NULL,
  version INTEGER NOT NULL,
  state TEXT NOT NULL,
  started_at INTEGER NOT NULL,
  ended_at INTEGER NOT NULL
);

CREATE INDEX journal_id_seq_idx ON journal (id, seq);
