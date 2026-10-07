DROP TABLE IF EXISTS riverbase_audit.update_queue;
DROP TABLE IF EXISTS riverbase_audit.command_queue;
DROP TABLE IF EXISTS riverbase_audit.command_log;
DROP TABLE IF EXISTS riverbase_audit.message_log;
DROP TABLE IF EXISTS riverbase_audit.event_log;
DROP TABLE IF EXISTS riverbase_audit.activity_log;
DROP TABLE IF EXISTS riverbase_audit.context_log;

DROP TYPE IF EXISTS riverbase_audit.domain_transport;
DROP TYPE IF EXISTS riverbase_audit.domain_entity_type;
DROP TYPE IF EXISTS riverbase_audit.command_action;
DROP TYPE IF EXISTS riverbase_audit.command_status;
DROP TYPE IF EXISTS riverbase_audit.activity_msg_type;

DROP SCHEMA IF EXISTS riverbase_audit;
