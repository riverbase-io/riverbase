use riverbase_core::diesel_table_with_domain_fields;

diesel_table_with_domain_fields! {
    #[sql_name = "entry"]
    riverbase_media.media_entries (_id) {
        filename -> Text,
        filehash -> Nullable<Text>,
        filemime -> Nullable<Text>,
        fskey -> Nullable<Text>,
        length -> Int8,
        fspath -> Nullable<Text>,
        compress -> Nullable<Text>,
        resource -> Nullable<Text>,
        resource_id -> Nullable<Uuid>,
        resource_sid -> Nullable<Uuid>,
        resource_iid -> Nullable<Uuid>,
        xattrs -> Nullable<Text>,
        cdn_exp -> Nullable<Timestamptz>,
        cdn_url -> Nullable<Text>,
    }
}
