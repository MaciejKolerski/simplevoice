-- Initial schema for SimpleVoice

CREATE TABLE IF NOT EXISTS transcriptions (
    id TEXT PRIMARY KEY,
    timestamp TEXT NOT NULL,
    date TEXT NOT NULL,
    text TEXT NOT NULL,
    model TEXT NOT NULL,
    wav_path TEXT,
    duration_sec REAL
);

CREATE INDEX IF NOT EXISTS idx_transcriptions_date ON transcriptions(date);
CREATE INDEX IF NOT EXISTS idx_transcriptions_timestamp ON transcriptions(timestamp);

CREATE TABLE IF NOT EXISTS daily_usage (
    date TEXT PRIMARY KEY, -- YYYY-MM-DD
    words_generated INTEGER DEFAULT 0,
    time_transcribed_sec REAL DEFAULT 0
);
