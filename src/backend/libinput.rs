use crate::backend::{InputBackend, BackendEvent, Backend};
use crate::compositor::Compositor;

pub struct LibinputInputBackend {
	udev: udev::Context,
	libinput: input::Libinput,
	event_source: calloop::Source<calloop::generic::Generic<calloop::generic::EventedRawFd>>,
}

impl LibinputInputBackend {
	pub fn new<B: Backend>(event_loop_handle: calloop::LoopHandle<Compositor<B>>) -> Result<Self, ()> {
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
		libinput.udev_assign_seat("seat0").expect("Failed to assign seat to libinput");
		
		let libinput_raw_fd = std::os::unix::io::AsRawFd::as_raw_fd(&libinput);
		let libinput_evented = calloop::generic::Generic::from_raw_fd(libinput_raw_fd);
		let event_source = event_loop_handle.insert_source(libinput_evented, |event, compositor| {
			let mut backend = compositor.backend.borrow_mut();
			if let Some(event) = backend.poll_for_events() {
				drop(backend);
				compositor.handle_input_event(event)
			}
		}).expect("Failed to insert libinput event source into event loop");
		
		Ok(Self {
			udev,
			libinput,
			event_source,
		})
	}
}

impl InputBackend for LibinputInputBackend {
	type Error = ();
	
	fn update(&mut self) -> Result<(), Self::Error> {
		let _ = self.libinput.dispatch().map_err(|e| {
			log::error!("Failed to dispatch libinput events: {}", e);
		});
		
		Ok(())
	}
	
	fn poll_for_events(&mut self) -> Option<BackendEvent> {
		let _ = self.libinput.dispatch().map_err(|e| {
			log::error!("Failed to dispatch libinput events: {}", e);
		});
		
		if let Some(event) = self.libinput.next() {
			log::warn!("Got event: {:?}", event);
			Some(BackendEvent::KeyPress)
		} else {
			None
		}
	}
}
