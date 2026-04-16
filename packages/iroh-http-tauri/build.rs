fn main() {
    tauri_plugin::Builder::new(&[
        // Endpoint lifecycle
        "create_endpoint",
        "close_endpoint",
        "ping",
        // Address introspection
        "node_addr",
        "node_ticket",
        "home_relay",
        "peer_info",
        "peer_stats",
        // Streaming primitives
        "next_chunk",
        "send_chunk",
        "finish_body",
        "alloc_body_writer",
        "alloc_fetch_token",
        "cancel_in_flight",
        "cancel_request",
        "next_trailer",
        "send_trailers",
        // HTTP client
        "raw_fetch",
        // HTTP server
        "serve",
        "stop_serve",
        "wait_serve_stop",
        "wait_endpoint_closed",
        "respond_to_request",
        // Raw / duplex connect
        "raw_connect",
        // Session (QUIC)
        "session_connect",
        "session_create_bidi_stream",
        "session_next_bidi_stream",
        "session_close",
        "session_closed",
        "session_create_uni_stream",
        "session_next_uni_stream",
        "session_send_datagram",
        "session_recv_datagram",
        "session_max_datagram_size",
        // Crypto utilities
        "secret_key_sign",
        "public_key_verify",
        "generate_secret_key",
        // mDNS discovery
        "mdns_browse",
        "mdns_next_event",
        "mdns_browse_close",
        "mdns_advertise",
        "mdns_advertise_close",
    ])
    .android_path("android")
    .ios_path("ios")
    .build();
}
