-- Form registry and data tables in schema riverbase_form.

CREATE SCHEMA IF NOT EXISTS riverbase_form;

CREATE TABLE IF NOT EXISTS riverbase_form.element_registry (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    serial_no BIGINT GENERATED ALWAYS AS IDENTITY,
    element_key TEXT NOT NULL,
    element_label TEXT,
    element_schema JSONB NOT NULL DEFAULT '{}'::jsonb
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_element_registry_key
    ON riverbase_form.element_registry (element_key)
    WHERE _deleted IS NULL;

CREATE TABLE IF NOT EXISTS riverbase_form.form_registry (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    serial_no BIGINT GENERATED ALWAYS AS IDENTITY,
    form_key TEXT NOT NULL,
    title TEXT NOT NULL,
    "desc" TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_form_registry_key
    ON riverbase_form.form_registry (form_key)
    WHERE _deleted IS NULL;

CREATE TABLE IF NOT EXISTS riverbase_form.template_registry (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    serial_no BIGINT GENERATED ALWAYS AS IDENTITY,
    template_key TEXT NOT NULL,
    template_name TEXT NOT NULL,
    "desc" TEXT,
    version INT NOT NULL DEFAULT 1,
    types TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_template_registry_key
    ON riverbase_form.template_registry (template_key)
    WHERE _deleted IS NULL;

CREATE TABLE IF NOT EXISTS riverbase_form.collection (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    collection_key TEXT NOT NULL,
    collection_name TEXT NOT NULL,
    "desc" TEXT,
    owner_id UUID,
    organization_id TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_collection_key
    ON riverbase_form.collection (collection_key)
    WHERE _deleted IS NULL;

CREATE TABLE IF NOT EXISTS riverbase_form.document (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    template_id UUID,
    document_key TEXT NOT NULL,
    document_name TEXT NOT NULL,
    "desc" TEXT,
    version INT NOT NULL DEFAULT 1,
    owner_id UUID,
    organization_id TEXT,
    resource_id UUID,
    resource_name TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_document_key
    ON riverbase_form.document (document_key)
    WHERE _deleted IS NULL;

CREATE TABLE IF NOT EXISTS riverbase_form.document_collection (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    document_id UUID NOT NULL,
    collection_id UUID NOT NULL,
    sort_order INT NOT NULL DEFAULT 0
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_document_collection
    ON riverbase_form.document_collection (document_id, collection_id)
    WHERE _deleted IS NULL;

CREATE TABLE IF NOT EXISTS riverbase_form.document_node (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    document_id UUID NOT NULL,
    parent_node UUID,
    node_key TEXT NOT NULL,
    form_key TEXT,
    node_type TEXT NOT NULL DEFAULT 'section',
    sort_order INT NOT NULL DEFAULT 0,
    title TEXT,
    "desc" TEXT,
    content TEXT,
    content_type TEXT,
    attrs JSONB
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_document_node_key
    ON riverbase_form.document_node (document_id, node_key)
    WHERE _deleted IS NULL;

CREATE TABLE IF NOT EXISTS riverbase_form.form_submission (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    document_id UUID NOT NULL,
    form_reg_id UUID NOT NULL,
    title TEXT NOT NULL,
    "desc" TEXT,
    sort_order INT NOT NULL DEFAULT 0,
    locked BOOLEAN NOT NULL DEFAULT false,
    status TEXT NOT NULL DEFAULT 'draft'
);

CREATE TABLE IF NOT EXISTS riverbase_form.form_element (
    _id UUID PRIMARY KEY,
    _created TIMESTAMPTZ NOT NULL DEFAULT now(),
    _updated TIMESTAMPTZ,
    _creator UUID,
    _updater UUID,
    _realm UUID,
    _etag UUID,
    _deleted TIMESTAMPTZ,
    form_id UUID NOT NULL,
    elem_reg_id UUID NOT NULL,
    elem_name TEXT NOT NULL,
    index INT NOT NULL DEFAULT -1,
    required BOOLEAN NOT NULL DEFAULT false,
    data JSONB,
    status TEXT NOT NULL DEFAULT 'draft'
);
