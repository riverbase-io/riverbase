ALTER TABLE IF EXISTS riverbase_task.worker RENAME COLUMN _realm TO _tenant;
ALTER TABLE IF EXISTS riverbase_task.worker_job RENAME COLUMN _realm TO _tenant;
ALTER TABLE IF EXISTS riverbase_task.job_relation RENAME COLUMN _realm TO _tenant;
