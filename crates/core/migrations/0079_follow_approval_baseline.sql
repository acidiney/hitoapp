ALTER TABLE playlist_follows ADD COLUMN approved_capabilities TEXT;
ALTER TABLE playlist_follows ADD COLUMN approved_credential TEXT;
UPDATE playlist_follows SET
    approved_capabilities = (SELECT json_extract(i.manifest,'$.capabilities') FROM plugin_installations i
        WHERE i.id = playlist_follows.installation_id AND i.revision = playlist_follows.installation_revision),
    approved_credential = (SELECT CASE WHEN i.credential IS NULL THEN NULL ELSE hex(substr(i.credential,34,24)) END
        FROM plugin_installations i WHERE i.id = playlist_follows.installation_id AND i.revision = playlist_follows.installation_revision);
