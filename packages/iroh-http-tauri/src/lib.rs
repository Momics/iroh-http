mod commands;
mod state;

use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("iroh-http")
        .invoke_handler(tauri::generate_handler![
            commands::create_endpoint,
            commands::close_endpoint,
            commands::next_chunk,
            commands::send_chunk,
            commands::finish_body,
            commands::cancel_request,
            commands::next_trailer,
            commands::send_trailers,
            commands::alloc_body_writer,
            commands::raw_fetch,
            commands::serve,
            commands::respond_to_request,
            commands::raw_connect,
        ])
        .build()
}
