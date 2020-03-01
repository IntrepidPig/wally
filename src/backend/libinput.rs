use calloop::{
	channel::{self, Channel, Sender},
	generic::{EventedRawFd, Generic},
	LoopHandle, Source,
};
use thiserror::{Error};

use crate::backend::{PointerButton, PointerMotion};
use crate::{
	backend::{BackendEvent, InputBackend, KeyPress, RenderBackend},
	compositor::Compositor,
};

pub struct LibinputInputBackend {
	#[allow(unused)]
	udev: udev::Context,
	libinput: input::Libinput,
	#[allow(unused)]
	event_source: Source<Generic<EventedRawFd>>,
	event_sender: Sender<BackendEvent>,
	event_receiver: Option<Channel<BackendEvent>>,
}

impl LibinputInputBackend {
	pub fn new<I: InputBackend + 'static, R: RenderBackend + 'static>(
		event_loop_handle: LoopHandle<Compositor<I, R>>,
	) -> Result<Self, ()> {
		struct RootLibinputInterface;

		impl input::LibinputInterface for RootLibinputInterface {
			fn open_restricted(&mut self, path: &std::path::Path, flags: i32) -> Result<std::os::unix::io::RawFd, i32> {
				log::debug!("Opening device at {}", path.display());
				use std::os::unix::ffi::OsStrExt;
				unsafe {
					let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
					let fd = libc::open(path.as_ptr(), flags);
					if fd < 0 {
						panic!("Failed to open libinput device path");
					}
					Ok(fd)
				}
			}

			fn close_restricted(&mut self, fd: std::os::unix::io::RawFd) {
				unsafe {
					libc::close(fd);
				}
			}
		}

		let udev = udev::Context::new().expect("Failed to create udev context");
		let mut libinput = input::Libinput::new_from_udev(RootLibinputInterface, &udev);
		libinput
			.udev_assign_seat("seat0")
			.expect("Failed to assign seat to libinput");

		let libinput_raw_fd = std::os::unix::io::AsRawFd::as_raw_fd(&libinput);
		let libinput_evented = calloop::generic::Generic::from_raw_fd(libinput_raw_fd);
		let (event_sender, event_receiver) = channel::channel();
		let event_source = event_loop_handle
			.insert_source(libinput_evented, move |_event, compositor| {
				let mut backend = compositor.backend.lock().unwrap();
				backend
					.input_backend
					.update()
					.map_err(|e| log::error!("Failed to update input backend: {}", e))
					.unwrap();
			})
			.expect("Failed to insert libinput event source into event loop");

		Ok(Self {
			udev,
			libinput,
			event_source,
			event_sender,
			event_receiver: Some(event_receiver),
		})
	}
}

#[derive(Debug, Error)]
pub enum LibinputBackendError {
	#[error("An unknown error ocurred in the libinput backend")]
	Unknown,
}

impl InputBackend for LibinputInputBackend {
	type Error = LibinputBackendError;

	fn update(&mut self) -> Result<(), Self::Error> {
		let _ = self.libinput.dispatch().map_err(|e| {
			log::error!("Failed to dispatch libinput events: {}", e);
		});
		while let Some(event) = self.libinput.next() {
			println!("Got libinput event {:?}", event);
			if let Some(backend_event) = libinput_event_to_backend_event(event) {
				let _ = self.event_sender.send(backend_event).map_err(|e| log::error!("Failed to send event to backend: {}", e));
			}
		}

		Ok(())
	}

	fn get_event_source(&mut self) -> Channel<BackendEvent> {
		self.event_receiver
			.take()
			.expect("Already took event receiver from libinput backend")
	}
}

fn libinput_event_to_backend_event(event: input::Event) -> Option<BackendEvent> {
	use input::event::{keyboard::KeyboardEventTrait, pointer::PointerEventTrait};
	Some(match event {
		input::Event::Keyboard(keyboard_event) => match keyboard_event {
			input::event::KeyboardEvent::Key(keyboard_key_event) => BackendEvent::KeyPress(KeyPress {
				serial: crate::compositor::get_input_serial(),
				time: keyboard_key_event.time(),
				key: keyboard_key_event.key(),
				state: match keyboard_key_event.key_state() {
					input::event::keyboard::KeyState::Pressed => {
						wayland_server::protocol::wl_keyboard::KeyState::Pressed
					}
					input::event::keyboard::KeyState::Released => {
						wayland_server::protocol::wl_keyboard::KeyState::Released
					}
				},
			}),
		},
		input::Event::Pointer(pointer_event) => match pointer_event {
			input::event::PointerEvent::Motion(motion) => BackendEvent::PointerMotion(PointerMotion {
				serial: crate::compositor::get_input_serial(),
				time: motion.time(),
				dx: motion.dx(),
				dx_unaccelerated: motion.dx_unaccelerated(),
				dy: motion.dy(),
				dy_unaccelerated: motion.dy_unaccelerated(),
			}),
			input::event::PointerEvent::Button(button) => BackendEvent::PointerButton(PointerButton {
				serial: crate::compositor::get_input_serial(),
				time: button.time(),
				button: button.button(),
				state: match button.button_state() {
					input::event::pointer::ButtonState::Pressed => {
						wayland_server::protocol::wl_pointer::ButtonState::Pressed
					}
					input::event::pointer::ButtonState::Released => {
						wayland_server::protocol::wl_pointer::ButtonState::Released
					}
				},
			}),
			_ => {
				log::warn!("Got unsupported mouse event");
				return None;
			}
		},
		u => {
			log::debug!("Got unknown libinput event {:?}", u);
			return None;
		}
	})
}
