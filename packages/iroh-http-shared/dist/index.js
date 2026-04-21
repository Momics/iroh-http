"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.normaliseRelayMode = exports.encodeBase64 = exports.decodeBase64 = exports.IrohStreamError = exports.IrohProtocolError = exports.IrohHandleError = exports.IrohError = exports.IrohConnectError = exports.IrohBindError = exports.IrohArgumentError = exports.IrohAbortError = exports.classifyError = exports.classifyBindError = exports.SecretKey = exports.resolveNodeId = exports.PublicKey = exports.makeServe = exports.makeFetch = exports.makeConnect = exports.pipeToWriter = exports.makeReadable = exports.bodyInitToStream = exports.buildSession = exports.IrohNode = exports.IrohAdapter = void 0;
exports.ticketNodeId = ticketNodeId;
var IrohAdapter_js_1 = require("./IrohAdapter.js");
Object.defineProperty(exports, "IrohAdapter", { enumerable: true, get: function () { return IrohAdapter_js_1.IrohAdapter; } });
var IrohNode_js_1 = require("./IrohNode.js");
Object.defineProperty(exports, "IrohNode", { enumerable: true, get: function () { return IrohNode_js_1.IrohNode; } });
var session_js_1 = require("./session.js");
Object.defineProperty(exports, "buildSession", { enumerable: true, get: function () { return session_js_1.buildSession; } });
var streams_js_1 = require("./streams.js");
Object.defineProperty(exports, "bodyInitToStream", { enumerable: true, get: function () { return streams_js_1.bodyInitToStream; } });
Object.defineProperty(exports, "makeReadable", { enumerable: true, get: function () { return streams_js_1.makeReadable; } });
Object.defineProperty(exports, "pipeToWriter", { enumerable: true, get: function () { return streams_js_1.pipeToWriter; } });
var fetch_js_1 = require("./fetch.js");
Object.defineProperty(exports, "makeConnect", { enumerable: true, get: function () { return fetch_js_1.makeConnect; } });
Object.defineProperty(exports, "makeFetch", { enumerable: true, get: function () { return fetch_js_1.makeFetch; } });
var serve_js_1 = require("./serve.js");
Object.defineProperty(exports, "makeServe", { enumerable: true, get: function () { return serve_js_1.makeServe; } });
var keys_js_1 = require("./keys.js");
Object.defineProperty(exports, "PublicKey", { enumerable: true, get: function () { return keys_js_1.PublicKey; } });
Object.defineProperty(exports, "resolveNodeId", { enumerable: true, get: function () { return keys_js_1.resolveNodeId; } });
Object.defineProperty(exports, "SecretKey", { enumerable: true, get: function () { return keys_js_1.SecretKey; } });
var errors_js_1 = require("./errors.js");
Object.defineProperty(exports, "classifyBindError", { enumerable: true, get: function () { return errors_js_1.classifyBindError; } });
Object.defineProperty(exports, "classifyError", { enumerable: true, get: function () { return errors_js_1.classifyError; } });
Object.defineProperty(exports, "IrohAbortError", { enumerable: true, get: function () { return errors_js_1.IrohAbortError; } });
Object.defineProperty(exports, "IrohArgumentError", { enumerable: true, get: function () { return errors_js_1.IrohArgumentError; } });
Object.defineProperty(exports, "IrohBindError", { enumerable: true, get: function () { return errors_js_1.IrohBindError; } });
Object.defineProperty(exports, "IrohConnectError", { enumerable: true, get: function () { return errors_js_1.IrohConnectError; } });
Object.defineProperty(exports, "IrohError", { enumerable: true, get: function () { return errors_js_1.IrohError; } });
Object.defineProperty(exports, "IrohHandleError", { enumerable: true, get: function () { return errors_js_1.IrohHandleError; } });
Object.defineProperty(exports, "IrohProtocolError", { enumerable: true, get: function () { return errors_js_1.IrohProtocolError; } });
Object.defineProperty(exports, "IrohStreamError", { enumerable: true, get: function () { return errors_js_1.IrohStreamError; } });
var utils_js_1 = require("./utils.js");
Object.defineProperty(exports, "decodeBase64", { enumerable: true, get: function () { return utils_js_1.decodeBase64; } });
Object.defineProperty(exports, "encodeBase64", { enumerable: true, get: function () { return utils_js_1.encodeBase64; } });
Object.defineProperty(exports, "normaliseRelayMode", { enumerable: true, get: function () { return utils_js_1.normaliseRelayMode; } });
function ticketNodeId(ticket) {
    try {
        const info = JSON.parse(ticket);
        if (info && typeof info.id === 'string')
            return info.id;
    }
    catch { }
    return ticket;
}
//# sourceMappingURL=index.js.map