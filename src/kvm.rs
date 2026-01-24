use anyhow::{anyhow, Context, Result};
use std::{
    arch::x86_64::CpuidResult,
    error::Error,
    ffi::{c_int, c_void, CString},
    io,
    mem::MaybeUninit,
    ptr::NonNull,
};

use libc::{
    close, ioctl, mmap, MAP_ANON, MAP_FAILED, MAP_SHARED, O_RDWR, PROT_READ, PROT_WRITE, _IOR,
    _IOW, _IOWR,
};

const KVMIO: u32 = 0xae;
const KVM_GET_API_VERSION: libc::c_ulong = 0xae00;
const KVM_CREATE_VM: libc::c_ulong = 0xae01;
const KVM_SET_USER_MEMORY_REGION: libc::c_ulong = 0x4020ae46;
const KVM_CREATE_VCPU: libc::c_ulong = 0xae41;
const KVM_GET_VCPU_MMAP_SIZE: libc::c_ulong = 0xae04;
const KVM_GET_SUPPORTED_CPUID: libc::c_ulong = 0xc008ae05;
const KVM_SET_CPUID2: libc::c_ulong = 0x4008ae90;

const KVM_GET_SREGS: libc::c_ulong = _IOR::<kvm_sregs>(KVMIO, 0x83);
const KVM_SET_SREGS: libc::c_ulong = _IOW::<kvm_sregs>(KVMIO, 0x84);

const KVM_GET_REGS: libc::c_ulong = _IOR::<kvm_sregs>(KVMIO, 0x81);
const KVM_SET_REGS: libc::c_ulong = _IOR::<kvm_sregs>(KVMIO, 0x82);

const KVM_MAX_CPUID_ENTRIES: usize = 100;

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct kvm_segment {
    pub base: u64,
    pub limit: u32,
    pub selector: u16,
    pub type_: u8,
    pub present: u8,
    pub dpl: u8,
    pub db: u8,
    pub s: u8,
    pub l: u8,
    pub g: u8,
    pub avl: u8,
    pub padding: u8,
}

const fn seg_with_st(selector_index: u16, type_: u8) -> kvm_segment {
    kvm_segment {
        base: 0,
        limit: 0x000fffff,
        selector: selector_index << 3,
        // 0b1011: Code, Executed/Read, accessed
        // 0b0011: Data, Read/Write, accessed
        type_,
        present: 1,
        dpl: 0,
        // If L-bit is set, then D-bit must be cleared.
        db: 0,
        s: 1,
        l: 1,
        g: 1,
        avl: 0,
        padding: 0,
    }
}

