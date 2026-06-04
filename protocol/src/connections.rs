// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Process-wide connection limit.
//!
//! Complements per-listener `max_connections` with a global
//! ceiling across all listeners. Initialized once at server
//! startup from [`RuntimeConfig::max_connections`].
//!
//! [`RuntimeConfig::max_connections`]: praxis_core::config::RuntimeConfig::max_connections

use std::sync::{Arc, OnceLock};

use tokio::sync::{OwnedSemaphorePermit, Semaphore};

// ---------------------------------------------------------------------------
// Global Semaphore
// ---------------------------------------------------------------------------

/// Process-wide connection semaphore.
static GLOBAL_LIMIT: OnceLock<Arc<Semaphore>> = OnceLock::new();

/// Initialize the global connection limit.
///
/// Called once during server startup. Subsequent calls are no-ops.
pub fn init_global_limit(max: usize) {
    GLOBAL_LIMIT.get_or_init(|| Arc::new(Semaphore::new(max)));
}

/// Try to acquire a global connection permit.
///
/// Returns `Ok(None)` when no global limit is configured,
/// `Ok(Some(permit))` on success, or `Err(())` when the
/// limit is exceeded.
pub fn try_acquire_global() -> Result<Option<OwnedSemaphorePermit>, ()> {
    let Some(sem) = GLOBAL_LIMIT.get() else {
        return Ok(None);
    };
    Arc::clone(sem).try_acquire_owned().map(Some).map_err(|_| ())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "tests"
)]
mod tests {
    use super::*;

    #[test]
    fn no_init_returns_ok_none() {
        assert!(
            matches!(try_acquire_global(), Ok(None)),
            "uninitialized global should return Ok(None)"
        );
    }
}
