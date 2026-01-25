use anyhow::{Context, Result};
use libc::{c_void, mmap, munmap, MAP_ANON, MAP_FAILED, MAP_SHARED, PROT_READ, PROT_WRITE};
use std::ptr::NonNull;

pub(crate) struct MemoryWrapper {
    // NonNull verifies the pointer isn't null and is useful for optimization
    ptr: NonNull<c_void>,
    size: usize,
}

impl Default for MemoryWrapper {
    fn default() -> Self {
        Self {
            ptr: NonNull::dangling(),
            size: 0,
        }
    }
}

#[allow(unused)]
impl MemoryWrapper {
    pub fn new(mem_size: u64) -> Result<Self> {
        let addr = unsafe {
            mmap(
                core::ptr::null_mut(),
                mem_size as usize,
                PROT_READ | PROT_WRITE,
                MAP_ANON | MAP_SHARED,
                -1,
                0,
            )
        };

        if addr == MAP_FAILED {
            return Err(std::io::Error::last_os_error()).context("failed to map memory region");
        }

        let ptr = NonNull::new(addr)
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "mmap returned null"))?;

        Ok(Self {
            ptr,
            size: mem_size as usize,
        })
    }

    pub fn get_size(&self) -> usize {
        self.size
    }

    pub fn as_mut_ptr(&self) -> *mut u8 {
        self.ptr.as_ptr() as *mut u8
    }

    pub fn as_ptr(&self) -> *const u8 {
        self.ptr.as_ptr() as *const u8
    }

    pub fn host_addr(&self) -> u64 {
        self.ptr.as_ptr() as usize as u64
    }
}

impl Drop for MemoryWrapper {
    fn drop(&mut self) {
        unsafe {
            if munmap(self.ptr.as_ptr(), self.size) != 0 {
                eprintln!(
                    "Warning: munmap failed: {}",
                    std::io::Error::last_os_error()
                );
            } else {
                println!("Guest memory unmapped successfully.");
            }
        }
    }
}
