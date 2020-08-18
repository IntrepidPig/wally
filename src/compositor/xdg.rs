use std::{
	fmt,
	sync::{Arc, Mutex},
};

use wayland_protocols::xdg_shell::server::{xdg_positioner, xdg_surface, xdg_toplevel, xdg_wm_base};
use wayland_server::{Filter, Main};

use crate::{
	backend::{GraphicsBackend, InputBackend},
	compositor::{prelude::*, role::Role, surface::SurfaceData, Compositor},
};

#[derive(Debug, Default, Clone)]
pub struct XdgSurfacePendingState {
	solid_window_geometry: Option<Rect>,
}

// This object serves as the Role for a WlSurface, and so it is owned by the WlSurface. As such, it
// should not contain a strong reference to the WlSurface or a reference cycle would be created.
#[derive(Debug, Clone)]
pub struct XdgSurfaceData {
	pub pending_state: XdgSurfacePendingState,
	pub solid_window_geometry: Option<Rect>,
	pub xdg_surface_role: Option<XdgSurfaceRole>,
}

#[derive(Clone)]
pub enum XdgSurfaceRole {
	XdgToplevel(xdg_toplevel::XdgToplevel),
}

impl XdgSurfaceRole {
	pub fn resize_window(&self, size: Size) {
		match *self {
			XdgSurfaceRole::XdgToplevel(ref xdg_toplevel) => {
				xdg_toplevel.configure(size.width as i32, size.height as i32, Vec::new());
			}
		}
	}
}

impl XdgSurfaceData {
	pub fn new() -> Self {
		Self {
			pending_state: XdgSurfacePendingState::default(),
			solid_window_geometry: None,
			xdg_surface_role: None,
		}
	}

	pub fn commit_pending_state(&mut self) {
		if let Some(solid_window_geometry) = self.pending_state.solid_window_geometry.take() {
			self.solid_window_geometry = Some(solid_window_geometry);
		}
	}

	pub fn resize_window(&mut self, size: Size) {
		self.xdg_surface_role
			.as_mut()
			.map(|xdg_surface_role| xdg_surface_role.resize_window(size));
	}
}

impl fmt::Debug for XdgSurfaceRole {
	fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
		match *self {
			XdgSurfaceRole::XdgToplevel(ref _xdg_toplevel) => f
				.debug_struct("XdgSurfaceRole::XdgToplevel")
				.field("XdgToplevel", &"<XdgToplevel>")
				.finish(),
		}
	}
}

#[derive(Debug, Clone)]
pub struct XdgToplevelData {
	pub title: Option<String>,
}

impl XdgToplevelData {
	pub fn new() -> Self {
		Self { title: None }
	}
}

