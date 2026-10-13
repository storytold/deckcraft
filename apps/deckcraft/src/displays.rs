//! How many displays are connected (Presenter View needs a second one), asked of the display
//! server the window uses: Wayland's outputs or X11's RandR monitors. `None` when it can't tell.

/// The connected displays, through X11 when `x11` (the window's backend) else Wayland.
pub fn count(x11: bool) -> Option<usize> {
    let n = if x11 { x11_monitors() } else { wayland_outputs() };
    n.filter(|n| *n > 0)
}

fn wayland_outputs() -> Option<usize> {
    use wayland_client::protocol::wl_registry;
    use wayland_client::{Connection, Dispatch, QueueHandle};

    struct Outputs(usize);
    impl Dispatch<wl_registry::WlRegistry, ()> for Outputs {
        fn event(state: &mut Self, _: &wl_registry::WlRegistry, event: wl_registry::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
            if let wl_registry::Event::Global { interface, .. } = event
                && interface == "wl_output"
            {
                state.0 += 1;
            }
        }
    }

    let conn = Connection::connect_to_env().ok()?;
    let mut queue = conn.new_event_queue();
    let _registry = conn.display().get_registry(&queue.handle(), ());
    let mut outputs = Outputs(0);
    queue.roundtrip(&mut outputs).ok()?;
    Some(outputs.0)
}

fn x11_monitors() -> Option<usize> {
    use x11rb::connection::Connection;
    use x11rb::protocol::randr::ConnectionExt;

    let (conn, screen) = x11rb::connect(None).ok()?;
    let root = conn.setup().roots.get(screen)?.root;
    let reply = conn.randr_get_monitors(root, true).ok()?.reply().ok()?;
    Some(reply.monitors.len())
}

#[cfg(test)]
mod tests {
    #[test]
    fn counting_never_fails_without_a_display_server() {
        // CI has no display: the count is unknown, never a crash.
        let _ = super::count(true);
        let _ = super::count(false);
    }
}
