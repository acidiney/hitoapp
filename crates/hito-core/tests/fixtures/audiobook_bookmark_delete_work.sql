INSERT INTO playlists(id,user_id,name)
SELECT 'work-book-'||i,'work-user-'||(i%10),'Owned work book '||i
FROM hito_audiobook_work_numbers;

INSERT INTO audiobooks(playlist_id,user_id)
SELECT 'work-book-'||i,'work-user-'||(i%10) FROM hito_audiobook_work_numbers;

INSERT INTO playlist_entries(playlist_id,id,recording_id,position)
SELECT 'work-book-'||i,'work-chapter-'||i,'work-recording',0 FROM hito_audiobook_work_numbers;

WITH RECURSIVE marker(i) AS (VALUES(0) UNION ALL SELECT i+1 FROM marker WHERE i<249)
INSERT INTO audiobook_bookmarks(user_id,id,audiobook_id,entry_id,recording_id,position_ms,label,deleted)
SELECT 'work-user-'||(book.i%10),'work-marker-'||book.i||'-'||marker.i,
    'work-book-'||book.i,'work-chapter-'||book.i,'work-recording',marker.i*1000,
    'Owned work bookmark '||marker.i,marker.i%2
FROM hito_audiobook_work_numbers book CROSS JOIN marker;
