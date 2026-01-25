use anyhow::{anyhow, Context, Result};
use kvm_bindings::{kvm_cpuid2, kvm_run__bindgen_ty_1, kvm_run__bindgen_ty_2, KVM_EXIT_IO_OUT};
use std::{
    ffi::CString,
    io::{self, Write},
    os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd},
    ptr::NonNull,
};
use xmas_elf::ElfFile;

use libc::{
    __u64, ioctl, mmap, MAP_ANON, MAP_FAILED, MAP_SHARED, O_RDWR, PROT_READ, PROT_WRITE, _IO, _IOR,
    _IOW, _IOWR,
};

use crate::{
    bootparams::{__u8, boot_params, CAN_USE_HEAP, KEEP_SEGMENTS},
    memory::MemoryWrapper,
};

const KVMIO: u32 = 0xae;
const KVM_GET_API_VERSION: libc::c_ulong = 0xae00;
const KVM_CREATE_VM: libc::c_ulong = 0xae01;
const KVM_SET_USER_MEMORY_REGION: libc::c_ulong = 0x4020ae46;
const KVM_CREATE_VCPU: libc::c_ulong = 0xae41;
const KVM_GET_VCPU_MMAP_SIZE: libc::c_ulong = 0xae04;
const KVM_GET_SUPPORTED_CPUID: libc::c_ulong = _IOWR::<kvm_cpuid2>(KVMIO, 0x5);
const KVM_SET_CPUID2: libc::c_ulong = _IOW::<kvm_cpuid2>(KVMIO, 0x90);

const KVM_GET_SREGS: libc::c_ulong = _IOR::<kvm_sregs>(KVMIO, 0x83);
const KVM_SET_SREGS: libc::c_ulong = _IOW::<kvm_sregs>(KVMIO, 0x84);

const KVM_GET_REGS: libc::c_ulong = _IOR::<kvm_regs>(KVMIO, 0x81);
const KVM_SET_REGS: libc::c_ulong = _IOW::<kvm_regs>(KVMIO, 0x82);
const KVM_RUN: libc::c_ulong = _IO(KVMIO, 0x80);

// Inkernl-IRQ
const KVM_CREATE_IRQCHIP: libc::c_ulong = _IO(KVMIO, 0x60);
const KVM_CREATE_PIT2: libc::c_ulong = _IOW::<kvm_pit_config>(KVMIO, 0x77);

// TSS
const KVM_SET_TSS_ADDR: libc::c_ulong = _IO(KVMIO, 0x47);
// IDENTITY
const KVM_SET_IDENTITY_MAP_ADDR: libc::c_ulong = _IOW::<__u64>(KVMIO, 0x48);

const KVM_MAX_CPUID_ENTRIES: usize = 80;

// Exit reason
const KVM_EXIT_IO: u32 = 2;
const KVM_EXIT_HLT: u32 = 5;

// IO direction
// To lazy to retype everything, just import from kvm_bindings
const ADDR_BOOT_PARAMS: __u64 = 0x10000;
const ADDR_CMDLINE: usize = 0x20000;
const CMDLINE: &[u8] = b"console=ttyS0 earlyprintk=ttyS0";
const ADDR_TSS: __u64 = 0xffff_d000;
const ADDR_IDENTITY_MAP: __u64 = 0xffff_c000;

// for IO
const COM1_PORT: u16 = 0x3f8;
const COM1_DATA: u16 = 0x3f8; // Data register
const COM1_IER: u16 = 0x3f9; // Interrupt Enable
const COM1_IIR: u16 = 0x3fa; // Interrupt ID (read) / FIFO ctrl (write)
const COM1_LCR: u16 = 0x3fb; // Line Control
const COM1_MCR: u16 = 0x3fc; // Modem Control
const COM1_LSR: u16 = 0x3fd; // Line Status
const COM1_MSR: u16 = 0x3fe; // Modem Status