impl<I: InputBackend + 'static, G: GraphicsBackend + 'static> Compositor<I, G> {
	pub(crate) fn setup_xdg_wm_base_global(&mut self) {
		let inner = Arc::clone(&self.inner);
		let xdg_wm_base_filter = Filter::new(
			move |(main, _num): (Main<xdg_wm_base::XdgWmBase>, u32), _filter, _dispatch_data| {
				let inner = Arc::clone(&inner);
				main.quick_assign(move |_main, request: xdg_wm_base::Request, _| {
					let inner = Arc::clone(&inner);
					match request {
						xdg_wm_base::Request::Destroy => {
							log::debug!("Got xdg_wm_base destroy request");
						}
						xdg_wm_base::Request::CreatePositioner { id } => {
							log::debug!("Got xdg_wm_base create_positioner request");
							id.quick_assign(
								|_main: Main<xdg_positioner::XdgPositioner>, request: xdg_positioner::Request, _| {
									match request {
										xdg_positioner::Request::Destroy => {
											log::debug!("Got xdg_positioner destroy request");
										}
										xdg_positioner::Request::SetSize { width, height } => {
											log::debug!("Got xdg_positioner set_size request for {}x{}", width, height);
										}
										xdg_positioner::Request::SetAnchorRect { x, y, width, height } => {
											log::debug!(
												"Got xdg_positioner set_anchor_rect request for ({}, {}), {}x{}",
												x,
												y,
												width,
												height
											);
										}
										xdg_positioner::Request::SetAnchor { anchor } => {
											log::debug!("Got xdg_positioner set anchor request: {:?}", anchor);
										}
										xdg_positioner::Request::SetGravity { gravity } => {
											log::debug!("Got xdg_positioner set gravity request: {:?}", gravity);
										}
										xdg_positioner::Request::SetConstraintAdjustment { constraint_adjustment } => {
											log::debug!(
												"Got xdg_positioner set constraint adjustment request: {:?}",
												constraint_adjustment
											);
										}
										xdg_positioner::Request::SetOffset { x, y } => {
											log::debug!("Got xdg_positioner set offset request: ({}, {})", x, y);
										}
										_ => {
											log::warn!("Got unknown request for xdg_positioner");
										}
									}
								},
							);
						}
						xdg_wm_base::Request::GetXdgSurface {
							id: xdg_surface_id,
							surface,
						} => {
							log::debug!("Got xdg_wm_base get_xdg_surface request");
							let xdg_surface = (*xdg_surface_id).clone();
							let xdg_surface_data = Arc::new(Mutex::new(XdgSurfaceData::new()));
							let xdg_surface_data_clone = Arc::clone(&xdg_surface_data);
							xdg_surface
								.as_ref()
								.user_data()
								.set_threadsafe(move || xdg_surface_data_clone);
							xdg_surface_id.quick_assign(
								move |_main: Main<xdg_surface::XdgSurface>, request: xdg_surface::Request, _| {
									let inner = Arc::clone(&inner);
									match request {
										xdg_surface::Request::Destroy => {
											log::debug!("Got xdg_surface destroy request");
										}
										xdg_surface::Request::GetToplevel { id: xdg_toplevel_id } => {
											log::debug!("Got xdg_surface get_top_level request");

											// Set the xdg toplevel data
											let xdg_toplevel = (*xdg_toplevel_id).clone();
											let xdg_toplevel_data = Arc::new(Mutex::new(XdgToplevelData::new()));
											let xdg_toplevel_data_clone = Arc::clone(&xdg_toplevel_data);
											xdg_toplevel
												.as_ref()
												.user_data()
												.set_threadsafe(move || xdg_toplevel_data_clone);

											// Now that the surface has been assigned as a toplevel we assign the role to the wl_surface and the xdg_surface
											let surface_data = surface.get_synced::<SurfaceData<G>>();
											let mut surface_data_lock = surface_data.lock().unwrap();
											surface_data_lock.role = Some(Role::XdgSurface(xdg_surface.clone()));
											drop(surface_data_lock);
											let mut xdg_surface_data_lock = xdg_surface_data.lock().unwrap();
											xdg_surface_data_lock.xdg_surface_role =
												Some(XdgSurfaceRole::XdgToplevel(xdg_toplevel));
											drop(xdg_surface_data_lock);

											let mut inner_lock = inner.lock().unwrap();
											inner_lock.window_manager.manager_impl.add_surface(surface.clone());

											xdg_toplevel_id.quick_assign(
												move |_main, request: xdg_toplevel::Request, _| {
													let toplevel_data = Arc::clone(&xdg_toplevel_data);
													match request {
														xdg_toplevel::Request::Destroy => {
															log::debug!("Got xdg_toplevel destroy request");
														}
														xdg_toplevel::Request::SetParent { parent } => {
															log::debug!(
																"Got xdg_toplevel set_parent request on {:?}",
																parent.map(|parent| parent.as_ref().id())
															);
														}
														xdg_toplevel::Request::SetTitle { title } => {
															log::debug!("Got xdg_toplevel set_title request");
															let mut toplevel_data_lock = toplevel_data.lock().unwrap();
															toplevel_data_lock.title = Some(title);
														}
														xdg_toplevel::Request::SetAppId { app_id } => {
															log::debug!(
																"Got xdg_toplevel set_app_id request: '{}'",
																app_id
															);
														}
														xdg_toplevel::Request::ShowWindowMenu { .. } => {
															log::debug!("Got xdg_toplevel show_window_meny request");
														}
														xdg_toplevel::Request::Move { seat: _seat, serial: _serial } => {
															log::debug!("Got xdg_toplevel move request");
														}
														xdg_toplevel::Request::Resize { seat: _seat, serial: _serail, edges: _edges, } => {
															log::debug!("Got xdg_toplevel resize request");
														}
														xdg_toplevel::Request::SetMaxSize { .. } => {
															log::debug!("Got xdg_toplevel set_max_size request");
														}
														xdg_toplevel::Request::SetMinSize { .. } => {
															log::debug!("Got xdg_toplevel set_min_size request");
														}
														xdg_toplevel::Request::SetMaximized => {
															log::debug!("Got xdg_toplevel set_maximized request");
														}
														xdg_toplevel::Request::UnsetMaximized => {
															log::debug!("Got xdg_toplevel unset_maximized request");
														}
														xdg_toplevel::Request::SetFullscreen { .. } => {
															log::debug!("Got xdg_toplevel set_fullscreen request");
														}
														xdg_toplevel::Request::UnsetFullscreen => {
															log::debug!("Got xdg_toplevel unset_fullscreen request");
														}
														xdg_toplevel::Request::SetMinimized => {
															log::debug!("Got xdg_toplevel set_minimized request");
														}
														_ => {
															log::warn!("Got unknown request for xdg_toplevel");
														}
													}
												},
											);
										}
										xdg_surface::Request::GetPopup { .. } => {
											log::debug!("Got xdg_surface get_popup request");
										}
										xdg_surface::Request::SetWindowGeometry { x, y, width, height } => {
											log::debug!("Got xdg_surface set_window_geometry request");
											let solid_window_geometry = Rect::new(x, y, width as u32, height as u32);
											let mut xdg_surface_data_lock = xdg_surface_data.lock().unwrap();
											xdg_surface_data_lock.solid_window_geometry = Some(solid_window_geometry);
										}
										xdg_surface::Request::AckConfigure { .. } => {
											log::debug!("Got xdg_surface ack_configure request");
										}
										_ => log::warn!("Got unknown request for xdg_surface"),
									}
								},
							);
						}
						xdg_wm_base::Request::Pong { serial } => {
							log::debug!("Got xdg_wm_base pong request with serial: {}", serial);
						}
						_ => {
							log::warn!("Got unknown request for xdg_wm_base");
						}
					}
				});
			},
		);
		self.display
			.create_global::<xdg_wm_base::XdgWmBase, _>(2, xdg_wm_base_filter);
	}
}
