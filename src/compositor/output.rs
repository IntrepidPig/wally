use std::sync::Arc;

use wayland_server::{protocol::*, Filter, Main};

use crate::{
	backend::{GraphicsBackend, InputBackend},
	compositor::Compositor,
};

impl<I: InputBackend + 'static, G: GraphicsBackend + 'static> Compositor<I, G> {
	pub(crate) fn setup_output_global(&mut self) {
		let graphics_backend_state_lock = self.graphics_backend_state.lock().unwrap();
		let outputs = graphics_backend_state_lock.renderer.outputs();
		for output in outputs {
			let inner = Arc::clone(&self.inner);
			let output_filter = Filter::new(
				move |(main, _num): (Main<wl_output::WlOutput>, u32), _filter, _dispatch_data| {
					let inner = Arc::clone(&inner);
					let mut inner_lock = inner.lock().unwrap();
					let output_interface = &*main;
					let client_info = inner_lock
						.client_manager
						.get_client_info(output_interface.as_ref().client().unwrap());
					let mut client_info_lock = client_info.lock().unwrap();
					client_info_lock.outputs.push(output_interface.clone());
					output_interface.as_ref().user_data().set_threadsafe(|| output);
					output_interface.geometry(
						output.viewport.x,
						output.viewport.y,
						0,
						0,
						wl_output::Subpixel::HorizontalBgr,
						String::from("<unknown>"),
						String::from("<unknown>"),
						wl_output::Transform::Normal,
					);
					// TODO: don't hardcode
					output_interface.mode(wl_output::Mode::Current | wl_output::Mode::Preferred, 1920, 1080, 75);
					if output_interface.as_ref().version() >= 2 {
						output_interface.scale(1);
					}
					output_interface.done();
					main.quick_assign(move |_main, request, _dispatch_data| match request {
						wl_output::Request::Release => {}
						_ => log::warn!("Got unknown request for wl_output"),
					})
				},
			);
			let output_global = self.display.create_global(2, output_filter);
			let mut inner_lock = self.inner.lock().unwrap();
			inner_lock.output_globals.push((output_global, output));
		}
	}
}
