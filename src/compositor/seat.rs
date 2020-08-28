use std::{
	cell::{Cell},
};

use crate::{
	backend::{GraphicsBackend, InputBackend},
	compositor::{Compositor, prelude::*},
};

pub struct SeatData {
	pub inner: RefCell<SeatDataInner>,
	current_serial: Cell<Serial>,
	pub current_pointer_serial: Cell<Serial>,
	pub current_keyboard_serial: Cell<Serial>,
}

impl SeatData {
	pub fn new() -> Self {
		Self {
			inner: RefCell::new(SeatDataInner {
				pointer: None,
				keyboard: None,
				touch: None,
			}),
			current_serial: Cell::new(Serial(0)),
			current_pointer_serial: Cell::new(Serial(0)),
			current_keyboard_serial: Cell::new(Serial(0)),
		}
	}
}

impl SeatData {
	fn next_serial(&self) -> Serial {
		let mut serial = self.current_serial.get();
		serial.advance();
		self.current_serial.set(serial);
		serial
	}

	pub fn next_pointer_serial(&self) -> Serial {
		let serial = self.next_serial();
		self.current_pointer_serial.set(serial);
		serial
	}

	pub fn next_keyboard_serial(&self) -> Serial {
		let serial = self.next_serial();
		self.current_pointer_serial.set(serial);
		serial
	}
}

pub struct SeatDataInner {
	pub pointer: Option<Resource<WlPointer, PointerData>>,
	pub keyboard: Option<Resource<WlKeyboard, KeyboardData>>,
	pub touch: Option<Resource<WlTouch, TouchData>>,
}

pub struct PointerData {
	pub seat: Resource<WlSeat, SeatData>,
}

impl PointerData {
	pub fn new(seat: Resource<WlSeat, SeatData>) -> Self {
		Self {
			seat,
		}
	}
}

pub struct KeyboardData {
	pub seat: Resource<WlSeat, SeatData>,
}

impl KeyboardData {
	pub fn new(seat: Resource<WlSeat, SeatData>) -> Self {
		Self {
			seat,
		}
	}
}

pub struct TouchData {
	pub seat: Resource<WlSeat, SeatData>,
}

impl TouchData {
	pub fn new(seat: Resource<WlSeat, SeatData>) -> Self {
		Self {
			seat,
		}
	}
}

