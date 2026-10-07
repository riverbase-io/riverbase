-- Media metadata store: schema riverbase_media, table entry (shared domain fields).

CREATE SCHEMA IF NOT EXISTS riverbase_media;

CREATE TABLE IF NOT EXISTS riverbase_media.entry (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    filename TEXT NOT NULL,
    filehash TEXT,
    filemime TEXT,
    fskey TEXT,
    length BIGINT NOT NULL,
    fspath TEXT,
    compress TEXT,
    resource TEXT,
    resource_id UUID,
    resource_sid UUID,
    resource_iid UUID,
    xattrs TEXT,
    cdn_exp TIMESTAMPTZ,
    cdn_url TEXT
);

CREATE INDEX IF NOT EXISTS idx_entry_resource ON riverbase_media.entry(resource);
CREATE INDEX IF NOT EXISTS idx_entry_resource_id ON riverbase_media.entry(resource_id);
CREATE INDEX IF NOT EXISTS idx_entry_created ON riverbase_media.entry(_created);
