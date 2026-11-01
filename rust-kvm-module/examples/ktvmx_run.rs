//! Userspace driver for /dev/ktvmx (stage 11). PROVIDED, complete: it is the "VMM" half
//! of the course, written in the same style as rust-kvm-tool (raw ioctls + mmap).
//!
//! Loads a flat real-mode binary (see guest/Makefile) at guest-physical 0x1000, starts it
//! at CS:IP = 0:0x1000 and runs it, printing everything it writes to COM1 (port 0x3f8).
//!
//!     make -C guest
//!     cargo run --example ktvmx_run -- guest/07_hello.bin [/dev/ktvmx]
//!
//! In the QEMU guest: qemu/stage11_userspace.sh.
use ktvmx::vmx_uapi::*;
use std::{
    fs,
    io::{self, Write},
    os::fd::{AsRawFd, RawFd},
    process::exit,
};

const RAM_SIZE: usize = 2 << 20;
const LOAD_ADDR: usize = 0x1000;
const COM1_DATA: u16 = 0x3f8;
const COM1_LSR: u16 = 0x3fd;

fn ioctl<T>(fd: RawFd, cmd: u32, arg: *mut T) -> io::Result<i32> {
    // SAFETY: plain ioctl; `arg` points to a live struct of the size encoded in `cmd`.
    let r = unsafe { libc::ioctl(fd, cmd as libc::c_ulong, arg) };
    if r < 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(r)
    }
}

fn flat_segment(selector: u16, type_: u8, s: u8) -> ktvmx_segment {
    ktvmx_segment {
        base: 0,
        limit: 0xffff,
        selector,
        type_,
        present: 1,
        dpl: 0,
        db: 0,
        s,
        l: 0,
        g: 0,
        avl: 0,
        unusable: 0,
        padding: 0,
    }
}

fn main() -> io::Result<()> {
    let mut args = std::env::args().skip(1);
    let payload = fs::read(args.next().expect("usage: ktvmx_run <flat.bin> [/dev/ktvmx]"))?;
    let dev = args.next().unwrap_or_else(|| "/dev/ktvmx".into());
    let file = fs::OpenOptions::new().read(true).write(true).open(&dev)?;
    let fd = file.as_raw_fd();
    let null = std::ptr::null_mut::<u8>();

    let ver = ioctl(fd, KTVMX_GET_API_VERSION, null)?;
    assert_eq!(ver, KTVMX_API_VERSION, "unexpected API version");
    ioctl(fd, KTVMX_CREATE_VM, null)?;
    ioctl(fd, KTVMX_CREATE_VCPU, null)?;

    // Guest RAM: anonymous memory, filled, then handed to the module (it copies it).
    // SAFETY: standard anonymous mapping.
    let ram = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            RAM_SIZE,
            libc::PROT_READ | libc::PROT_WRITE,
            libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
            -1,
            0,
        )
    };
    assert_ne!(ram, libc::MAP_FAILED, "mmap ram");
    assert!(LOAD_ADDR + payload.len() <= RAM_SIZE);
    // SAFETY: bounds checked just above.
    unsafe { std::ptr::copy_nonoverlapping(payload.as_ptr(), (ram as *mut u8).add(LOAD_ADDR), payload.len()) };
    let mut region = ktvmx_userspace_memory_region {
        slot: 0,
        flags: 0,
        guest_phys_addr: 0,
        memory_size: RAM_SIZE as u64,
        userspace_addr: ram as u64,
    };
    ioctl(fd, KTVMX_SET_USER_MEMORY_REGION, &mut region)?;

    // The shared run area (page 0: ktvmx_run, page 1: PIO data).
    let map_len = ioctl(fd, KTVMX_GET_VCPU_MMAP_SIZE, null)? as usize;
    // SAFETY: mapping the vcpu area of our own fd.
    let run_ptr = unsafe {
        libc::mmap(std::ptr::null_mut(), map_len, libc::PROT_READ | libc::PROT_WRITE, libc::MAP_SHARED, fd, 0)
    };
    assert_ne!(run_ptr, libc::MAP_FAILED, "mmap vcpu run area");
    let run = run_ptr as *mut ktvmx_run;

    // Real mode, flat: every segment base 0, CS:IP = 0:0x1000.
    let mut sregs = ktvmx_sregs::default();
    ioctl(fd, KTVMX_GET_SREGS, &mut sregs)?;
    sregs.cs = flat_segment(0, 0xb, 1);
    for s in [&mut sregs.ds, &mut sregs.es, &mut sregs.fs, &mut sregs.gs, &mut sregs.ss] {
        *s = flat_segment(0, 0x3, 1);
    }
    sregs.tr = flat_segment(0, 0xb, 0);
    sregs.ldt = flat_segment(0, 0x2, 0);
    sregs.idt.limit = 0x3ff;
    sregs.cr0 = 0;
    sregs.cr3 = 0;
    sregs.cr4 = 0;
    sregs.efer = 0;
    ioctl(fd, KTVMX_SET_SREGS, &mut sregs)?;
    let mut regs = ktvmx_regs { rip: LOAD_ADDR as u64, rflags: 2, ..Default::default() };
    ioctl(fd, KTVMX_SET_REGS, &mut regs)?;

    let out = io::stdout();
    for n in 0..1_000_000u32 {
        ioctl(fd, KTVMX_RUN, null)?;
        // SAFETY: `run` is the live shared mapping; the kernel filled it before returning.
        let (reason, io_exit) = unsafe { ((*run).exit_reason, (*run).exit.io) };
        match reason {
            KTVMX_EXIT_IO => {
                // SAFETY: data_offset is inside the 2-page mapping by contract.
                let data = unsafe { (run_ptr as *mut u8).add(io_exit.data_offset as usize) };
                if io_exit.direction == KTVMX_EXIT_IO_OUT {
                    if io_exit.port == COM1_DATA {
                        // SAFETY: at least `size` (<= 4) bytes were written by the kernel.
                        out.lock().write_all(&[unsafe { *data }])?;
                    }
                } else {
                    let v = if io_exit.port == COM1_LSR { 0x60 } else { 0xff };
                    // SAFETY: the PIO page is ours to fill for an IN.
                    unsafe { std::ptr::write_bytes(data, v, io_exit.size as usize) };
                }
            }
            KTVMX_EXIT_HLT => {
                out.lock().flush()?;
                eprintln!("[ktvmx_run] guest halted after {} exits", n + 1);
                return Ok(());
            }
            KTVMX_EXIT_SHUTDOWN => {
                eprintln!("[ktvmx_run] guest triple-faulted");
                exit(1)
            }
            KTVMX_EXIT_FAIL_ENTRY => {
                // SAFETY: fail_entry is the active union member for this exit.
                let r = unsafe { (*run).exit.fail_entry.hardware_entry_failure_reason };
                eprintln!("[ktvmx_run] VM entry failed, reason {r:#x}");
                exit(1)
            }
            other => {
                eprintln!("[ktvmx_run] unhandled exit reason {other}");
                exit(1)
            }
        }
    }
    eprintln!("[ktvmx_run] giving up after 1000000 exits");
    exit(1)
}