// CR
const X86_CR0_PE: u64 = 0x1;
const X86_CR4_PAE: u64 = 0x20;
const X86_CR4_PSE: u64 = 0x10;
const X86_CR0_PG: u64 = 0x80000000;
const EFER_LME: u64 = 0x100;
const EFER_LMA: u64 = 0x400;

#[repr(C)]
#[derive(Default)]
struct kvm_pit_config {
    flags: u32,
    pad: [u32; 15],
}

#[repr(C)]
#[derive(Default)]
pub struct kvm_regs {
    pub rax: __u64,
    pub rbx: __u64,
    pub rcx: __u64,
    pub rdx: __u64,
    pub rsi: __u64,
    pub rdi: __u64,
    pub rsp: __u64,
    pub rbp: __u64,
    pub r8: __u64,
    pub r9: __u64,
    pub r10: __u64,
    pub r11: __u64,
    pub r12: __u64,
    pub r13: __u64,
    pub r14: __u64,
    pub r15: __u64,
    pub rip: __u64,
    pub rflags: __u64,
}

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
        limit: 0xffffffff,
        selector: selector_index << 3,
        // 0b1011: Code, Executed/Read, accessed
        // 0b0011: Data, Read/Write, accessed
        type_,
        present: 1,
        dpl: 0,
        // If L-bit is set, then D-bit must be cleared.
        db: if type_ == 0b1011 { 0 } else { 1 },
        s: 1,
        l: if type_ == 0b1011 { 1 } else { 0 },
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

pub struct VCpu {
    pub fd: OwnedFd,
    pub run_ptr: NonNull<kvm_run>,
}

#[repr(C)]
#[derive(Default)]
struct kvm_userspace_memory_region {
    pub slot: u32,                     // An index for this memory slot
    pub flags: u32,                    // Flags
    pub guest_phys_addr: u64,          // Where the guest thinks its RAM starts
    pub memory_size: u64,              // How many bytes of RAM
    pub userspace_addr: MemoryWrapper, // userspace_addr
}
#[repr(C)]
#[derive(Debug)]
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
    pub __bindgen_anon_1: kvm_run__bindgen_ty_1,
    pub kvm_valid_regs: __u64,
    pub kvm_dirty_regs: __u64,
    pub s: kvm_run__bindgen_ty_2,
}

struct Kvm {
    fd: OwnedFd,
}

impl VCpu {
    fn init_cpuid(&self, kvm_fd: BorrowedFd) -> Result<()> {
        use kvm_bindings::fam_wrappers::CpuId;
        const KVM_CPUID_SIGNATURE: u32 = 0x40000000;
        const KVM_CPUID_FEATURES: u32 = 0x40000001;
        // 1. Prepare the wrapper with space for 100 entries
        let mut cpuid = CpuId::new(KVM_MAX_CPUID_ENTRIES)?;
        // 2. Ask KVM to fill the supported features
        // Casting the pointer to our wrapper as a pointer to the header
        let ret = unsafe {
            ioctl(
                kvm_fd.as_raw_fd(),
                KVM_GET_SUPPORTED_CPUID,
                cpuid.as_mut_fam_struct_ptr(),
            )
        };
        if ret < 0 {
            return Err(io::Error::last_os_error())
                .context("failed to get supported cpuid features");
        }

        for entry in cpuid.as_mut_slice().iter_mut() {
            if entry.function == KVM_CPUID_SIGNATURE {
                entry.eax = KVM_CPUID_FEATURES;
                entry.ebx = 0x4b4d564b;
                entry.ecx = 0x564b4d56;
                entry.edx = 0x4d;
            }
        }

        // 3. Apply these features to our VCPU
        // remember that we input a mutable reference in last ioctl
        let ret = unsafe {
            ioctl(
                self.fd.as_raw_fd(),
                KVM_SET_CPUID2,
                cpuid.as_mut_fam_struct_ptr(),
            )
        };
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
        let ret = unsafe { ioctl(self.fd.as_raw_fd(), KVM_GET_SREGS, &mut sregs, 0) };
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
        mem[0x500..0x500 + gdt_bytes.len()].copy_from_slice(gdt_bytes);

        sregs.gdt.base = 0x500; // we put it at 0x0
        sregs.gdt.limit = std::mem::size_of_val(&gdt_table) as u16 - 1;

        // Setup page table
        sregs.cr3 = pml4_addr;
        sregs.cr0 |= X86_CR0_PE;
        sregs.cr0 |= X86_CR0_PG;

        // PAE enabling
        sregs.cr4 |= X86_CR4_PAE | X86_CR4_PSE;
        // Enable long mode in efer
        sregs.efer |= EFER_LMA | EFER_LME;

        let ret = unsafe { ioctl(self.fd.as_raw_fd(), KVM_SET_SREGS, &sregs, 0) };
        if ret < 0 {
            return Err(io::Error::last_os_error()).context("failed to set current sregs");
        }

        Ok(())
    }

