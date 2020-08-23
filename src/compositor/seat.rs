use crate::{
	backend::{GraphicsBackend, InputBackend},
	compositor::{Compositor, prelude::*},
};

impl<I: InputBackend + 'static, G: GraphicsBackend + 'static> Compositor<I, G> {
	pub fn setup_seat_global(&mut self) {
		self.server.register_global(|new: NewResource<WlSeat>| {
			let seat = new.register_fn((), |state, this, request| {
				let state = state.get_mut::<CompositorState<I, G>>();
				state.handle_seat_request(this, request);
			});

			let capabilities_event = wl_seat::CapabilitiesEvent {
				capabilities: wl_seat::Capability::POINTER | wl_seat::Capability::KEYBOARD,
			};
			seat.send_event(WlSeatEvent::Capabilities(capabilities_event));
		});
	}
}

impl<I: InputBackend + 'static, G: GraphicsBackend + 'static> CompositorState<I, G> {
	pub fn handle_seat_request(&mut self, this: Resource<WlSeat>, request: WlSeatRequest) {
		match request {
			WlSeatRequest::GetPointer(request) => self.handle_seat_get_pointer(this, request),
			WlSeatRequest::GetKeyboard(request) => self.handle_seat_get_keyboard(this, request),
			WlSeatRequest::GetTouch(_request) => log::warn!("wl_seat::get_touch not implemented"),
			WlSeatRequest::Release => log::warn!("wl_seat::release not implemented"),
		}
	}

	pub fn handle_seat_get_pointer(&mut self, this: Resource<WlSeat>, request: wl_seat::GetPointerRequest) {
		let pointer = request.id.register_fn((), |state, this, request| {
			let state = state.get_mut::<Self>();
			state.handle_pointer_request(this, request);
		});

		let client = this.client();
		let client = client.get().unwrap();
		let client_state = client.state::<RefCell<ClientState>>();
		client_state.borrow_mut().pointers.push(pointer);
	}

	pub fn handle_pointer_request(&mut self, this: Resource<WlPointer>, request: WlPointerRequest) {
		match request {
			WlPointerRequest::SetCursor(_request) => log::warn!("wl_pointer::set_cursor not implemented"),
			WlPointerRequest::Release => self.handle_pointer_release(this),
		}
	}

	pub fn handle_pointer_release(&mut self, this: Resource<WlPointer>) {
		this.client().get().unwrap().state::<RefCell<ClientState>>().borrow_mut().pointers.retain(|pointer| !pointer.is(&this));
		this.destroy();
	}

	pub fn handle_seat_get_keyboard(&mut self, this: Resource<WlSeat>, request: wl_seat::GetKeyboardRequest) {
		let keyboard = request.id.register_fn((), |state, this, request| {
			let state = state.get_mut::<Self>();
			state.handle_keyboard_request(this, request);
		});

		let keymap_event = wl_keyboard::KeymapEvent {
			format: wl_keyboard::KeymapFormat::XkbV1,
			fd: self.inner.keyboard_state.fd,
			size: self.inner.keyboard_state.keymap_string.as_bytes().len() as u32,
		};
		keyboard.send_event(WlKeyboardEvent::Keymap(keymap_event));

		let client = this.client();
		let client = client.get().unwrap();
		let client_state = client.state::<RefCell<ClientState>>();
		client_state.borrow_mut().keyboards.push(keyboard);
	}

	pub fn handle_keyboard_request(&mut self, this: Resource<WlKeyboard>, request: WlKeyboardRequest) {
		match request {
			WlKeyboardRequest::Release => self.handle_keyboard_release(this),
		}
	}

	pub fn handle_keyboard_release(&mut self, this: Resource<WlKeyboard>) {
		this.client().get().unwrap().state::<RefCell<ClientState>>().borrow_mut().keyboards.retain(|pointer| !pointer.is(&this));
		this.destroy();
	}
}
