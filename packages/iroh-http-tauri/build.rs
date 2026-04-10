fn main() {
    tauri_plugin::Builder::new(&[
        "create_endpoint",
        "close_endpoint",
        "next_chunk",
        "send_chunk",
        "finish_body",
        "alloc_body_writer",
        "raw_fetch",
        "serve",
        "respond_to_request",
    ])
    .build();
}