    fn init_registers(&self, rip: __u64, rsi: __u64) -> Result<()> {
        let mut regs = kvm_regs::default();

        let ret = unsafe { ioctl(self.fd.as_raw_fd(), KVM_GET_REGS, &mut regs, 0) };
        if ret < 0 {
            return Err(io::Error::last_os_error()).context("failed to get current sregs");
        }

        regs.rflags = 2;
        regs.rip = rip;
        regs.rsi = rsi;

        let ret = unsafe { ioctl(self.fd.as_raw_fd(), KVM_SET_REGS, &mut regs, 0) };
        if ret < 0 {
            return Err(io::Error::last_os_error())
                .context("failed to get supported cpuid features");
        }

        Ok(())
    }

    fn handle_io(&self, kvm_run: &kvm_run) -> Result<()> {
        let io = unsafe { kvm_run.__bindgen_anon_1.io };
        let port = io.port;
        let is_out = io.direction as u32 == KVM_EXIT_IO_OUT;
        let data_ptr = unsafe { (self.run_ptr.as_ptr() as *mut u8).add(io.data_offset as usize) };

        match (port, is_out) {
            (COM1_DATA, true) => {
                let ch = unsafe { *data_ptr } as char;
                print!("{}", ch);
                _ = std::io::stdout().flush();
            }
            (COM1_LSR, false) => {
                // Return: Transmitter empty (0x20) | Transmitter holding register empty (0x40)
                unsafe {
                    *data_ptr = 0x60;
                }
            }
            (0x61, false) => unsafe {
                *data_ptr = 0x20;
            },
            (COM1_IER..=COM1_MSR, _) => {
                // Ignore other UART register accesses
                if !is_out {
                    unsafe {
                        *data_ptr = 0;
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn run(&self) -> Result<()> {
        loop {
            let ret = unsafe { ioctl(self.fd.as_raw_fd(), KVM_RUN, 0) };
            if ret < 0 {
                return Err(io::Error::last_os_error())
                    .context("failed to get supported cpuid features");
            }
            let kvm_run = unsafe { self.run_ptr.as_ref() };

            /*
            * DEBUG
            let mut regs = kvm_regs::default();
            let ret = unsafe { ioctl(self.fd, KVM_GET_REGS, &mut regs, 0) };
            if ret < 0 {
                return Err(io::Error::last_os_error()).context("failed to get current sregs");
            }
            let mut sregs = kvm_sregs::default();
            // Get the current sregs first to avoid wiping other important deafaults
            let ret = unsafe { ioctl(self.fd, KVM_GET_SREGS, &mut sregs, 0) };
            if ret < 0 {
                return Err(io::Error::last_os_error()).context("failed to get current sregs");
            }
            println!("{:?} rip: {}", kvm_run, regs.rip);
            */
            match kvm_run.exit_reason {
                KVM_EXIT_IO => {
                    self.handle_io(kvm_run)?;
                }
                KVM_EXIT_HLT => {
                    println!("Guest executed HLT. Stopping.");
                    break Ok(());
                }
                un => {
                    /*
                    let fail_reason = unsafe {
                        kvm_run
                            .__bindgen_anon_1
                            .fail_entry
                            .hardware_entry_failure_reason
                    };
                    println!(
                        "exit reason {} : hardware_entry_failure_reason = {:#x}",
                        un, fail_reason
                    );

                    // Dump CPU state for debugging
                    let mut sregs = kvm_sregs::default();
                    unsafe { ioctl(self.fd, KVM_GET_SREGS, &mut sregs, 0) };
                    println!(
                        "  CR0={:#x} CR3={:#x} CR4={:#x} EFER={:#x}",
                        sregs.cr0, sregs.cr3, sregs.cr4, sregs.efer
                    );
                    println!(
                        "  CS: sel={:#x} base={:#x} limit={:#x} type={} l={} db={}",
                        sregs.cs.selector,
                        sregs.cs.base,
                        sregs.cs.limit,
                        sregs.cs.type_,
                        sregs.cs.l,
                        sregs.cs.db
                    );
                    println!(
                        "  DS: sel={:#x} base={:#x} limit={:#x} type={} l={} db={}",
                        sregs.ds.selector,
                        sregs.ds.base,
                        sregs.ds.limit,
                        sregs.ds.type_,
                        sregs.ds.l,
                        sregs.ds.db
                    );
                    return Err(anyhow!("VM entry failed"));
                    */
                }
            };
        }
    }
}

impl Kvm {
    fn new() -> Result<Self> {
        let kvm_path = CString::new("/dev/kvm")?;
        let kvm_fd = unsafe { libc::open(kvm_path.as_ptr(), O_RDWR) };

        if kvm_fd >= 0 {
            Ok(Self {
                fd: unsafe { OwnedFd::from_raw_fd(kvm_fd) },
            })
        } else {
            Err(io::Error::last_os_error()).context("failed to open /dev/kvm")
        }
    }

    fn setup(&self) -> Result<()> {
        let version = unsafe { ioctl(self.fd.as_raw_fd(), KVM_GET_API_VERSION, 0) };
        if version < 0 {
            return Err(io::Error::last_os_error()).context("failed to read kvm version");
        }

        if version != 12 {
            anyhow::bail!("Unsupported KVM API version: {}", version);
        }

        Ok(())
    }
}

pub struct VM {
    kvm: Kvm,
    pub fd: OwnedFd,
    vcpus_fd: Vec<VCpu>,
    memory_regions: kvm_userspace_memory_region,
}

impl VM {
    pub fn new() -> Result<Self> {
        let kvm = Kvm::new()?;
        kvm.setup()?;

        let fd = unsafe { ioctl(kvm.fd.as_raw_fd(), KVM_CREATE_VM, 0) };
        if fd >= 0 {
            Ok(Self {
                kvm,
                // should be safe because fd is confirmed validity
                fd: unsafe { OwnedFd::from_raw_fd(fd) },
                vcpus_fd: Vec::new(),
                memory_regions: kvm_userspace_memory_region::default(),
            })
        } else {
            Err(io::Error::last_os_error()).context("failed to open VM fd")
        }
    }

    pub fn setup_irqchip(&self) -> Result<()> {
        // TSS ADDR
        let ret = unsafe { ioctl(self.fd.as_raw_fd(), KVM_SET_TSS_ADDR, ADDR_TSS, 0) };
        if ret < 0 {
            return Err(io::Error::last_os_error()).context("KVM_SET_TSS_ADDR failed");
        }

        // IDENTITY mapping
        let ret = unsafe {
            ioctl(
                self.fd.as_raw_fd(),
                KVM_SET_IDENTITY_MAP_ADDR,
                &ADDR_IDENTITY_MAP,
                0,
            )
        };
        if ret < 0 {
            return Err(io::Error::last_os_error()).context("KVM_SET_IDENTITY_MAP_ADDR failed");
        }

        // Create in-kernel APIC
        let ret = unsafe { ioctl(self.fd.as_raw_fd(), KVM_CREATE_IRQCHIP, 0) };
        if ret < 0 {
            return Err(io::Error::last_os_error()).context("KVM_CREATE_IRQCHIP failed");
        }

        // Create PIT (timer)
        let pit_config = kvm_pit_config::default();
        let ret = unsafe { ioctl(self.fd.as_raw_fd(), KVM_CREATE_PIT2, &pit_config) };
        if ret < 0 {
            return Err(io::Error::last_os_error()).context("KVM_CREATE_PIT2 failed");
        }
        Ok(())
    }

    pub fn insert_memory_region(
        &mut self,
        slot: u32,
        flags: u32,
        mem_size: u64,
        guest_pa: u64,
    ) -> Result<()> {
        let host_memory = MemoryWrapper::new(mem_size)?;
        let mem_region = kvm_userspace_memory_region {
            slot,
            flags,
            userspace_addr: host_memory,
            memory_size: mem_size,
            guest_phys_addr: guest_pa,
        };

        let ret = unsafe {
            ioctl(
                self.fd.as_raw_fd(),
                KVM_SET_USER_MEMORY_REGION,
                &mem_region,
                0,
            )
        };

        if ret < 0 {
            Err(io::Error::last_os_error()).context("cannot insert memory region")
        } else {
            self.memory_regions = mem_region;
            Ok(())
        }
    }

    pub fn create_vcpu(&mut self) -> Result<()> {
        let map_size = unsafe { ioctl(self.kvm.fd.as_raw_fd(), KVM_GET_VCPU_MMAP_SIZE, 0) };
        if map_size < 0 {
            return Err(io::Error::last_os_error()).context("cannot get vcpu mmap size");
        }

        // Create the first and only VCPU
        let vcpu_fd = unsafe { ioctl(self.fd.as_raw_fd(), KVM_CREATE_VCPU, 0) };
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
            fd: unsafe { OwnedFd::from_raw_fd(vcpu_fd) },
            run_ptr: kvm_run_mmap,
        });

        Ok(())
    }

    pub fn enter_long_mode(&mut self) -> Result<()> {
        for cpu in &self.vcpus_fd {
            let mem: &mut [u8] = unsafe {
                std::slice::from_raw_parts_mut(
                    self.memory_regions.userspace_addr.as_mut_ptr(),
                    self.memory_regions.memory_size as usize,
                )
            };
            cpu.enter_long_mode(mem)?;
        }
        Ok(())
    }

    fn setup_boot_params(boot_params: &mut boot_params, cmdline_addr: u32, total_mem_bytes: u64) {
        // Constants for E820 types
        const E820_RAM: u32 = 1;
        const E820_RESERVED: u32 = 2;
        // 1. Setup Header (hdr)
        // 0xFF means "Unregistered/Custom Bootloader". Without this, Linux won't boot.
        boot_params.hdr.type_of_loader = 0xFF;
        boot_params.hdr.ext_loader_type = 0;
        // VGA display
        boot_params.hdr.vid_mode = 0xFFFF;

        // 2. Command Line setup
        // Point to where you loaded the "console=ttyS0..." string in guest memory
        boot_params.hdr.cmd_line_ptr = cmdline_addr;
        boot_params.ext_cmd_line_ptr = 0;
        boot_params.hdr.cmdline_size = CMDLINE.len() as u32;

        // No ramdisk now
        boot_params.hdr.ramdisk_size = 0;
        boot_params.hdr.ramdisk_image = 0;

        boot_params.hdr.loadflags = (CAN_USE_HEAP as __u8) | 0x01 | (KEEP_SEGMENTS as __u8);
        boot_params.hdr.heap_end_ptr = 0xFE00;
        boot_params.hdr.ext_loader_ver = 0x0;

        const KERNEL_TYPE_OF_LOADER: u8 = 0xff;
        const KERNEL_BOOT_FLAG_MAGIC_NUMBER: u16 = 0xaa55;
        const KERNEL_HDR_MAGIC_NUMBER: u32 = 0x5372_6448;
        const KERNEL_MIN_ALIGNMENT_BYTES: u32 = 0x0100_0000;

        boot_params.hdr.type_of_loader = KERNEL_TYPE_OF_LOADER;
        boot_params.hdr.boot_flag = KERNEL_BOOT_FLAG_MAGIC_NUMBER;
        boot_params.hdr.header = KERNEL_HDR_MAGIC_NUMBER;
        boot_params.hdr.kernel_alignment = KERNEL_MIN_ALIGNMENT_BYTES;

        // 3. E820 Memory Map
        // We define 2 regions: Low memory and High memory
        boot_params.e820_entries = 2;

        // Entry 0: Low Memory (0 to ~640KB)
        boot_params.e820_table[0].addr = 0x0;
        boot_params.e820_table[0].size = 0x9FC00;
        boot_params.e820_table[0].type_ = E820_RAM;

        // Entry 1: High Memory (1MB up to max memory)
        boot_params.e820_table[1].addr = 0x100000;
        boot_params.e820_table[1].size = total_mem_bytes - 0x100000;
        boot_params.e820_table[1].type_ = E820_RAM;
    }

    pub fn load_code(&self, vmlinux: &Vec<u8>) -> Result<__u64> {
        let elf = ElfFile::new(vmlinux).map_err(|e| anyhow::anyhow!(e))?;
        let mut boot_params = boot_params::default();
        boot_params = unsafe { std::mem::zeroed() };

        Self::setup_boot_params(
            &mut boot_params,
            ADDR_CMDLINE as u32,
            self.memory_regions.memory_size,
        );

        for ph in elf.program_iter() {
            if ph.get_type().unwrap_or(xmas_elf::program::Type::Null)
                == xmas_elf::program::Type::Load
            {
                let offset = ph.offset() as usize;
                let file_size = ph.file_size() as usize;
                let mem_size = ph.mem_size() as usize;
                let paddr = ph.physical_addr() as usize; // GPA

                // Copy data from the ELF into guest memory
                unsafe {
                    let dest = self.memory_regions.userspace_addr.as_mut_ptr();
                    let src = vmlinux[offset..(offset + file_size)].as_ptr();

                    std::ptr::copy_nonoverlapping(src, dest.add(paddr), file_size);

                    // Padding zero (to BSS) if mem_size > file_size
                    if mem_size > file_size {
                        std::ptr::write_bytes(dest.add(paddr + file_size), 0, mem_size - file_size);
                    }
                };
            }
        }
        unsafe {
            let src: *const boot_params = &boot_params as *const boot_params;
            std::ptr::copy_nonoverlapping(
                src as *const u8,
                self.memory_regions.userspace_addr.as_mut_ptr().add(0x10000),
                size_of_val(&boot_params),
            );
            std::ptr::copy_nonoverlapping(
                CMDLINE.as_ptr(),
                self.memory_regions
                    .userspace_addr
                    .as_mut_ptr()
                    .add(ADDR_CMDLINE),
                CMDLINE.len(),
            );
        }
        Ok(elf.header.pt2.entry_point())
    }

    pub fn run(&mut self, entry: __u64) -> Result<()> {
        // actually we only have one cpu
        for vcpu in &self.vcpus_fd {
            vcpu.init_registers(entry, ADDR_BOOT_PARAMS as __u64)?;
            vcpu.init_cpuid(self.kvm.fd.as_fd())?;
            vcpu.run()?;
        }
        Ok(())
    }
}
