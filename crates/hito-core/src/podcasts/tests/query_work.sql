CREATE TEMP TABLE podcast_request_work_numbers(i INTEGER PRIMARY KEY);
WITH RECURSIVE n(i) AS (
    VALUES(0) UNION ALL SELECT i + 1 FROM n WHERE i < 59999
)
INSERT INTO podcast_request_work_numbers SELECT i FROM n;

INSERT INTO users(id,username,password_hash)
SELECT printf('90000000-0000-4000-8000-%012x',n.i),
    'podcast-history-account-' || n.i, owner.password_hash
FROM podcast_request_work_numbers n
CROSS JOIN podcast_request_work_fixture fixture
JOIN users owner ON owner.id=fixture.user_id
WHERE n.i BETWEEN 1 AND 16;

INSERT INTO libraries(id,name,owner_id)
SELECT printf('91000000-0000-4000-8000-%012x',n.i),
    'Owned podcast history',printf('90000000-0000-4000-8000-%012x',n.i)
FROM podcast_request_work_numbers n WHERE n.i BETWEEN 1 AND 16;

INSERT INTO library_members(library_id,user_id)
SELECT id,owner_id FROM libraries WHERE id LIKE '91000000-%';

INSERT INTO podcast_subscriptions(id,user_id,library_id,access_grant_id,feed_url,fetch_url)
SELECT printf('92000000-0000-4000-8000-%012x',n.i),
    library.owner_id,library.id,grant_row.id,fixture.feed_url,fixture.feed_url
FROM podcast_request_work_numbers n
JOIN libraries library ON library.id=printf('91000000-0000-4000-8000-%012x',n.i)
JOIN library_access_grants grant_row
    ON grant_row.library_id=library.id AND grant_row.user_id=library.owner_id
CROSS JOIN podcast_request_work_fixture fixture
WHERE n.i BETWEEN 1 AND 16;

INSERT INTO podcast_refresh_jobs(
    id,subscription_id,user_id,generation,status,attempts,created_at,updated_at)
SELECT printf('93000000-0000-4000-8000-%012x',i),
    printf('92000000-0000-4000-8000-%012x',i / 3750 + 1),
    printf('90000000-0000-4000-8000-%012x',i / 3750 + 1),
    1,CASE i % 3 WHEN 0 THEN 'complete' WHEN 1 THEN 'failed' ELSE 'cancelled' END,
    3,201,201
FROM podcast_request_work_numbers;

INSERT INTO podcast_subscription_receipts(
    user_id,request_id,request_digest,subscription_id,job_id)
SELECT printf('90000000-0000-4000-8000-%012x',n.i / 3750 + 1),
    printf('96000000-0000-4000-8000-%012x',n.i),fixture.request_digest,
    printf('92000000-0000-4000-8000-%012x',n.i / 3750 + 1),
    printf('93000000-0000-4000-8000-%012x',n.i)
FROM podcast_request_work_numbers n CROSS JOIN podcast_request_work_fixture fixture
WHERE n.i % 2=0;

INSERT INTO podcast_refresh_receipts(user_id,request_id,subscription_id,job_id)
SELECT printf('90000000-0000-4000-8000-%012x',i / 3750 + 1),
    printf('96000000-0000-4000-8000-%012x',i),
    printf('92000000-0000-4000-8000-%012x',i / 3750 + 1),
    printf('93000000-0000-4000-8000-%012x',i)
FROM podcast_request_work_numbers WHERE i % 2=1;

UPDATE podcast_refresh_jobs
SET status='complete',attempts=1,created_at=200,updated_at=200
WHERE subscription_id=(SELECT subscription_id FROM podcast_request_work_fixture);

INSERT INTO podcast_refresh_jobs(
    id,subscription_id,user_id,generation,status,attempts,created_at,updated_at)
SELECT older_job_id,subscription_id,user_id,1,'complete',1,201,201
FROM podcast_request_work_fixture;

INSERT INTO podcast_refresh_jobs(
    id,subscription_id,user_id,generation,status,attempts,created_at,updated_at)
SELECT latest_job_id,subscription_id,user_id,1,'failed',3,201,201
FROM podcast_request_work_fixture;

INSERT INTO podcast_subscription_receipts(
    user_id,request_id,request_digest,subscription_id,job_id)
SELECT user_id,'95000000-0000-4000-8000-000000000001',request_digest,
    subscription_id,older_job_id FROM podcast_request_work_fixture;

INSERT INTO podcast_refresh_receipts(user_id,request_id,subscription_id,job_id)
SELECT user_id,'95000000-0000-4000-8000-000000000002',subscription_id,latest_job_id
FROM podcast_request_work_fixture;

DROP TABLE podcast_request_work_numbers;
DROP TABLE podcast_request_work_fixture;