impl<I: InputBackend, G: GraphicsBackend> Compositor<I, G> {
	pub fn setup_seat_global(&mut self) {
		self.server.register_global(|new: NewResource<WlSeat>| {
			let seat = new.register_fn(
				SeatData::new(),
				|state, this, request| {
					let state = state.get_mut::<CompositorState<I, G>>();
					state.handle_seat_request(this, request);
				},
				|_state, _this| {
					log::warn!("wl_seat destructor not implemented");
				},

			);

			let capabilities_event = wl_seat::CapabilitiesEvent {
				capabilities: wl_seat::Capability::POINTER | wl_seat::Capability::KEYBOARD,
			};
			seat.send_event(WlSeatEvent::Capabilities(capabilities_event));

			let client = seat.client();
			let client = client.get().unwrap();
			let client_state = client.state::<RefCell<ClientState<G>>>();
			let mut client_state = client_state.borrow_mut();
			if let Some(old_seat) = client_state.seat.take() {
				log::warn!("Multiple seats not supported yet");
				old_seat.destroy();
			}

			client_state.seat = Some(seat);
		});
	}
}

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn handle_seat_request(&mut self, this: Resource<WlSeat, SeatData>, request: WlSeatRequest) {
		match request {
			WlSeatRequest::GetPointer(request) => self.handle_seat_get_pointer(this, request),
			WlSeatRequest::GetKeyboard(request) => self.handle_seat_get_keyboard(this, request),
			WlSeatRequest::GetTouch(_request) => log::warn!("wl_seat::get_touch not implemented"),
			WlSeatRequest::Release => log::warn!("wl_seat::release not implemented"),
		}
	}

	pub fn handle_seat_get_pointer(&mut self, this: Resource<WlSeat, SeatData>, request: wl_seat::GetPointerRequest) {
		let pointer = request.id.register_fn(
			PointerData::new(this.clone()),
			|state, this, request| {
				let state = state.get_mut::<Self>();
				state.handle_pointer_request(this, request);
			},
			|_state, _this| {
				log::warn!("wl_pointer destructor not implemented");
			},
		);

		let seat_data = this.get_data();
		let mut seat_data = seat_data.inner.borrow_mut();
		if let Some(old_pointer) = seat_data.pointer.replace(pointer) {
			log::warn!("A seat requested a second pointer. The intended behavior for this is unclear");
			old_pointer.destroy();
		}
	}

	pub fn handle_pointer_request(&mut self, this: Resource<WlPointer, PointerData>, request: WlPointerRequest) {
		match request {
			WlPointerRequest::SetCursor(_request) => log::warn!("wl_pointer::set_cursor not implemented"),
			WlPointerRequest::Release => self.handle_pointer_release(this),
		}
	}

	pub fn handle_pointer_release(&mut self, this: Resource<WlPointer, PointerData>) {
		let pointer_data = this.get_data();
		let seat_data = pointer_data.seat.get_data();
		let mut seat_data = seat_data.inner.borrow_mut();
		if seat_data.pointer.as_ref().map(|pointer| pointer.is(&this)).unwrap_or(false) {
			seat_data.pointer.take();
		}
		this.destroy();
	}

	pub fn handle_seat_get_keyboard(&mut self, this: Resource<WlSeat, SeatData>, request: wl_seat::GetKeyboardRequest) {
		let keyboard = request.id.register_fn(
			KeyboardData::new(this.clone()),
			|state, this, request| {
				let state = state.get_mut::<Self>();
				state.handle_keyboard_request(this, request);
			},
			|_state, _this| {
				log::warn!("wl_keyboard destructor not implemented");
			},
		);

		let keymap_event = wl_keyboard::KeymapEvent {
			format: wl_keyboard::KeymapFormat::XkbV1,
			fd: self.inner.keyboard_state.fd,
			size: self.inner.keyboard_state.keymap_string.as_bytes().len() as u32,
		};
		keyboard.send_event(WlKeyboardEvent::Keymap(keymap_event));

		let seat_data = this.get_data();
		let serial = seat_data.next_keyboard_serial();
		self.inner.keyboard_state.send_current_keyboard_modifiers(keyboard.clone(), serial);
		if let Some(ref keyboard_focus) = self.inner.keyboard_focus {
			let enter_event = wl_keyboard::EnterEvent {
				serial: serial.into(),
				surface: keyboard_focus.to_untyped(),
				keys: Vec::new(), // TODO: actual value
			};
			keyboard.send_event(WlKeyboardEvent::Enter(enter_event));
		}

		let seat_data = this.get_data();
		let mut seat_data = seat_data.inner.borrow_mut();
		if let Some(old_keyboard) = seat_data.keyboard.replace(keyboard) {
			log::warn!("A seat requested a second keyboard. The intended behavior for this is unclear");
			old_keyboard.destroy();
		}
	}

	pub fn handle_keyboard_request(&mut self, this: Resource<WlKeyboard, KeyboardData>, request: WlKeyboardRequest) {
		match request {
			WlKeyboardRequest::Release => self.handle_keyboard_release(this),
		}
	}

	pub fn handle_keyboard_release(&mut self, this: Resource<WlKeyboard, KeyboardData>) {
		let keyboard_data = this.get_data();
		let seat_data = keyboard_data.seat.get_data();
		let mut seat_data = seat_data.inner.borrow_mut();
		if seat_data.keyboard.as_ref().map(|keyboard| keyboard.is(&this)).unwrap_or(false) {
			seat_data.keyboard.take();
		}
		this.destroy();
	}
}

impl<I: InputBackend, G: GraphicsBackend> CompositorState<I, G> {
	pub fn send_keyboard_key_event(&mut self, seat: Resource<WlSeat, SeatData>, event: KeyPress) {
		let seat_data = seat.get_data();
		let seat_data_inner = seat_data.inner.borrow();
		if let Some(ref keyboard) = seat_data_inner.keyboard {
			let serial = seat_data.next_keyboard_serial();
			if self.inner.keyboard_state.mods_state_change {
				self.inner.keyboard_state.send_current_keyboard_modifiers(keyboard.clone(), serial);
				self.inner.keyboard_state.mods_state_change = false;
			}
			keyboard.send_event(WlKeyboardEvent::Key(wl_keyboard::KeyEvent {
				serial: serial.into(),
				time: self.time().as_u32(),
				key: event.key,
				state: event.state.into(),
			}));
		}
	}

	pub fn send_pointer_motion_event(&self, seat: Resource<WlSeat, SeatData>, surface_relative_coords: Point) {
		if let Some(ref pointer) = seat.get_data().inner.borrow().pointer {
			pointer.send_event(WlPointerEvent::Motion(wl_pointer::MotionEvent {
				time: self.time().as_u32(),
				surface_x: (surface_relative_coords.x as f64).into(),
				surface_y: (surface_relative_coords.y as f64).into(),
			}));
		}
	}

	pub fn send_pointer_button_event(&self, seat: Resource<WlSeat, SeatData>, event: PointerButton) {
		let seat_data = seat.get_data();
		let seat_data_inner = seat_data.inner.borrow();
		if let Some(ref pointer) = seat_data_inner.pointer {
			pointer.send_event(WlPointerEvent::Button(wl_pointer::ButtonEvent {
				serial: seat_data.next_pointer_serial().into(),
				time: self.time().as_u32(),
				button: event.button.to_wl(),
				state: event.state.into(),
			}));
		}
	}
}
