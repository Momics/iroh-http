import Foundation

/// Stub iOS native plugin for iroh-http mDNS discovery.
/// Full implementation uses Network.framework NWBrowser for local peer discovery.
@objc public class IrohDiscoveryPlugin: NSObject {
    @objc public static func startDiscovery(serviceName: String, advertise: Bool) {
        // TODO: implement using NWBrowser / NWListener
    }

    @objc public static func stopDiscovery() {
        // TODO: stop NWBrowser / NWListener
    }
}
