ALTER TABLE IF EXISTS riverbase_task.job_relation RENAME COLUMN _tenant TO _realm;
ALTER TABLE IF EXISTS riverbase_task.worker_job RENAME COLUMN _tenant TO _realm;
ALTER TABLE IF EXISTS riverbase_task.worker RENAME COLUMN _tenant TO _realm;
