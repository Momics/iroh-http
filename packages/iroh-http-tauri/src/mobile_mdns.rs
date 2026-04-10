//! Tauri mobile mDNS bridge — routes to native Swift/Kotlin plugins.
//!
//! On iOS: calls `IrohDiscoveryPlugin` via Tauri's PluginHandle.
//! On Android: calls `IrohDiscoveryPlugin` via Tauri's PluginHandle.
//!
//! This module is only compiled on mobile targets (`#[cfg(mobile)]`).

// Stub — full implementation follows the pattern in .old_references/iroh-tauri.
// The mobile native plugins are in ios/ and android/ directories.
