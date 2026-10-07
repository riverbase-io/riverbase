# riverbase_proc

Proc-macros for Riverbase command/query registration (`#[domain_action]`,
`riverbase_namespace!`).

`#[domain_action(event = "…")]` is required on every state-changing aggregate method. On `Ok` it
enqueues one domain event whose key is the action (persisted by the command session). Optional
`resources = ["…"]` repeats the `CMD-003` resource allow-list.
