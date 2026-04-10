package com.iroh.http.discovery

/**
 * Stub Android native plugin for iroh-http mDNS discovery.
 * Full implementation uses Android's NsdManager for local peer discovery.
 */
class IrohDiscoveryPlugin {
    fun startDiscovery(serviceName: String, advertise: Boolean) {
        // TODO: implement using NsdManager
    }

    fun stopDiscovery() {
        // TODO: stop NsdManager
    }
}
