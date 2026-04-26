#![deny(unsafe_code)]

mod commands;
mod state;

#[cfg(mobile)]
pub mod mobile_mdns;

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
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
            commands::try_next_chunk,
            commands::send_chunk,
            commands::finish_body,
            commands::cancel_request,
            commands::create_body_writer,
            commands::create_fetch_token,
            commands::cancel_in_flight,
            commands::fetch,
            commands::serve,
            commands::stop_serve,
            commands::wait_serve_stop,
            commands::wait_endpoint_closed,
            commands::respond_to_request,
            commands::connect,
            commands::secret_key_sign,
            commands::public_key_verify,
            commands::generate_secret_key,
            commands::mdns_browse,
            commands::mdns_next_event,
            commands::mdns_browse_close,
            commands::mdns_advertise,
            commands::mdns_advertise_close,
            commands::session_connect,
            commands::session_accept,
            commands::session_create_bidi_stream,
            commands::session_next_bidi_stream,
            commands::session_close,
            commands::session_closed,
            commands::session_create_uni_stream,
            commands::session_next_uni_stream,
            commands::session_send_datagram,
            commands::session_recv_datagram,
            commands::session_max_datagram_size,
            commands::start_transport_events,
        ])
        .setup(|_app, _api| {
            #[cfg(mobile)]
            {
                // ISS-009: return recoverable error instead of panicking on init failure.
                let mdns = mobile_mdns::init(_app, _api).map_err(|e| e.into())?;
                _app.manage(mdns);
            }
            Ok(())
        })
        // ISS-079: close all registered endpoints when the *last* webview is
        // destroyed to prevent QUIC socket leaks on window close without an
        // explicit JS `closeEndpoint` call.
        //
        // We count the remaining windows *after* the destroyed event fires.
        // When that count reaches zero, no webview is left running, so it is
        // safe to tear down all endpoints.  In multi-window apps this means
        // closing window A does not affect window B's networking.
        .on_event(|app, event| {
            if let tauri::RunEvent::WindowEvent {
                event: tauri::WindowEvent::Destroyed,
                ..
            } = event
            {
                if app.webview_windows().is_empty() {
                    iroh_http_core::registry::close_all_endpoints();
                }
            }
        })
        .build()
}
