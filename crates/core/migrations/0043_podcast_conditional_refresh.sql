ALTER TABLE podcast_subscriptions ADD COLUMN validator_url TEXT;

CREATE INDEX podcast_subscription_receipt_job ON podcast_subscription_receipts(job_id);
CREATE INDEX podcast_refresh_receipt_job ON podcast_refresh_receipts(job_id);
