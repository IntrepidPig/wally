
use crate::{
	backend::{GraphicsBackend, InputBackend},
	compositor::{Compositor, prelude::*},
};

impl<I: InputBackend, G: GraphicsBackend> Compositor<I, G> {
	pub(crate) fn setup_wl_shell_global(&mut self) {
		self.server.register_global(|_new: NewResource<WlShell>| {
			log::warn!("wl_shell interface not implemented");
		});
	}
}
