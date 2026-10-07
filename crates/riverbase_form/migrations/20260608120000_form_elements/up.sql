-- Generated element data tables.

CREATE SCHEMA IF NOT EXISTS riverbase_form;

CREATE TABLE IF NOT EXISTS riverbase_form.text_input_data (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    value TEXT NOT NULL
);
