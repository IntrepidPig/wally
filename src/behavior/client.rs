use wl_server::{
	protocol::*,
};

use crate::{
	compositor::{seat::SeatData, prelude::*},
};

#[derive(Debug)]
pub struct ClientState<G: GraphicsBackend> {
	pub seat: Option<Resource<WlSeat, SeatData>>,
	pub outputs: Vec<Resource<WlOutput, OutputData<G>>>,
}

impl<G: GraphicsBackend> ClientState<G> {
	pub fn new() -> Self {
		Self {
			seat: None,
			outputs: Vec::new(),
		}
	}
}