-- How eagerly the PII detector masks. The value is the confidence
-- threshold handed to the GLiNER2 span scorer: a span is masked when
-- the model scores it at or above this number, so a LOWER value masks
-- MORE text and accepts more false positives (a company name read as a
-- person, a catalogue code read as a document number). Users set it in
-- Settings → Sicurezza.
--
-- Default 0.5 is the model's calibrated default, not a guess. The
-- previous hard-coded 0.2 was tuned against the scores of an earlier
-- engine whose prompt layout was defective: the numbers it produced
-- were plausible but wrong (a codice fiscale scored 0.5016 against a
-- 0.5 threshold), so any value tuned on them does not transfer. With
-- corrected scoring, 0.2 over-masks heavily.
--
-- Note this threshold governs *detection*, not the breadth of the
-- replacement: every literal occurrence of a detected value is masked
-- throughout the document regardless of this setting.

ALTER TABLE user_settings ADD COLUMN pii_threshold REAL NOT NULL DEFAULT 0.5;
