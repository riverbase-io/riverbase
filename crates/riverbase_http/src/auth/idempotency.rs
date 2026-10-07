/// Client idempotency key carried from the `Idempotency-Key` request header.
#[derive(Debug, Clone)]
pub struct IdempotencyKey(pub String);
