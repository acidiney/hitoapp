CREATE TABLE podcast_unsubscribe_receipts (
    user_id TEXT NOT NULL REFERENCES users(id),
    request_id TEXT NOT NULL,
    subscription_id TEXT NOT NULL,
    PRIMARY KEY(user_id,request_id),
    FOREIGN KEY(subscription_id,user_id) REFERENCES podcast_subscriptions(id,user_id)
);
CREATE TRIGGER podcast_unsubscribe_receipt_retained BEFORE DELETE ON podcast_unsubscribe_receipts BEGIN
    SELECT RAISE(ABORT, 'Podcast unsubscribe receipts must be retained');
END;
CREATE TRIGGER podcast_unsubscribe_receipt_immutable BEFORE UPDATE ON podcast_unsubscribe_receipts BEGIN
    SELECT RAISE(ABORT, 'Podcast unsubscribe receipts are immutable');
END;
CREATE TRIGGER podcast_unsubscribe_receipt_count AFTER INSERT ON podcast_unsubscribe_receipts BEGIN
    INSERT INTO podcast_request_account_counts(user_id,receipt_count) VALUES(new.user_id,1)
    ON CONFLICT(user_id) DO UPDATE SET receipt_count=receipt_count+1;
    UPDATE podcast_request_total SET receipt_count=receipt_count+1 WHERE singleton=1;
END;
