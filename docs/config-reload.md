# Hot config reload

Praxis AI watches the config file passed at startup and rebuilds its filter
pipelines in place when the file changes. Edits are debounced, validated, and
applied atomically: a config that fails validation or pipeline construction is
rejected and the running server keeps its current pipelines. There is no request
downtime and no dropped connections during a successful reload.

This page lists which settings take effect on reload and which require a full
process restart. When a restart-required setting changes, the server logs a
warning naming the setting and continues running with the previous value for
that setting.

## Applied on reload

These take effect on the next request after a successful reload, without a
restart:

- **Filter chains and filter parameters** — adding, removing, reordering, or
  reconfiguring filters within a chain.
- **Routes and clusters** — route matches, cluster membership, endpoint lists,
  and load-balancing selection.
- **Health checks** — cluster health-check settings are re-derived and their
  background tasks respawned.
- **`body_limits.max_response_bytes`** — the sub-request response ceiling. On
  reload the server builds a sub-request client carrying the new ceiling and
  publishes it before rebuilding pipelines, so filters that make HTTP callouts
  (for example `openai_file_resolve`, `openai_web_search`,
  `openai_file_search_callout`, `openai_responses_compact`, and
  `anthropic_web_search`) enforce the new ceiling on their next build. Lowering
  the ceiling causes oversized callout bodies to be rejected; raising it admits
  bodies that were previously rejected.
- **`log_overrides`** — validated as part of the reload.

## Requires a restart

Changing any of these logs a warning and keeps the previous value until the
process is restarted:

- **Listener topology** — adding or removing a listener, or changing a
  listener's bind address. The socket bind happens once at startup.
- **Listener protocol** — switching a listener between HTTP and TCP.
- **TLS toggles** — enabling or disabling TLS on a listener.
- **Compression** — adding a compression filter to a chain that did not
  previously have one; module registration is one-shot at startup.
- **`runtime.subrequest_pool_size`** — the sub-request connection pool size.
  The pool is created once at startup and reused across reloads.
- **`runtime.subrequest_max_connections`** — the sub-request connection cap.
  Reused across reloads alongside the pool.

Note the split within the sub-request client: the **response ceiling**
(`body_limits.max_response_bytes`) is reloadable because a fresh client carrying
the new ceiling is built on top of the existing connection pool, while the
**pool and connection settings** (`runtime.subrequest_pool_size`,
`runtime.subrequest_max_connections`) are restart-only because the pool itself is
preserved across reloads.

## Stateful filters

Filters that hold runtime state — `rate_limit` and `circuit_breaker` — have that
state reset when their pipeline is rebuilt on reload. In-flight requests that
already captured the old pipeline retain the old state through an `Arc` guard;
new requests see fresh state. The server logs a warning when a reloaded config
contains these filters.
