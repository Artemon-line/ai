// SPDX-License-Identifier: MIT
// Copyright (c) 2026 Praxis Contributors

//! Reloadable shared sub-request client handle.
//!
//! Client-aware filter factories must build filters against the *current*
//! [`SubRequestClient`], not the one captured at startup. [`ReloadableSubRequestClient`]
//! wraps the client in an [`ArcSwap`] so a hot config reload can atomically
//! swap in a client carrying the new `body_limits.max_response_bytes` ceiling.

use std::sync::Arc;

use arc_swap::ArcSwap;
use praxis_core::subrequest::SubRequestClient;

/// A shared, atomically swappable handle to the server's [`SubRequestClient`].
///
/// Client-aware filter factories capture a clone of this handle and read the
/// current client via [`current`](Self::current) **each time they build a
/// filter**. On a hot config reload the server calls [`store`](Self::store) with a
/// client carrying the new `body_limits.max_response_bytes` ceiling, so any
/// filter rebuilt during that reload observes the new ceiling instead of the
/// startup client.
///
/// This mirrors the `Arc<ArcSwap<_>>` reload idiom used elsewhere in this crate
/// (see [`routing`](crate::routing)): readers are lock-free and in-flight work
/// keeps the snapshot it loaded even as a writer swaps in a replacement.
#[derive(Clone)]
pub struct ReloadableSubRequestClient(Arc<ArcSwap<SubRequestClient>>);

impl ReloadableSubRequestClient {
    /// Wrap `client` in a fresh swappable handle.
    #[must_use]
    pub fn new(client: SubRequestClient) -> Self {
        Self(Arc::new(ArcSwap::from_pointee(client)))
    }

    /// Load the current client as a shared [`Arc`].
    ///
    /// Preferred when a reference or pointer identity is enough (e.g. reload
    /// rollback).
    #[must_use]
    pub fn load(&self) -> Arc<SubRequestClient> {
        self.0.load_full()
    }

    /// Return an owned snapshot of the current client for filter construction.
    #[must_use]
    pub fn current(&self) -> SubRequestClient {
        SubRequestClient::clone(&self.0.load())
    }

    /// Atomically replace the current client.
    ///
    /// Called on a successful config reload, and again to restore the previous
    /// client if a reload fails. In-flight filters that already loaded the old
    /// client keep using it; the next filter built sees `client`.
    pub fn store(&self, client: Arc<SubRequestClient>) {
        self.0.store(client);
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
#[expect(clippy::allow_attributes, reason = "blanket test suppressions")]
#[allow(clippy::unwrap_used, reason = "tests")]
mod tests {
    use praxis_core::subrequest::SubRequestConnector;

    use super::*;

    #[test]
    fn store_swaps_in_a_new_client() {
        let handle = ReloadableSubRequestClient::new(client());
        let before = handle.load();

        handle.store(Arc::new(client()));

        let after = handle.load();
        assert!(
            !Arc::ptr_eq(&before, &after),
            "store should replace the current client with the new one"
        );
    }

    #[test]
    fn clones_share_one_swappable_cell() {
        let handle = ReloadableSubRequestClient::new(client());
        let clone = handle.clone();

        let new_client = Arc::new(client());
        clone.store(Arc::clone(&new_client));

        assert!(
            Arc::ptr_eq(&handle.load(), &new_client),
            "a store through one clone must be visible through another (shared cell)"
        );
    }

    #[test]
    fn load_reflects_the_latest_store() {
        let handle = ReloadableSubRequestClient::new(client());
        let stored = Arc::new(client());

        handle.store(Arc::clone(&stored));

        assert!(
            Arc::ptr_eq(&handle.load(), &stored),
            "load should return the most recently stored client"
        );
        let _owned = handle.current();
    }

    // -------------------------------------------------------------------------
    // Test Utilities
    // -------------------------------------------------------------------------

    fn client() -> SubRequestClient {
        SubRequestClient::new(SubRequestConnector::new(8, None))
    }
}
