#[allow(unused)]
use std::{
	io::{self, Read, Write},
	thread,
	sync::{
		mpsc::{self, Sender, Receiver},
	},
	os::unix::{
		io::{RawFd, AsRawFd},
		net::{UnixListener, UnixStream},
	},
	ffi::{CString},
	collections::{HashMap, VecDeque},
	cell::{RefCell},
	fmt,
};

use byteorder::{NativeEndian, ReadBytesExt, WriteBytesExt};
use graph_storage::{Key, GraphStorage};
use thiserror::{Error};

use wl_common::{
	wire::{MessageHeader, RawMessage, RawMessageReader, DynArgument, DynMessage, SerializeRawError},
	protocol::{Interface, DynInterface, Message, FromArgsError, IntoArgsError},
	resource::{Resource, Untyped, ResourceManager, ClientHandle, ObjectHandle, GlobalHandle, Client},
};
use wl_protocol::wl::*;

pub enum ServerMessage {
	NewClient(UnixStream),
}

#[derive(Debug, Error)]
pub enum ServerError {
	#[error("Failed to create wayland server")]
	SocketBind(#[from] ServerCreateError),
	#[error("Failed to accept connection from client")]
	AcceptError(#[source] io::Error),
	#[error("Received a message in an invalid format")]
	InvalidMessage,
	#[error("An unknown IO error occurred")]
	UnknownIoError(#[from] io::Error),
}

#[derive(Debug, Error)]
pub enum ServerCreateError {
	#[error("Failed to bind wayland server socket")]
	SocketBind(#[source] io::Error),
	#[error("An unknown IO error occurred")]
	UnknownIoError(#[from] io::Error),
}

const MAX_MESSAGE_SIZE: usize = 4096;
const MAX_FDS: usize = 16;

#[derive(Debug)]
pub struct NetClient {
	stream: UnixStream,
	client_handle: ClientHandle,
}

impl NetClient {
	pub fn new(stream: UnixStream, client_handle: ClientHandle) -> Self {
		Self {
			stream,
			client_handle,
		}
	}
}

type DynHandlerCallback = Box<dyn FnMut(&mut ServerData, &mut ImplementationRef, Resource<DynInterface>, u16, Vec<DynArgument>)>;

pub struct Handler {
	resource: Resource<DynInterface>,
	callback: DynHandlerCallback,
}

impl fmt::Debug for Handler {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct("Handler")
			.field("resource", &self.resource)
			.field("callback", &"<callback>")
			.finish()
	}
}

pub struct UniversalHandler {
	interface: &'static str,
	version: u32,
	callback: DynHandlerCallback,
}

impl fmt::Debug for UniversalHandler {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct("UniversalHandler")
			.field("interface", &self.interface)
			.field("version", &self.version)
			.field("callback", &"<callback>")
			.finish()
	}
}

#[derive(Debug)]
pub struct Server {
	pub data: ServerData,
	pub implementation: Implementation,
	pub implementation_ref: ImplementationRef,
}

#[derive(Debug)]
pub struct ServerData {
	net_manager: NetManager,
	pub resources: ResourceManager,
	next_serial: u32,
}

struct NetManager {
	listener: UnixListener,
	net_clients: GraphStorage<NetClient>,
	msg_buf: Box<[u8; MAX_MESSAGE_SIZE]>,
	fds_buf: [RawFd; MAX_FDS],
}

#[derive(Debug)]
pub struct ImplementationRef {
	pending_implementation_changes: Vec<PendingImplementationChange>,
}

impl ImplementationRef {
	pub fn new() -> Self {
		Self {
			pending_implementation_changes: Vec::new(),
		}
	}

	pub fn register_universal_handler<I: Interface + fmt::Debug, F: FnMut(&mut ServerData, &mut ImplementationRef, Resource<I>, I::Request) + 'static>(&mut self, mut callback: F) {
		// This is where the type-safety magic happens!
		self.pending_implementation_changes.push(PendingImplementationChange::NewUniversalHandler(UniversalHandler {
			interface: I::NAME,
			version: I::VERSION,
			callback: Box::new(move |server_data, impl_ref, untyped_resource, opcode, args| {
				let typed_resource = untyped_resource.downcast::<I>().unwrap();
				dbg!(&typed_resource);
				//dbg!(opcode, &args);
				let request = <I::Request as Message>::from_args(&mut server_data.resources, typed_resource.client(), opcode, args).unwrap();
				callback(server_data, impl_ref, typed_resource, request);
			})
		}))
	}

	pub fn register_handler<I: Interface, F: FnMut(&mut ServerData, &mut ImplementationRef, Resource<I>, I::Request) + 'static>(&mut self, resource: &Resource<I>, mut callback: F) {
		self.pending_implementation_changes.push(PendingImplementationChange::NewHandler(
			Handler {
				resource: resource.to_dyn(),
				callback: Box::new(move |server_data, impl_ref, untyped_resource, opcode, args| {
					dbg!(&untyped_resource);
					dbg!(I::NAME);
					let typed_resource = untyped_resource.downcast::<I>().unwrap();
					dbg!(&typed_resource);
					dbg!((opcode, &args));
					let request = <I::Request as Message>::from_args(&mut server_data.resources, typed_resource.client(), opcode, args).unwrap();
					callback(server_data, impl_ref, typed_resource, request);
				})
			}
		));
	}
}

#[derive(Debug)]
pub enum PendingImplementationChange {
	NewUniversalHandler(UniversalHandler),
	NewHandler(Handler),
}

impl fmt::Debug for NetManager {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		f.debug_struct("NetManager")
			.field("listener", &self.listener)
			.field("net_clients", &self.net_clients)
			.finish()
	}
}

