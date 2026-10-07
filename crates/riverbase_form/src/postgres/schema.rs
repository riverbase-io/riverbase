//! Diesel schemas for form fixed tables.

riverbase_core::diesel_table_with_domain_fields! {
    riverbase_form.collection (_id) {
        collection_key -> Text,
        collection_name -> Text,
        desc -> Nullable<Text>,
        owner_id -> Nullable<Uuid>,
        organization_id -> Nullable<Text>,
    }
}

riverbase_core::diesel_table_with_domain_fields! {
    riverbase_form.document (_id) {
        template_id -> Nullable<Uuid>,
        document_key -> Text,
        document_name -> Text,
        desc -> Nullable<Text>,
        version -> Int4,
        owner_id -> Nullable<Uuid>,
        organization_id -> Nullable<Text>,
        resource_id -> Nullable<Uuid>,
        resource_name -> Nullable<Text>,
    }
}

riverbase_core::diesel_table_with_domain_fields! {
    #[sql_name = "document_collection"]
    riverbase_form.document_collection (_id) {
        document_id -> Uuid,
        collection_id -> Uuid,
        sort_order -> Int4,
    }
}

riverbase_core::diesel_table_with_domain_fields! {
    riverbase_form.document_node (_id) {
        document_id -> Uuid,
        parent_node -> Nullable<Uuid>,
        node_key -> Text,
        form_key -> Nullable<Text>,
        node_type -> Text,
        sort_order -> Int4,
        title -> Nullable<Text>,
        desc -> Nullable<Text>,
        content -> Nullable<Text>,
        content_type -> Nullable<Text>,
        attrs -> Nullable<Jsonb>,
    }
}

riverbase_core::diesel_table_with_domain_fields! {
    riverbase_form.form_submission (_id) {
        document_id -> Uuid,
        form_reg_id -> Uuid,
        title -> Text,
        desc -> Nullable<Text>,
        sort_order -> Int4,
        locked -> Bool,
        status -> Text,
    }
}

riverbase_core::diesel_table_with_domain_fields! {
    riverbase_form.form_element (_id) {
        form_id -> Uuid,
        elem_reg_id -> Uuid,
        elem_name -> Text,
        index -> Int4,
        required -> Bool,
        data -> Nullable<Jsonb>,
        status -> Text,
    }
}

riverbase_core::diesel_table_with_domain_fields! {
    riverbase_form.template_registry (_id) {
        serial_no -> Int8,
        template_key -> Text,
        template_name -> Text,
        desc -> Nullable<Text>,
        version -> Int4,
        types -> Nullable<Text>,
    }
}

riverbase_core::diesel_table_with_domain_fields! {
    riverbase_form.form_registry (_id) {
        serial_no -> Int8,
        form_key -> Text,
        title -> Text,
        desc -> Nullable<Text>,
    }
}

riverbase_core::diesel_table_with_domain_fields! {
    riverbase_form.element_registry (_id) {
        serial_no -> Int8,
        element_key -> Text,
        element_label -> Nullable<Text>,
        element_schema -> Jsonb,
    }
}

pub const RIVERBASE_FORM_SCHEMA: &str = "riverbase_form";
