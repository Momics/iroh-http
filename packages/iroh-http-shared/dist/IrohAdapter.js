"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.IrohAdapter = void 0;
class IrohAdapter {
    // ── Optional: raw connect ────────────────────────────────────────────────────
    rawConnect(_endpointHandle, _nodeId, _path, _headers) {
        return Promise.reject(new Error(`rawConnect() not supported by this adapter`));
    }
    // ── Optional: sessions ──────────────────────────────────────────────────────
    get sessionFns() { return null; }
    // ── Optional: mDNS discovery ────────────────────────────────────────────────
    mdnsBrowse(_endpointHandle, _serviceName) {
        return Promise.reject(new Error(`mdnsBrowse() not supported by this adapter`));
    }
    mdnsNextEvent(_browseHandle) {
        return Promise.reject(new Error(`mdnsNextEvent() not supported by this adapter`));
    }
    mdnsBrowseClose(_browseHandle) { }
    mdnsAdvertise(_endpointHandle, _serviceName) {
        return Promise.reject(new Error(`mdnsAdvertise() not supported by this adapter`));
    }
    mdnsAdvertiseClose(_advertiseHandle) { }
    // ── Optional: transport events ──────────────────────────────────────────────
    pollTransportEvent(_endpointHandle) {
        return Promise.resolve(null);
    }
}
exports.IrohAdapter = IrohAdapter;
//# sourceMappingURL=IrohAdapter.js.map