#[derive(Debug)]
pub struct Implementation {
	universal_handlers: Vec<UniversalHandler>,
	individual_handlers: Vec<Handler>,
}


impl Server {
	pub fn new() -> Result<Self, ServerCreateError> {
		Ok(Self {
			data: ServerData::new()?,
			implementation: Implementation::new(),
			implementation_ref: ImplementationRef::new(),
		})
	}

	pub fn run(&mut self) -> Result<(), ServerError> {
		while let Some(pending_change) = self.implementation_ref.pending_implementation_changes.pop() {
			self.implementation.apply_change(pending_change);
		}

		loop {
			match self.data.try_accept_client() {
				Ok(Some(_)) => log::info!("Client connected"),
				Ok(None) => {},
				Err(e) => log::error!("Client connection error"),
			}
			
			if let Some((resource, raw_message)) = self.data.try_next_raw_message()? {
				dbg!(&resource);
				dbg!(&resource.interface());
				dbg!(&raw_message);
				
				let data = &mut self.data;
				let implementation = &mut self.implementation;
				let implementation_ref = &mut self.implementation_ref;
				
				let mut handled = false;

				let object_info = data.resources.get_object_info_untyped(&resource.to_untyped()).unwrap().clone();
				dbg!(&object_info);

				let reader = RawMessageReader::new(&raw_message);
				let args = wl_common::wire::DynMessage::parse_dyn_args(&mut data.resources, resource.client(), object_info.interface.requests[raw_message.header.opcode as usize], reader).unwrap(); // TODO handle malformed request
				dbg!(&args);

				if object_info.interface.name == WlRegistry::NAME {
					let req = WlRegistryRequest::from_args(&mut data.resources, resource.client(), raw_message.header.opcode, args.clone()).unwrap();
					match req {
						WlRegistryRequest::Bind(bind) => {
							let global = data.resources.find_global_handle_untyped(|info| info.name == bind.name).unwrap();
							let global_info = data.resources.get_global_info_untyped(global).unwrap().clone();
							data.resources.set_resource_interface_untyped(&bind.id, global_info.interface.clone());
						}
					}
				}

				for universal_handler in &mut implementation.universal_handlers {
					if universal_handler.interface == resource.interface().name && universal_handler.version == resource.interface().version {
						(universal_handler.callback)(data, implementation_ref, resource.clone(), raw_message.header.opcode, args.clone()); // TODO pass args by reference
						handled = true;
					}
				}
				for handler in &mut implementation.individual_handlers {
					if handler.resource.client() == resource.client() && handler.resource.object() == resource.object() {
						(handler.callback)(data, implementation_ref, resource.clone(), raw_message.header.opcode, args.clone()); // TODO pass args by reference
					}
				}
				while let Some(pending_change) = implementation_ref.pending_implementation_changes.pop() {
					implementation.apply_change(pending_change);
				}
				if !handled {
					log::warn!("Unhandled message for resource {:#?}: {:#?}", resource, raw_message);
				}
			}
		}
	}

