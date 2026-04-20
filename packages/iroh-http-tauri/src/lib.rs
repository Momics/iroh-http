#![deny(unsafe_code)]

mod commands;
mod state;

#[cfg(mobile)]
pub mod mobile_mdns;

use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("iroh-http")
        .invoke_handler(tauri::generate_handler![
            commands::create_endpoint,
            commands::close_endpoint,
            commands::ping,
            commands::node_addr,
            commands::node_ticket,
            commands::home_relay,
            commands::peer_info,
            commands::peer_stats,
            commands::endpoint_stats,
            commands::next_chunk,
            commands::send_chunk,
            commands::finish_body,
            commands::cancel_request,
            commands::next_trailer,
            commands::send_trailers,
            commands::alloc_body_writer,
            commands::alloc_trailer_sender,
            commands::alloc_fetch_token,
            commands::cancel_in_flight,
            commands::raw_fetch,
            commands::serve,
            commands::stop_serve,
            commands::wait_serve_stop,
            commands::wait_endpoint_closed,
            commands::respond_to_request,
            commands::raw_connect,
            commands::secret_key_sign,
            commands::public_key_verify,
            commands::generate_secret_key,
            commands::mdns_browse,
            commands::mdns_next_event,
            commands::mdns_browse_close,
            commands::mdns_advertise,
            commands::mdns_advertise_close,
            commands::session_connect,
            commands::session_create_bidi_stream,
            commands::session_next_bidi_stream,
            commands::session_close,
            commands::session_closed,
            commands::session_create_uni_stream,
            commands::session_next_uni_stream,
            commands::session_send_datagram,
            commands::session_recv_datagram,
            commands::session_max_datagram_size,
        ])
        .setup(|_app, _api| {
            #[cfg(mobile)]
            {
                // ISS-009: return recoverable error instead of panicking on init failure.
                let mdns = mobile_mdns::init(_app, _api)
                    .map_err(|e| e.into())?;
                _app.manage(mdns);
            }
            Ok(())
        })
        .build()
}
