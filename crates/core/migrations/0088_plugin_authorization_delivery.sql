ALTER TABLE plugin_authorization_attempts ADD COLUMN delivery TEXT NOT NULL DEFAULT 'session' CHECK (delivery IN ('session','loopback'));
