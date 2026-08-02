# Relay conventions

- Validate every untrusted payload at the route boundary.
- Every payment and webhook mutation requires an idempotency key.
- Never log tokens, full payment payloads, or customer contact data.
- Add a focused test for each error path changed.
