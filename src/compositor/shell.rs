use wayland_server::{protocol::*, Filter, Main};

use crate::{
	backend::{InputBackend, RenderBackend},
	compositor::Compositor,
};

impl<I: InputBackend, R: RenderBackend> Compositor<I, R> {
	pub(crate) fn setup_wl_shell_global(&mut self) {
		let wl_shell_filter = Filter::new(|(main, _num): (Main<wl_shell::WlShell>, u32), filter, _dispatch_data| {
			main.quick_assign(|main, request: wl_shell::Request, _| match request {
				wl_shell::Request::GetShellSurface { id, surface } => {
					log::debug!("Got get_shell_surface request for wl_shell");
				}
				_ => {
					log::warn!("Got unknown request for wl_shell");
				}
			})
		});
		self.display.create_global::<wl_shell::WlShell, _>(1, wl_shell_filter);
	}
}
