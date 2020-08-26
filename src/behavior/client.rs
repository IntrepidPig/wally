use wl_server::{
	protocol::*,
};

use crate::{
	compositor::prelude::*,
};

#[derive(Debug)]
pub struct ClientState<G: GraphicsBackend> {
	pub(crate) keyboards: Vec<Resource<WlKeyboard, ()>>,
	pub(crate) pointers: Vec<Resource<WlPointer, ()>>,
	pub(crate) outputs: Vec<Resource<WlOutput, OutputData<G>>>,
}

impl<G: GraphicsBackend> ClientState<G> {
	pub fn new() -> Self {
		Self {
			keyboards: Vec::new(),
			pointers: Vec::new(),
			outputs: Vec::new(),
		}
	}
}