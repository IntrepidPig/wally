use std::time::Instant;

use calloop::channel::{self, Channel, Sender};
use thiserror::Error;
use winit::{
	event::{ElementState, Event as WinitEvent, WindowEvent},
	event_loop::{ControlFlow, EventLoop},
};

use crate::backend::{BackendEvent, InputBackend, KeyPress, PointerButton, PointerMotion};
use std::sync::Arc;

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

	pub fn start(sender: Sender<BackendEvent>, event_loop: EventLoop<()>, window: Arc<winit::window::Window>) {
		let start = Instant::now();
		let mut ctrl_pressed = false;
		let mut pointer_grabbed = false;
		event_loop.run(
			move |event: WinitEvent<()>, _event_loop_window_target, control_flow: &mut ControlFlow| {
				*control_flow = ControlFlow::Wait;
				let backend_event = match event {
					WinitEvent::WindowEvent {
						window_id: _window_id,
						event: WindowEvent::CloseRequested,
					} => {
						*control_flow = ControlFlow::Exit;
						Some(BackendEvent::StopRequested)
					}
					WinitEvent::WindowEvent {
						window_id: _window_id,
						event:
							WindowEvent::KeyboardInput {
								device_id: _device_id,
								input,
								is_synthetic: _is_synthetic,
							},
					} => {
						// TODO: store an xkbcommon::xkb::State in here and update it with every key
						// press so we can keep track of modifiers and serialize them
						if input.virtual_keycode == Some(winit::event::VirtualKeyCode::LControl) {
							if input.state == ElementState::Pressed {
								ctrl_pressed = true;
							} else {
								ctrl_pressed = false;
							}
						}
						if input.virtual_keycode == Some(winit::event::VirtualKeyCode::Space) {
							if input.state == ElementState::Pressed && ctrl_pressed {
								if pointer_grabbed {
									pointer_grabbed = false;
									let _ = window
										.set_cursor_grab(false)
										.map_err(|e| log::error!("Failed to release cursor: {}", e));
									window.set_cursor_visible(true);
								} else {
									pointer_grabbed = true;
									let _ = window
										.set_cursor_grab(true)
										.map_err(|e| log::error!("Failed to grab cursor: {}", e));
									window.set_cursor_visible(false);
								}
							}
						}
						Some(BackendEvent::KeyPress(KeyPress {
							serial: crate::compositor::get_input_serial(),
							time: start.elapsed().as_millis() as u32,
							key: input.scancode,
							state: input.state.into(),
						}))
					}
					WinitEvent::DeviceEvent {
						device_id: _device_id,
						event: winit::event::DeviceEvent::MouseMotion { delta },
					} => {
						if pointer_grabbed {
							Some(BackendEvent::PointerMotion(PointerMotion {
								serial: crate::compositor::get_input_serial(),
								time: start.elapsed().as_millis() as u32,
								dx: delta.0,
								dx_unaccelerated: delta.0,
								dy: delta.1,
								dy_unaccelerated: delta.1,
							}))
						} else {
							None
						}
					}
					WinitEvent::DeviceEvent {
						device_id: _device_id,
						event: winit::event::DeviceEvent::Button { button, state },
					} => {
						if pointer_grabbed {
							Some(BackendEvent::PointerButton(PointerButton {
								serial: crate::compositor::get_input_serial(),
								time: start.elapsed().as_millis() as u32,
								button,
								state: state.into(),
							}))
						} else {
							None
						}
					}
					_ => None,
				};
				if let Some(backend_event) = backend_event {
					let _ = sender.send(backend_event).map_err(|e| {
						panic!("Failed to send event to backend: {}", e);
					});
				}
			},
		)
	}

	pub fn get_sender(&self) -> Sender<BackendEvent> {
		self.event_sender.clone()
	}
}

#[derive(Debug, Error)]
pub enum WinitInputBackendError {
	#[error("An unknown error occurred in the winit input backend")]
	Unknown,
}

impl InputBackend for WinitInputBackend {
	type Error = WinitInputBackendError;

	fn update(&mut self) -> Result<(), Self::Error> {
		Ok(())
	}

	fn get_event_source(&mut self) -> Channel<BackendEvent> {
		self.event_receiver
			.take()
			.expect("Already took event source from Winit backend")
	}
}
