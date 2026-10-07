use riverbase_core::diesel_table_with_domain_fields;

diesel_table_with_domain_fields! {
    riverbase_task.worker (_id) {
        pid -> Nullable<Integer>,
        status -> Nullable<Text>,
        hostname -> Nullable<Text>,
        queue_name -> Nullable<Text>,
        jobs_complete -> Nullable<Integer>,
        jobs_failed -> Nullable<Integer>,
        jobs_retried -> Nullable<Integer>,
        jobs_queued -> Nullable<Integer>,
        start_time -> Nullable<Timestamptz>,
        heart_beat -> Nullable<Timestamptz>,
        stop_time -> Nullable<Timestamptz>,
    }
}

diesel_table_with_domain_fields! {
    riverbase_task.worker_job (_id) {
        worker_id -> Nullable<Uuid>,
        job_message -> Nullable<Text>,
        job_progress -> Nullable<Double>,
        job_status -> Nullable<Text>,
        job_try -> Nullable<Integer>,
        score -> Nullable<BigInt>,
        queue_name -> Nullable<Text>,
        function -> Nullable<Text>,
        args -> Nullable<Jsonb>,
        kwargs -> Nullable<Jsonb>,
        result -> Nullable<Jsonb>,
        err_message -> Nullable<Text>,
        err_trace -> Nullable<Text>,
        enqueue_time -> Nullable<Timestamptz>,
        start_time -> Nullable<Timestamptz>,
        finish_time -> Nullable<Timestamptz>,
        defer_time -> Nullable<Timestamptz>,
    }
}

diesel_table_with_domain_fields! {
    riverbase_task.job_relation (_id) {
        job_id -> Nullable<Uuid>,
        resource -> Nullable<Text>,
        resource_id -> Nullable<Uuid>,
        domain -> Nullable<Text>,
    }
}

diesel::allow_tables_to_appear_in_same_query!(worker, worker_job, job_relation);