	pub fn print_debug_info(&self) {
		log::debug!("Server clients: {:#?}", self.data.resources.clients);
	}
}

#[derive(Debug, Error)]
pub enum SendEventError {
	#[error(transparent)]
	IntoArgsError(#[from] IntoArgsError),
	#[error(transparent)]
	SerializeRawError(#[from] SerializeRawError),
	#[error("The client referred to does not exist")]
	ClientMissing,
	#[error("The sender referred to does not exist")]
	SenderMissing,
}

impl ServerData {
	pub fn new() -> Result<Self, ServerCreateError> {
		Ok(Self {
			net_manager: NetManager::new()?,
			resources: ResourceManager::new(),
			next_serial: 1,
		})
	}

	pub fn next_serial(&mut self) -> u32 {
		let serial = self.next_serial;
		self.next_serial += 1;
		serial
	}

	pub fn try_accept_client(&mut self) -> Result<Option<ClientHandle>, ()> {
		if let Some(stream) = self.net_manager.try_accept_stream().unwrap() { // TODO fix unwrap
			let client = Client::new();
			let client_handle = self.resources.add_client(client);
			self.net_manager.net_clients.add(NetClient::new(stream, client_handle));
			Ok(Some(client_handle))
		} else {
			Ok(None)
		}
	}

	pub fn try_next_raw_message(&mut self) -> Result<Option<(Resource<DynInterface>, RawMessage)>, ServerError> {
		use nix::{
			sys::{socket, uio::IoVec},
			poll,
		};

		let poll_targets = self.net_manager.net_clients
			.kv_iter()
			.map(|(key, client)| {
				(client.client_handle, client.stream.as_raw_fd())
			})
			.collect::<Vec<_>>();
		let mut pollfds = poll_targets.iter().map(|t| poll::PollFd::new(t.1, poll::PollFlags::POLLIN)).collect::<Vec<_>>();
		poll::poll(&mut pollfds, 0).map_err(|e| {
			log::error!("Polling fds failed: {}", e);
		}).unwrap();

		let mut cmsg_buffer = nix::cmsg_space!([RawFd; MAX_FDS]);

		for (i, (client_handle, _)) in poll_targets.iter().enumerate() {
			let pollfd = &pollfds[i];
			if pollfd.revents().map(|revents| !(revents & poll::PollFlags::POLLIN).is_empty()).unwrap_or(false) {
				if !(pollfd.revents().unwrap() & poll::PollFlags::POLLHUP).is_empty() {
					// TODO: destroy client
					log::trace!("Client {:?} disconnected", client_handle);
					self.net_manager.remove_client(*client_handle);
					self.resources.remove_client(*client_handle);
					continue;
				}

				let fd = pollfd.fd();
				cmsg_buffer.clear();
				
				let mut header_buf = [0u8; 8];
				let iovec = IoVec::from_mut_slice(&mut header_buf);
				let flags = socket::MsgFlags::MSG_PEEK | socket::MsgFlags::MSG_DONTWAIT;
				let recv = socket::recvmsg(fd, &[iovec], None, flags).unwrap();
				if recv.bytes != 8 {
					log::error!("Header read returned {} bytes instead of 8", recv.bytes);
					return Err(ServerError::InvalidMessage)
				}
				let msg_header = MessageHeader::from_bytes(&header_buf).unwrap();

				let iovec = IoVec::from_mut_slice(&mut self.net_manager.msg_buf[..msg_header.msg_size as usize]);
				let flags = socket::MsgFlags::MSG_CMSG_CLOEXEC | socket::MsgFlags::MSG_DONTWAIT;
				let recv = socket::recvmsg(fd, &[iovec], Some(&mut cmsg_buffer), flags).unwrap();
				dbg!(&recv);
				let buf = &self.net_manager.msg_buf[..recv.bytes];
				let mut fds = Vec::new();
				let _ = recv.cmsgs().map(|cmsg| match cmsg {
					socket::ControlMessageOwned::ScmRights(fds_) => fds.extend_from_slice(&fds_),
					_ => {},
				}).collect::<()>();
				let raw = match RawMessage::from_data(buf, fds).map_err(|_| ServerError::InvalidMessage) {
					Ok(raw) => raw,
					Err(e) => {
						log::error!("Failed to read message from client");
						continue;
					}
				};

				let object_handle = self.resources.find_object_handle(*client_handle, raw.header.sender).unwrap();
				let resource = self.resources.get_resource_dyn(Resource::new_untyped(*client_handle, object_handle)).unwrap();
				return Ok(Some((resource, raw)));
			}
		}

		Ok(None)
	}

	pub fn send_event<I: Interface + Copy>(&mut self, resource: &Resource<I>, event: I::Event) -> Result<(), SendEventError> where I::Event: fmt::Debug {
		dbg!(&resource);
		let object_info = self.resources.get_object_info(resource).ok_or(SendEventError::SenderMissing)?.clone();
		let id = object_info.id;
		let args = event.into_args(&self.resources, resource.client())?;
		let dyn_msg = DynMessage::new(id, args.0, args.1);
		let raw = dyn_msg.into_raw(&self.resources)?;
		let net_client = self.net_manager.get_client_mut(resource.client()).ok_or(SendEventError::ClientMissing)?;
		let mut data = Vec::with_capacity(raw.header.msg_size as usize);
		data.write_u32::<NativeEndian>(raw.header.sender).unwrap();
		data.write_u16::<NativeEndian>(raw.header.opcode).unwrap();
		data.write_u16::<NativeEndian>(raw.header.msg_size).unwrap();
		data.extend_from_slice(&raw.data);
		net_client.stream.write_all(&data).unwrap();
		log::trace!(
			" -> {interface_name}.{interface_version}@{object_id} {event:?}",
			interface_name=object_info.interface.name,
			interface_version=object_info.interface.version,
			object_id=id,
			event=event
		);
		Ok(())
	}

	pub fn register_new_global<I: Interface>(&mut self) -> Result<GlobalHandle, ()> {
		for global in &self.resources.globals {
			if global.interface == I::as_dyn() { // TODO proper comparison
				// A global with this interface was already registered
				return Err(())
			}
		}
		let global_handle = self.resources.add_global::<I>();
		Ok(global_handle)
	}

	pub fn advertise_global(&mut self, global_handle: GlobalHandle, registry: &Resource<WlRegistry>) {
		dbg!(&self.resources);
		dbg!(&registry);
		let global_info = self.resources.get_global_info_untyped(global_handle).unwrap().clone();
		self.send_event(registry, WlRegistryEvent::Global(wl_registry::GlobalEvent {
			name: global_info.name,
			interface: CString::new(&*global_info.interface.name).unwrap().into_bytes_with_nul(),
			version: global_info.interface.version,
		})).unwrap();
	}

	pub fn bind_global<I: Interface>(&mut self, global_handle: GlobalHandle, resource: &Resource<Untyped>) -> Result<Resource<I>, ()> {
		panic!("Do not call this");
		self.resources.set_resource_interface::<I>(resource).ok_or(())
	}
}

impl NetManager {
	pub fn new() -> Result<Self, ServerCreateError> {
		let listener = UnixListener::bind("/run/user/1000/wayland-0")
			.map_err(|e| ServerCreateError::SocketBind(e))?;
		listener.set_nonblocking(true)?;

		Ok(Self {
			listener,
			net_clients: GraphStorage::new(),
			msg_buf: Box::new([0u8; 4096]),
			fds_buf: [0; MAX_FDS],
		})
	}

	fn try_accept_stream(&mut self) -> Result<Option<UnixStream>, ServerError> {
		match self.listener.accept() {
			Ok((stream, _addr)) => {
				Ok(Some(stream))
			},
			Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
				Ok(None)
			},
			Err(e) => {
				Err(ServerError::AcceptError(e))
			},
		}
	}

	pub fn get_client_mut(&mut self, client_handle: ClientHandle) -> Option<&mut NetClient> {
		self.net_clients.find_mut(|c| c.client_handle == client_handle)
	}

	pub fn remove_client(&mut self, client_handle: ClientHandle) {
		if let Some(key) = self.net_clients.find_key(|c| c.client_handle == client_handle) {
			let _ = self.net_clients.remove(key);
		}
	}
}

impl Implementation {
	pub fn new() -> Self {
		Self {
			universal_handlers: Vec::new(),
			individual_handlers: Vec::new(),
		}
	}

	fn apply_change(&mut self, change: PendingImplementationChange) {
		match change {
		    PendingImplementationChange::NewUniversalHandler(handler) => {
				self.universal_handlers.push(handler);
			},
			
		    PendingImplementationChange::NewHandler(handler) => {
				self.individual_handlers.push(handler);
			},
		}
	}
}
