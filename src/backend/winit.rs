use std::time::Instant;

use calloop::channel::{self, Channel, Sender};
use wayland_server::protocol::*;
use winit::{
	event::{ElementState, Event as WinitEvent, WindowEvent},
	event_loop::{ControlFlow, EventLoop},
};

use crate::backend::{BackendEvent, InputBackend, KeyPress};

pub struct WinitInputBackend {
	event_sender: Sender<BackendEvent>,
	event_receiver: Option<Channel<BackendEvent>>,
}

impl WinitInputBackend {
	pub fn new() -> Self {
		let (event_sender, event_receiver) = channel::channel();
		Self {
			event_sender,
			event_receiver: Some(event_receiver),
		}
	}

	pub fn start(sender: Sender<BackendEvent>, event_loop: EventLoop<()>) {
		let start = Instant::now();
		event_loop.run(
			move |event: WinitEvent<()>, _event_loop_window_target, control_flow: &mut ControlFlow| {
				*control_flow = ControlFlow::Wait;
				let backend_event = match event {
					WinitEvent::WindowEvent {
						window_id,
						event: WindowEvent::CloseRequested,
					} => {
						*control_flow = ControlFlow::Exit;
						Some(BackendEvent::StopRequested)
					}
					WinitEvent::WindowEvent {
						window_id,
						event: WindowEvent::KeyboardInput {
							device_id,
							input,
							is_synthetic,
						},
					} => Some(BackendEvent::KeyPress(KeyPress {
						serial: crate::compositor::get_input_serial(),
						time: start.elapsed().as_millis() as u32,
						key: input.scancode,
						state: match input.state {
							ElementState::Pressed => wl_keyboard::KeyState::Pressed,
							ElementState::Released => wl_keyboard::KeyState::Released,
						},
					})),
					_ => None,
				};
				if let Some(backend_event) = backend_event {
					let _ = sender.send(backend_event).map_err(|e| {
						log::error!("Failed to send event to backend: {}", e);
					});
				}
			},
		)
	}

	pub fn get_sender(&self) -> Sender<BackendEvent> {
		self.event_sender.clone()
	}
}

impl InputBackend for WinitInputBackend {
	type Error = ();

	fn update(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}

	fn get_event_source(&mut self) -> Channel<BackendEvent> {
		self.event_receiver
			.take()
			.expect("Already took event source from Winit backend")
	}
}
