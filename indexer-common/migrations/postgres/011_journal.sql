-- Runs of startup tasks, in order: see `indexer_common::startup`. Each row is a run that completed,
-- with the task version it reached and the state it returned. Times are Unix milliseconds.
CREATE TABLE journal (
  seq BIGSERIAL PRIMARY KEY,
  id TEXT NOT NULL,
  version BIGINT NOT NULL,
  state JSONB NOT NULL,
  started_at BIGINT NOT NULL,
  ended_at BIGINT NOT NULL
);

CREATE INDEX journal_id_seq_idx ON journal (id, seq);
