ALTER TABLE plugin_authorization_attempts ADD COLUMN status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending','exchanging','completed','failed'));
