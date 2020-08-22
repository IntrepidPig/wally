use std::{
	convert::TryFrom,
	os::{raw::c_void, unix::io::RawFd},
};

use nix::sys::mman;
use wl_server::{
	protocol::*,
};

use crate::backend::ShmBuffer;

#[derive(Debug)]
pub struct EasyShmPool {
	ptr: *mut c_void,
	fd: RawFd,
	size: usize,
}

impl EasyShmPool {
	pub unsafe fn create(fd: RawFd, size: usize) -> Result<Self, nix::Error> {
		let ptr: *mut c_void = mman::mmap(
			std::ptr::null_mut(),
			size,
			mman::ProtFlags::PROT_READ,
			mman::MapFlags::MAP_SHARED,
			fd,
			0,
		)?;
		Ok(Self { ptr, fd, size })
	}

	pub unsafe fn resize(&mut self, new_size: usize) -> Result<(), nix::Error> {
		mman::munmap(self.ptr, self.size)?;
		let new_ptr = mman::mmap(
			std::ptr::null_mut(),
			new_size,
			mman::ProtFlags::PROT_READ,
			mman::MapFlags::MAP_SHARED,
			self.fd,
			0,
		)?;
		self.ptr = new_ptr;
		self.size = new_size;
		Ok(())
	}

	pub unsafe fn duplicate(&self) -> Self {
		EasyShmPool {
			ptr: self.ptr,
			fd: self.fd,
			size: self.size,
		}
	}
}

#[derive(Debug)]
pub struct EasyShmBuffer {
	pub pool: EasyShmPool,
	pub offset: usize,
	pub width: u32,
	pub height: u32,
	pub stride: u32,
	pub format: wl_shm::Format,
}

impl EasyShmBuffer {
	pub fn get_size(&self) -> usize {
		usize::try_from(self.stride)
			.unwrap()
			.checked_mul(usize::try_from(self.height).unwrap())
			.unwrap()
	}

	pub unsafe fn get_ptr(&self) -> *mut u8 {
		let ptr = (self.pool.ptr as *mut u8).offset(self.offset as isize) as *mut _;
		ptr
	}

	pub unsafe fn as_slice<'a>(&self) -> &'a [u8] {
		let ptr = self.get_ptr();
		assert!(self.offset + self.get_size() <= self.pool.size);
		let slice = std::slice::from_raw_parts(ptr as *mut _ as *const _, self.get_size() as usize);
		std::mem::transmute(slice)
	}
}

impl ShmBuffer for EasyShmBuffer {
	fn offset(&self) -> usize {
		self.offset
	}
	fn width(&self) -> u32 {
		self.width
	}
	fn height(&self) -> u32 {
		self.height
	}
	fn stride(&self) -> u32 {
		self.stride
	}
	fn format(&self) -> wl_shm::Format {
		self.format
	}
}

unsafe impl Send for EasyShmPool {}
