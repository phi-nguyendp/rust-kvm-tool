use anyhow::{anyhow, Context, Result};
use std::{
    arch::x86_64::CpuidResult,
    error::Error,
    ffi::{c_int, c_void, CString},
    io,
    ptr::NonNull,
};

use libc::{close, ioctl, mmap, MAP_ANON, MAP_FAILED, MAP_SHARED, O_RDWR, PROT_READ, PROT_WRITE};

const KVM_GET_API_VERSION: libc::c_ulong = 0xae00;
const KVM_CREATE_VM: libc::c_ulong = 0xae01;
const KVM_SET_USER_MEMORY_REGION: libc::c_ulong = 0x4020ae46;
const KVM_CREATE_VCPU: libc::c_ulong = 0xae41;
const KVM_GET_VCPU_MMAP_SIZE: libc::c_ulong = 0xae04;
const KVM_GET_SUPPORTED_CPUID: libc::c_ulong = 0xc008ae05;
const KVM_SET_CPUID2: libc::c_ulong = 0x4008ae90;

const KVM_MAX_CPUID_ENTRIES: usize = 100;

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct kvm_cpuid_entry2 {
    pub function: u32,
    pub index: u32,
    pub flags: u32,
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
    pub padding: [u32; 3],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct kvm_cpuid2 {
    pub nent: u32,
    pub padding: u32,
    // The kernel expects entries to follow immediately.
    // In our wrapper, they will be adjacent.
    pub entries: [kvm_cpuid_entry2; 0],
}

// The wrapper we discussed
#[repr(C)]
pub struct CpuidWrapper {
    pub header: kvm_cpuid2,
    pub entries: [kvm_cpuid_entry2; KVM_MAX_CPUID_ENTRIES],
}

pub struct VCpu {
    pub fd: c_int,
    pub run_ptr: NonNull<kvm_run>,
    pub mmap_size: usize,
}

#[repr(C)]
struct kvm_userspace_memory_region {
    pub slot: u32,            // An index for this memory slot
    pub flags: u32,           // Flags
    pub guest_phys_addr: u64, // Where the guest thinks its RAM starts
    pub memory_size: u64,     // How many bytes of RAM
    pub userspace_addr: u64,  // userspace_addr
}
#[repr(C)]
struct kvm_run {
    pub request_interrupt_window: u8,
    pub immediate_exit: u8,
    pub padding1: [u8; 6],
    pub exit_reason: u32,
    pub ready_for_interrupt_injection: u8,
    pub if_flag: u8,
    pub flags: u16,
    pub cr8: u64,
    pub apic_base: u64,
}

struct Kvm {
    fd: c_int,
}

impl VCpu {
    fn init_cpuid(&self, kvm_fd: c_int) -> Result<()> {
        // 1. Prepare the wrapper with space for 100 entries
        let mut cpuid = CpuidWrapper {
            header: kvm_cpuid2 {
                nent: KVM_MAX_CPUID_ENTRIES as u32,
                padding: 0,
                entries: [],
            },
            entries: [Default::default(); KVM_MAX_CPUID_ENTRIES],
        };

        // 2. Ask KVM to fill the supported features
        // Casting the pointer to our wrapper as a pointer to the header
        let ret = unsafe { ioctl(kvm_fd, KVM_GET_SUPPORTED_CPUID, &mut cpuid.header) };
        if ret < 0 {
            return Err(io::Error::last_os_error())
                .context("failed to get supported cpuid features");
        }

        // 3. Apply these features to our VCPU
        // remember that we input a mutable reference in last ioctl
        let ret = unsafe { ioctl(self.fd, KVM_SET_CPUID2, &cpuid.header) };
        if ret < 0 {
            return Err(io::Error::last_os_error()).context("KVM_SET_CPUID2 failed");
        }

        Ok(())
    }
}

impl Drop for VCpu {
    fn drop(&mut self) {
        unsafe {
            close(self.fd);
        }
    }
}

impl Kvm {
    fn new() -> Result<Self> {
        let kvm_path = CString::new("/dev/kvm")?;
        let kvm_fd = unsafe { libc::open(kvm_path.as_ptr(), O_RDWR) };

        if kvm_fd >= 0 {
            Ok(Self { fd: kvm_fd })
        } else {
            Err(io::Error::last_os_error()).context("failed to open /dev/kvm")
        }
    }

    fn setup(&self) -> Result<()> {
        let version = unsafe { ioctl(self.fd, KVM_GET_API_VERSION, 0) };
        if version < 0 {
            return Err(io::Error::last_os_error()).context("failed to read kvm version");
        }

        if version != 12 {
            anyhow::bail!("Unsupported KVM API version: {}", version);
        }

        Ok(())
    }
}

impl Drop for Kvm {
    fn drop(&mut self) {
        unsafe {
            libc::close(self.fd);
        }
    }
}

pub struct VM {
    kvm: Kvm,
    pub fd: c_int,
    vcpus_fd: Vec<VCpu>,
}

impl VM {
    pub fn new() -> Result<Self> {
        let kvm = Kvm::new()?;
        kvm.setup()?;

        let fd = unsafe { ioctl(kvm.fd, KVM_CREATE_VM, 0) };
        if fd >= 0 {
            Ok(Self {
                kvm,
                fd,
                vcpus_fd: Vec::new(),
            })
        } else {
            Err(io::Error::last_os_error()).context("failed to open VM fd")
        }
    }

    pub fn insert_memory_region(
        &self,
        slot: u32,
        flags: u32,
        mem_size: u64,
        guest_pa: u64,
    ) -> Result<()> {
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

        if addr == libc::MAP_FAILED {
            return Err(io::Error::last_os_error()).context("failed to map memory region");
        }
        let mem_region = kvm_userspace_memory_region {
            slot,
            flags,
            guest_phys_addr: addr as u64,
            memory_size: mem_size,
            userspace_addr: guest_pa,
        };

        let ret = unsafe { ioctl(self.fd, KVM_SET_USER_MEMORY_REGION, &mem_region, 0) };

        if ret < 0 {
            Err(io::Error::last_os_error()).context("cannot insert memory region")
        } else {
            Ok(())
        }
    }

    pub fn create_vcpu(&mut self) -> Result<()> {
        let map_size = unsafe { ioctl(self.kvm.fd, KVM_GET_VCPU_MMAP_SIZE, 0) };
        if map_size < 0 {
            return Err(io::Error::last_os_error()).context("cannot get vcpu mmap size");
        }

        // Create the first and only VCPU
        let vcpu_fd = unsafe { ioctl(self.fd, KVM_CREATE_VCPU, 0) };
        // mmap memory region for kvm run map
        let addr = unsafe {
            mmap(
                core::ptr::null_mut(),
                map_size as usize,
                PROT_READ | PROT_WRITE,
                MAP_SHARED,
                vcpu_fd,
                0,
            )
        };

        if addr == MAP_FAILED {
            return Err(io::Error::last_os_error()).context("failed to map memory for VCPU_MMAP");
        }

        let kvm_run_mmap = NonNull::new(addr as *mut kvm_run).ok_or_else(|| anyhow!("no value"))?;

        self.vcpus_fd.push(VCpu {
            fd: vcpu_fd,
            run_ptr: kvm_run_mmap,
            mmap_size: map_size as usize,
        });

        Ok(())
    }
}

impl Drop for VM {
    fn drop(&mut self) {
        // Don't need to close kvm fd here, the Kvm::Drop already do it
        unsafe {
            libc::close(self.fd);
        }
    }
}