// Ref: <https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html> 3-10 Vol. 3A
const fn to_gdt_entry(seg: &kvm_segment) -> u64 {
    let base = seg.base;
    let limit = seg.limit as u64;
    // flags: G, DB, L, AVL
    let flags = (seg.g as u64 & 0x1) << 3
        | (seg.db as u64 & 0x1) << 2
        | (seg.l as u64 & 0x1) << 1
        | (seg.avl as u64 & 0x1);
    // access: P, DPL, S, Type
    let access = (seg.present as u64 & 0x1) << 7
        | (seg.dpl as u64 & 0x11) << 5
        | (seg.s as u64 & 0x1) << 4
        | (seg.type_ as u64 & 0x1111);
    ((base & 0xff00_0000u64) << 32)
        | ((base & 0x00ff_ffffu64) << 16)
        | (limit & 0x0000_ffffu64)
        | ((limit & 0x000f_0000u64) << 32)
        | (flags << 52)
        | (access << 40)
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct kvm_dtable {
    pub base: u64,
    pub limit: u16,
    pub padding: [u16; 3],
}

#[repr(C)]
#[derive(Debug, Copy, Clone, Default)]
pub struct kvm_sregs {
    pub cs: kvm_segment,
    pub ds: kvm_segment,
    pub es: kvm_segment,
    pub fs: kvm_segment,
    pub gs: kvm_segment,
    pub ss: kvm_segment,
    pub tr: kvm_segment,
    pub ldt: kvm_segment,
    pub gdt: kvm_dtable,
    pub idt: kvm_dtable,
    pub cr0: u64,
    pub cr2: u64,
    pub cr3: u64, // Pointer to Page Tables (PML4)
    pub cr4: u64,
    pub cr8: u64,
    pub efer: u64,
    pub apic_base: u64,
    pub interrupt_bitmap: [u64; 4],
}

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
#[derive(Default)]
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

    fn enter_long_mode(&self, mem: &mut [u8]) -> Result<()> {
        // We need  identical mapping page table as the linux boot protocol requires
        // Layout:
        // 0x2000: PML4 (PGD)
        // 0x3000: PDPT (PUD)
        // 0x4000: PD   (PMD)
        let pml4_addr: u64 = 0x2000;
        let pdpt_addr: u64 = 0x3000;
        let pd_addr: u64 = 0x4000;

        // Flags: Present|Read/Write
        let flags: u64 = (1 << 0) | (1 << 1);

        // 1. Write PML4 Entry 0 pointing to PDP
        let entry = pdpt_addr | flags;
        mem[0x2000..0x2008].copy_from_slice(&entry.to_le_bytes());

        // 2. Write PDPT Entry 0 point to PD
        let entry = pd_addr | flags;
        mem[0x3000..0x3008].copy_from_slice(&entry.to_le_bytes());

        // 3. Write PD entries (enable 2MB huge page)
        let huge_page_flags = flags | 0x80;
        for i in 0..512 {
            let addr = i * 0x200000; // 2MB: the physical address
            let entry = addr | huge_page_flags;
            let offset = 0x4000 + (i as usize * 8); // offset in PD
            mem[offset..offset + 8].copy_from_slice(&entry.to_le_bytes());
        }

        let mut sregs = kvm_sregs::default();
        // Get the current sregs first to avoid wiping other important deafaults
        let ret = unsafe { ioctl(self.fd, KVM_GET_REGS, &mut sregs, 0) };
        if ret < 0 {
            return Err(io::Error::last_os_error()).context("failed to get current sregs");
        }
        // Create code and data segement
        // I'm not sure if I need to create NULL segment
        // but it works fine without setup one
        const CODE_SEG: kvm_segment = seg_with_st(1, 0b1011);
        const DATA_SEG: kvm_segment = seg_with_st(2, 0b0011);

        // construct kvm_segment and set to segment registers
        sregs.cs = CODE_SEG;
        sregs.ds = DATA_SEG;
        sregs.es = DATA_SEG;
        sregs.fs = DATA_SEG;
        sregs.gs = DATA_SEG;
        sregs.ss = DATA_SEG;

        // Now we need to setup the base GDT table and load it to memory

        // construct gdt table, write to memory and set it to register
        let gdt_table: [u64; 3] = [
            0,                       // NULL
            to_gdt_entry(&CODE_SEG), // CODE
            to_gdt_entry(&DATA_SEG), // DATA
        ];

        // copy it into mapped memory
        // 1.convert the table into [u8]
        let gdt_bytes: &[u8] = unsafe {
            core::slice::from_raw_parts(
                gdt_table.as_ptr() as *const u8,
                core::mem::size_of::<[u64; 3]>(),
            )
        };
        // copy
        mem[16..16 + gdt_bytes.len()].copy_from_slice(gdt_bytes);

        sregs.gdt.base = 0x0; // we put it at 0x0
        sregs.gdt.limit = std::mem::size_of_val(&gdt_table) as u16 - 1;

        // PAE enabling
        sregs.cr4 = 1 << 5;

        // Enable long mode in efer
        sregs.efer |= 1 << 8; // Long Mode enable
        sregs.efer |= 1 << 10; // Long mode Active

        // Setup page table
        sregs.cr3 = 0x2000;

        let ret = unsafe { ioctl(self.fd, KVM_SET_SREGS, &sregs, 0) };
        if ret < 0 {
            return Err(io::Error::last_os_error()).context("failed to get current sregs");
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
    memory_regions: kvm_userspace_memory_region,
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
                memory_regions: kvm_userspace_memory_region::default(),
            })
        } else {
            Err(io::Error::last_os_error()).context("failed to open VM fd")
        }
    }

    pub fn insert_memory_region(
        &mut self,
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
            userspace_addr: addr as u64,
            memory_size: mem_size,
            guest_phys_addr: guest_pa,
        };

        let ret = unsafe { ioctl(self.fd, KVM_SET_USER_MEMORY_REGION, &mem_region, 0) };

        if ret < 0 {
            Err(io::Error::last_os_error()).context("cannot insert memory region")
        } else {
            self.memory_regions = mem_region;
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

    pub fn enter_long_mode(&mut self) -> Result<()> {
        for cpu in &self.vcpus_fd {
            let mem: &mut [u8] = unsafe {
                std::slice::from_raw_parts_mut(
                    self.memory_regions.userspace_addr as *mut u8,
                    self.memory_regions.memory_size as usize,
                )
            };
            cpu.enter_long_mode(mem)?;
        }
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
