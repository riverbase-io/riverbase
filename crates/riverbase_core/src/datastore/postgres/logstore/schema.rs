diesel::table! {
    use diesel::sql_types::*;
    use crate::logstore::types::DomainTransportSql;

    riverbase_audit.context_log (_id) {
        _id -> Uuid,
        _created -> Timestamptz,
        _creator -> Nullable<Uuid>,
        domain -> Nullable<Text>,
        revision -> Nullable<Integer>,
        realm -> Nullable<Uuid>,
        dataset_id -> Nullable<Uuid>,
        request_id -> Nullable<Uuid>,
        user_id -> Nullable<Uuid>,
        profile_id -> Nullable<Uuid>,
        organization_id -> Nullable<Uuid>,
        iam_roles -> Nullable<Array<Text>>,
        session -> Nullable<Text>,
        timestamp -> Nullable<Timestamptz>,
        transport -> Nullable<DomainTransportSql>,
        source -> Nullable<Jsonb>,
        headers -> Nullable<Jsonb>,
        _tenant -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::logstore::types::CommandStatusSql;

    riverbase_audit.query_log (_id) {
        _id -> Uuid,
        _created -> Timestamptz,
        _creator -> Nullable<Uuid>,
        domain -> Nullable<Text>,
        resource -> Nullable<Text>,
        access -> Text,
        identifier -> Nullable<Uuid>,
        domain_sid -> Nullable<Uuid>,
        domain_iid -> Nullable<Uuid>,
        request -> Nullable<Jsonb>,
        context -> Uuid,
        status -> Nullable<CommandStatusSql>,
        result_count -> Nullable<Integer>,
        error_code -> Nullable<Text>,
        _tenant -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::logstore::types::CommandStatusSql;

    riverbase_audit.command_log (_id) {
        _id -> Uuid,
        _created -> Timestamptz,
        _creator -> Nullable<Uuid>,
        domain -> Nullable<Text>,
        identifier -> Nullable<Uuid>,
        resource -> Nullable<Text>,
        revision -> Nullable<Integer>,
        command -> Nullable<Text>,
        domain_sid -> Nullable<Uuid>,
        domain_iid -> Nullable<Uuid>,
        payload -> Nullable<Jsonb>,
        context -> Uuid,
        status -> Nullable<CommandStatusSql>,
        _tenant -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    riverbase_audit.event_log (_id) {
        _id -> Uuid,
        _created -> Timestamptz,
        _creator -> Nullable<Uuid>,
        domain -> Nullable<Text>,
        event -> Nullable<Text>,
        identifier -> Nullable<Uuid>,
        resource -> Nullable<Text>,
        src_cmd -> Nullable<Uuid>,
        args -> Nullable<Jsonb>,
        data -> Nullable<Jsonb>,
        _tenant -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    riverbase_audit.message_log (_id) {
        _id -> Uuid,
        _created -> Timestamptz,
        _creator -> Nullable<Uuid>,
        domain -> Nullable<Text>,
        src_cmd -> Nullable<Uuid>,
        message -> Nullable<Text>,
        data -> Nullable<Jsonb>,
        _tenant -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;
    use crate::logstore::types::ActivityMsgTypeSql;

    riverbase_audit.activity_log (_id) {
        _id -> Uuid,
        _created -> Timestamptz,
        _creator -> Nullable<Uuid>,
        _source -> Nullable<Text>,
        domain -> Nullable<Text>,
        identifier -> Nullable<Uuid>,
        resource -> Nullable<Text>,
        domain_sid -> Nullable<Uuid>,
        domain_iid -> Nullable<Uuid>,
        message -> Nullable<Text>,
        msgtype -> Nullable<ActivityMsgTypeSql>,
        msglabel -> Nullable<Text>,
        context -> Nullable<Uuid>,
        src_cmd -> Nullable<Uuid>,
        src_evt -> Nullable<Uuid>,
        data -> Nullable<Jsonb>,
        code -> Nullable<Integer>,
        _tenant -> Nullable<Uuid>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    riverbase_audit.command_response (cmd_id) {
        cmd_id -> Text,
        payload -> Jsonb,
        created_at -> Timestamptz,
        updated_at -> Timestamptz,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    riverbase_audit.outbox (_id) {
        _id -> Uuid,
        _created -> Timestamptz,
        src_cmd -> Uuid,
        topic -> Text,
        payload -> Jsonb,
        status -> Text,
        attempts -> Integer,
        next_attempt_at -> Timestamptz,
        published_at -> Nullable<Timestamptz>,
        last_error -> Nullable<Text>,
    }
}

diesel::table! {
    use diesel::sql_types::*;

    riverbase_audit.idempotency_key (namespace, command, key) {
        key -> Text,
        namespace -> Text,
        command -> Text,
        actor -> Nullable<Uuid>,
        request_hash -> Text,
        status -> Text,
        response -> Nullable<Jsonb>,
        cmd_id -> Nullable<Text>,
        created_at -> Timestamptz,
        completed_at -> Nullable<Timestamptz>,
        expires_at -> Nullable<Timestamptz>,
        owner_token -> Nullable<Uuid>,
        lease_expires_at -> Nullable<Timestamptz>,
        failed_at -> Nullable<Timestamptz>,
        error -> Nullable<Jsonb>,
    }
}
