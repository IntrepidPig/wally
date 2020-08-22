use std::ffi::CString;

use crate::{
	backend::{GraphicsBackend, InputBackend},
	compositor::{Compositor, prelude::*},
};

impl<I: InputBackend + 'static, G: GraphicsBackend + 'static> Compositor<I, G> {
	pub(crate) fn setup_output_globals(&mut self) {
		let outputs = self.state().graphics_state.renderer.outputs();
		for output in outputs {
			let output_global = self.server.register_global::<WlOutput, _>(move |new: NewResource<WlOutput>| {
				let output_resource = new.register_fn(output, |state, this, request| {
					let state = state.get_mut::<CompositorState<I, G>>();
					state.handle_output_request(this, request);
				});

				let client = output_resource.client();
				let client = client.get().unwrap();
				let client_state = client.state::<RefCell<ClientState>>();
				client_state.borrow_mut().outputs.push(output_resource.clone());

				let geometry_event = wl_output::GeometryEvent {
					x: output.viewport.x,
					y: output.viewport.y,
					physical_width: 0,
					physical_height: 0,
					subpixel: wl_output::Subpixel::HorizontalBgr,
					make: CString::new(String::from("<unknown>")).unwrap().into_bytes_with_nul(),
					model: CString::new(String::from("<unknown>")).unwrap().into_bytes_with_nul(),
					transform: wl_output::Transform::Normal,
				};
				let mode_event = wl_output::ModeEvent {
					flags: wl_output::Mode::CURRENT | wl_output::Mode::PREFERRED,
					width: output.viewport.width as i32,
					height: output.viewport.height as i32,
					refresh: 75,
				};
				let scale_event = wl_output::ScaleEvent {
					factor: 1,
				};
				output_resource.send_event(WlOutputEvent::Geometry(geometry_event));
				output_resource.send_event(WlOutputEvent::Mode(mode_event));
				if true { // TODO: check version >= 2
					output_resource.send_event(WlOutputEvent::Scale(scale_event));
				}
				output_resource.send_event(WlOutputEvent::Done);
			});
			
			self.state_mut().inner.output_globals.push((output_global, output));
		}
	}
}

impl<I: InputBackend + 'static, G: GraphicsBackend + 'static> CompositorState<I, G> {
	pub fn handle_output_request(&mut self, this: Resource<WlOutput>, request: WlOutputRequest) {
		match request {
			WlOutputRequest::Release => self.handle_output_release(this),
		}
	}

	pub fn handle_output_release(&mut self, _this: Resource<WlOutput>) {
		log::warn!("Output release handling unimplemented");
	}
}
