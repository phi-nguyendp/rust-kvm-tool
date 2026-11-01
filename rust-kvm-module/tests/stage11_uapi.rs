use core::mem::{offset_of, size_of};
use ktvmx::mock::MockVmcs;
use ktvmx::vmcs_ops::VmcsOps;
use ktvmx::vmx::*;
use ktvmx::vmx_uapi::*;

// ---- ABI layout (provided structs: these pass from the start, they protect you from
// ---- editing them. The real KVM sizes are what rust-kvm-tool was written against.)

#[test]
fn struct_sizes_match_kvm() {
    assert_eq!(size_of::<ktvmx_regs>(), 144);
    assert_eq!(size_of::<ktvmx_segment>(), 24);
    assert_eq!(size_of::<ktvmx_dtable>(), 16);
    assert_eq!(size_of::<ktvmx_sregs>(), 312);
    assert_eq!(size_of::<ktvmx_userspace_memory_region>(), 32);
    assert_eq!(size_of::<ktvmx_run_exit>(), 256);
    assert_eq!(size_of::<ktvmx_run>(), 304);
}

#[test]
fn struct_offsets_match_kvm() {
    assert_eq!(offset_of!(ktvmx_regs, rip), 128);
    assert_eq!(offset_of!(ktvmx_segment, selector), 12);
    assert_eq!(offset_of!(ktvmx_segment, type_), 14);
    assert_eq!(offset_of!(ktvmx_segment, unusable), 22);
    assert_eq!(offset_of!(ktvmx_sregs, gdt), 192);
    assert_eq!(offset_of!(ktvmx_sregs, cr0), 224);
    assert_eq!(offset_of!(ktvmx_sregs, efer), 264);
    assert_eq!(offset_of!(ktvmx_sregs, interrupt_bitmap), 280);
    assert_eq!(offset_of!(ktvmx_userspace_memory_region, userspace_addr), 24);
    assert_eq!(offset_of!(ktvmx_run, exit_reason), 8);
    assert_eq!(offset_of!(ktvmx_run, cr8), 16);
    assert_eq!(offset_of!(ktvmx_run, exit), 32);
    assert_eq!(offset_of!(ktvmx_exit_io, port), 2);
    assert_eq!(offset_of!(ktvmx_exit_io, count), 4);
    assert_eq!(offset_of!(ktvmx_exit_io, data_offset), 8);
}

// ---- 1. ioctl numbers -----------------------------------------------------------

#[test]
fn ioc_reproduces_every_kvm_number() {
    let t = KTVMX_IOCTL_TYPE;
    assert_eq!(ioc(IOC_NONE, t, 0x00, 0), KTVMX_GET_API_VERSION);
    assert_eq!(ioc(IOC_NONE, t, 0x01, 0), KTVMX_CREATE_VM);
    assert_eq!(ioc(IOC_NONE, t, 0x04, 0), KTVMX_GET_VCPU_MMAP_SIZE);
    assert_eq!(ioc(IOC_NONE, t, 0x41, 0), KTVMX_CREATE_VCPU);
    assert_eq!(ioc(IOC_NONE, t, 0x80, 0), KTVMX_RUN);
    assert_eq!(
        ioc(IOC_WRITE, t, 0x46, size_of::<ktvmx_userspace_memory_region>()),
        KTVMX_SET_USER_MEMORY_REGION
    );
    assert_eq!(ioc(IOC_READ, t, 0x81, size_of::<ktvmx_regs>()), KTVMX_GET_REGS);
    assert_eq!(ioc(IOC_WRITE, t, 0x82, size_of::<ktvmx_regs>()), KTVMX_SET_REGS);
    assert_eq!(ioc(IOC_READ, t, 0x83, size_of::<ktvmx_sregs>()), KTVMX_GET_SREGS);
    assert_eq!(ioc(IOC_WRITE, t, 0x84, size_of::<ktvmx_sregs>()), KTVMX_SET_SREGS);
}

#[test]
fn ioc_decoding() {
    let c = KTVMX_SET_SREGS;
    assert_eq!(ioc_dir(c), IOC_WRITE);
    assert_eq!(ioc_type(c), 0xae);
    assert_eq!(ioc_nr(c), 0x84);
    assert_eq!(ioc_size(c), 312);
    assert_eq!(ioc_dir(KTVMX_GET_REGS), IOC_READ);
    assert_eq!(ioc_size(KTVMX_GET_REGS), 144);
    assert_eq!(ioc_dir(KTVMX_RUN), IOC_NONE);
    assert_eq!(ioc_size(KTVMX_RUN), 0);
}

#[test]
fn ioc_roundtrip_for_all_fields() {
    for (dir, ty, nr, size) in [(1, 0x7f, 0xff, 0x3fff), (2, 0, 0, 1), (0, 0xff, 0x55, 0), (3, 0xab, 0xcd, 0x1234)] {
        let c = ioc(dir, ty, nr, size);
        assert_eq!((ioc_dir(c), ioc_type(c), ioc_nr(c), ioc_size(c)), (dir, ty, nr, size));
    }
}

// ---- 2. segments ----------------------------------------------------------------

fn seg(ty: u8, s: u8, dpl: u8, p: u8, avl: u8, l: u8, db: u8, g: u8) -> ktvmx_segment {
    ktvmx_segment {
        base: 0, limit: 0xffff_ffff, selector: 8, type_: ty, present: p, dpl, db, s, l, g, avl,
        unusable: 0, padding: 0,
    }
}

#[test]
fn segment_ar_for_the_usual_descriptors() {
    // 64-bit code: type 0xb, S, P, L, G
    assert_eq!(segment_to_ar(&seg(0xb, 1, 0, 1, 0, 1, 0, 1)), 0xa09b);
    // 32-bit flat code / data
    assert_eq!(segment_to_ar(&seg(0xb, 1, 0, 1, 0, 0, 1, 1)), 0xc09b);
    assert_eq!(segment_to_ar(&seg(0x3, 1, 0, 1, 0, 0, 1, 1)), 0xc093);
    // real mode: byte granular 16-bit
    assert_eq!(segment_to_ar(&seg(0x3, 1, 0, 1, 0, 0, 0, 0)), 0x93);
    assert_eq!(segment_to_ar(&seg(0xb, 1, 0, 1, 0, 0, 0, 0)), 0x9b);
    // busy 64-bit TSS: S = 0
    assert_eq!(segment_to_ar(&seg(0xb, 0, 0, 1, 0, 0, 0, 0)), 0x8b);
    // DPL 3 and AVL
    assert_eq!(segment_to_ar(&seg(0x3, 1, 3, 1, 1, 0, 0, 0)), 0x10f3);
}

#[test]
fn unusable_or_absent_sets_bit_16() {
    let mut s = seg(0x3, 1, 0, 1, 0, 0, 0, 0);
    s.unusable = 1;
    assert_ne!(segment_to_ar(&s) & (1 << 16), 0);
    let n = seg(0x3, 1, 0, 0, 0, 0, 0, 0); // not present
    assert_ne!(segment_to_ar(&n) & (1 << 16), 0);
    assert_eq!(segment_to_ar(&seg(0x3, 1, 0, 1, 0, 0, 0, 0)) & (1 << 16), 0);
}

#[test]
fn ar_to_segment_decodes_every_field() {
    let s = ar_to_segment(0x10, 0xdead_0000, 0xfffff, 0xa09b | (3 << 5));
    assert_eq!(s.selector, 0x10);
    assert_eq!(s.base, 0xdead_0000);
    assert_eq!(s.limit, 0xfffff);
    assert_eq!((s.type_, s.s, s.dpl, s.present), (0xb, 1, 3, 1));
    assert_eq!((s.avl, s.l, s.db, s.g), (0, 1, 0, 1));
    assert_eq!(s.unusable, 0);
    let u = ar_to_segment(0, 0, 0, 1 << 16);
    assert_eq!((u.unusable, u.present), (1, 0));
}

#[test]
fn segment_roundtrip() {
    for ar in [0x93u32, 0x9b, 0xc093, 0xc09b, 0xa09b, 0x8b, 0x82, 0x10f3, 0xc0fb] {
        let s = ar_to_segment(8, 0x1000, 0xffff, ar);
        assert_eq!(segment_to_ar(&s), ar, "ar {ar:#x}");
    }
}

// ---- 3. sregs <-> VMCS ------------------------------------------------------------

fn sample_sregs() -> ktvmx_sregs {
    let mut s = ktvmx_sregs::default();
    s.cs = ar_to_segment(0x08, 0, 0xffff_ffff, 0xa09b);
    s.ds = ar_to_segment(0x10, 0, 0xffff_ffff, 0xc093);
    s.es = ar_to_segment(0x11, 0x11000, 0xffff, 0x93);
    s.fs = ar_to_segment(0x12, 0xffff_8000_0000_1000, 0xffff_ffff, 0xc093);
    s.gs = ar_to_segment(0x13, 0xffff_8000_0000_2000, 0xffff_ffff, 0xc093);
    s.ss = ar_to_segment(0x14, 0, 0xffff_ffff, 0xc093);
    s.tr = ar_to_segment(0x18, 0xffff_d000, 0x67, 0x8b);
    s.ldt = ar_to_segment(0, 0, 0xffff, 0x10000);
    s.gdt = ktvmx_dtable { base: 0x500, limit: 0x17, padding: [0; 3] };
    s.idt = ktvmx_dtable { base: 0x600, limit: 0xfff, padding: [0; 3] };
    s.cr0 = X86_CR0_PE | X86_CR0_PG | X86_CR0_NE;
    s.cr3 = 0x2000;
    s.cr4 = X86_CR4_PAE;
    s.efer = EFER_LME | EFER_LMA;
    s
}

#[test]
fn sregs_land_in_the_right_vmcs_fields() {
    let mut v = MockVmcs::default();
    let s = sample_sregs();
    sregs_to_vmcs(&s, &mut v);
    assert_eq!(v.vmcs_readl(GUEST_CS_SELECTOR), 0x08);
    assert_eq!(v.vmcs_readl(GUEST_CS_AR_BYTES), 0xa09b);
    assert_eq!(v.vmcs_readl(GUEST_CS_LIMIT), 0xffff_ffff);
    assert_eq!(v.vmcs_readl(GUEST_ES_BASE), 0x11000);
    assert_eq!(v.vmcs_readl(GUEST_FS_BASE), 0xffff_8000_0000_1000);
    assert_eq!(v.vmcs_readl(GUEST_GS_BASE), 0xffff_8000_0000_2000);
    assert_eq!(v.vmcs_readl(GUEST_SS_SELECTOR), 0x14);
    assert_eq!(v.vmcs_readl(GUEST_DS_AR_BYTES), 0xc093);
    assert_eq!(v.vmcs_readl(GUEST_TR_BASE), 0xffff_d000);
    assert_eq!(v.vmcs_readl(GUEST_TR_AR_BYTES), 0x8b);
    assert_eq!(v.vmcs_readl(GUEST_TR_LIMIT), 0x67);
    assert_eq!(v.vmcs_readl(GUEST_LDTR_AR_BYTES), 0x10000);
    assert_eq!(v.vmcs_readl(GUEST_GDTR_BASE), 0x500);
    assert_eq!(v.vmcs_readl(GUEST_GDTR_LIMIT), 0x17);
    assert_eq!(v.vmcs_readl(GUEST_IDTR_BASE), 0x600);
    assert_eq!(v.vmcs_readl(GUEST_IDTR_LIMIT), 0xfff);
    assert_eq!(v.vmcs_readl(GUEST_CR0), s.cr0);
    assert_eq!(v.vmcs_readl(GUEST_CR3), 0x2000);
    assert_eq!(v.vmcs_readl(GUEST_CR4), X86_CR4_PAE);
    assert_eq!(v.vmcs_readl(GUEST_IA32_EFER), EFER_LME | EFER_LMA);
}

#[test]
fn sregs_roundtrip_through_the_vmcs() {
    let mut v = MockVmcs::default();
    let s = sample_sregs();
    sregs_to_vmcs(&s, &mut v);
    assert_eq!(vmcs_to_sregs(&v), s);
}

#[test]
fn non_vmcs_sregs_are_dropped() {
    let mut v = MockVmcs::default();
    let mut s = sample_sregs();
    s.cr2 = 0xdead;
    s.cr8 = 3;
    s.apic_base = 0xfee0_0900;
    s.interrupt_bitmap = [1, 2, 3, 4];
    sregs_to_vmcs(&s, &mut v);
    let back = vmcs_to_sregs(&v);
    assert_eq!((back.cr2, back.cr8, back.apic_base, back.interrupt_bitmap), (0, 0, 0, [0; 4]));
    assert_eq!(back.cr3, s.cr3);
}

#[test]
fn real_mode_start_state() {
    // what rust-kvm-tool-style userspace does for a flat 16-bit payload: CS base 0.
    let mut s = ktvmx_sregs::default();
    for g in [&mut s.cs, &mut s.ds, &mut s.es, &mut s.fs, &mut s.gs, &mut s.ss] {
        *g = ar_to_segment(0, 0, 0xffff, 0x93);
    }
    s.cs = ar_to_segment(0, 0, 0xffff, 0x9b);
    s.tr = ar_to_segment(0, 0, 0xffff, 0x8b);
    s.ldt = ar_to_segment(0, 0, 0xffff, 0x82);
    let mut v = MockVmcs::default();
    sregs_to_vmcs(&s, &mut v);
    assert_eq!(v.vmcs_readl(GUEST_CS_AR_BYTES), 0x9b);
    assert_eq!(v.vmcs_readl(GUEST_CR0), 0, "no PE: the caller adds NE via stage 10");
    assert_eq!(vmcs_to_sregs(&v), s);
}

// ---- 4. regs ----------------------------------------------------------------------

fn sample_regs() -> ktvmx_regs {
    ktvmx_regs {
        rax: 1, rbx: 2, rcx: 3, rdx: 4, rsi: 5, rdi: 6, rsp: 0x7000, rbp: 8, r8: 9, r9: 10,
        r10: 11, r11: 12, r12: 13, r13: 14, r14: 15, r15: 16, rip: 0x1000, rflags: 0x202,
    }
}

#[test]
fn regs_split_between_vmcs_and_gprs() {
    let mut v = MockVmcs::default();
    let mut g = [0u64; 16];
    regs_to_vmcs(&sample_regs(), &mut v, &mut g);
    assert_eq!(v.vmcs_readl(GUEST_RIP), 0x1000);
    assert_eq!(v.vmcs_readl(GUEST_RSP), 0x7000);
    assert_eq!(v.vmcs_readl(GUEST_RFLAGS), 0x202);
    // x86 encoding order: RAX RCX RDX RBX RSP RBP RSI RDI R8..R15
    assert_eq!(&g[..4], &[1, 3, 4, 2]);
    assert_eq!(&g[5..8], &[8, 5, 6]);
    assert_eq!(&g[8..], &[9, 10, 11, 12, 13, 14, 15, 16]);
}

#[test]
fn rflags_bit1_is_forced() {
    let mut v = MockVmcs::default();
    let mut g = [0u64; 16];
    let mut r = sample_regs();
    r.rflags = 0; // userspace zero-initialised the struct
    regs_to_vmcs(&r, &mut v, &mut g);
    assert_eq!(v.vmcs_readl(GUEST_RFLAGS), X86_EFLAGS_FIXED);
}

#[test]
fn regs_roundtrip() {
    let mut v = MockVmcs::default();
    let mut g = [0u64; 16];
    let r = sample_regs();
    regs_to_vmcs(&r, &mut v, &mut g);
    assert_eq!(vmcs_to_regs(&v, &g), r);
}

// ---- 5. memory region -------------------------------------------------------------

fn region(slot: u32, flags: u32, gpa: u64, size: u64, uaddr: u64) -> ktvmx_userspace_memory_region {
    ktvmx_userspace_memory_region { slot, flags, guest_phys_addr: gpa, memory_size: size, userspace_addr: uaddr }
}

const MAX: u64 = 64 << 20;

#[test]
fn memslot_accepts_a_good_region() {
    assert_eq!(memslot_check(&region(0, 0, 0, 2 << 20, 0x7f00_0000_0000), MAX), Ok(()));
    assert_eq!(memslot_check(&region(0, 0, 0x10_0000, MAX, 0x1000), MAX), Ok(()));
}

#[test]
fn memslot_rejects_bad_requests() {
    assert_eq!(memslot_check(&region(1, 0, 0, 0x1000, 0x1000), MAX), Err(EINVAL), "only slot 0");
    assert_eq!(memslot_check(&region(0, 1, 0, 0x1000, 0x1000), MAX), Err(EINVAL), "no flags");
    assert_eq!(memslot_check(&region(0, 0, 0x800, 0x1000, 0x1000), MAX), Err(EINVAL), "gpa alignment");
    assert_eq!(memslot_check(&region(0, 0, 0, 0x1800, 0x1000), MAX), Err(EINVAL), "size alignment");
    assert_eq!(memslot_check(&region(0, 0, 0, 0x1000, 0x1234), MAX), Err(EINVAL), "uaddr alignment");
    assert_eq!(memslot_check(&region(0, 0, 0, 0, 0x1000), MAX), Err(EINVAL), "empty");
    assert_eq!(
        memslot_check(&region(0, 0, u64::MAX - 0xfff, 0x2000, 0x1000), MAX),
        Err(EINVAL),
        "gpa + size wraps"
    );
}

#[test]
fn memslot_too_big_is_enomem() {
    assert_eq!(memslot_check(&region(0, 0, 0, MAX + 0x1000, 0x1000), MAX), Err(ENOMEM));
}

// ---- 6. exits -------------------------------------------------------------------------

#[test]
fn io_out_exit() {
    let mut run = ktvmx_run::zeroed();
    let mut pio = [0u8; 4096];
    let e = ktvmx_exit::Io { is_out: true, size: 1, port: 0x3f8, count: 1, data: [b'H', 0, 0, 0] };
    exit_to_run(&e, &mut run, &mut pio);
    assert_eq!(run.exit_reason, KTVMX_EXIT_IO);
    let io = unsafe { run.exit.io };
    assert_eq!(io.direction, KTVMX_EXIT_IO_OUT);
    assert_eq!((io.size, io.port, io.count), (1, 0x3f8, 1));
    assert_eq!(io.data_offset, KTVMX_PIO_DATA_OFFSET);
    assert_eq!(pio[0], b'H');
    assert_eq!(pio[1], 0);
}

#[test]
fn io_in_exit_leaves_the_buffer() {
    let mut run = ktvmx_run::zeroed();
    let mut pio = [0xaau8; 4096];
    let e = ktvmx_exit::Io { is_out: false, size: 2, port: 0x60, count: 1, data: [1, 2, 0, 0] };
    exit_to_run(&e, &mut run, &mut pio);
    let io = unsafe { run.exit.io };
    assert_eq!(io.direction, KTVMX_EXIT_IO_IN);
    assert_eq!((io.size, io.port), (2, 0x60));
    assert!(pio.iter().all(|&b| b == 0xaa));
}

#[test]
fn io_out_wide_data() {
    let mut run = ktvmx_run::zeroed();
    let mut pio = [0u8; 4096];
    let e = ktvmx_exit::Io { is_out: true, size: 4, port: 0xcf8, count: 1, data: [0x78, 0x56, 0x34, 0x12] };
    exit_to_run(&e, &mut run, &mut pio);
    assert_eq!(&pio[..4], &[0x78, 0x56, 0x34, 0x12]);
}

#[test]
fn simple_exits() {
    let mut pio = [0u8; 4096];
    let mut run = ktvmx_run::zeroed();
    exit_to_run(&ktvmx_exit::Hlt, &mut run, &mut pio);
    assert_eq!(run.exit_reason, KTVMX_EXIT_HLT);
    exit_to_run(&ktvmx_exit::Shutdown, &mut run, &mut pio);
    assert_eq!(run.exit_reason, KTVMX_EXIT_SHUTDOWN);
    exit_to_run(&ktvmx_exit::FailEntry { reason: 0x8000_0021 }, &mut run, &mut pio);
    assert_eq!(run.exit_reason, KTVMX_EXIT_FAIL_ENTRY);
    assert_eq!(unsafe { run.exit.fail_entry.hardware_entry_failure_reason }, 0x8000_0021);
    exit_to_run(&ktvmx_exit::InternalError { suberror: 1 }, &mut run, &mut pio);
    assert_eq!(run.exit_reason, KTVMX_EXIT_INTERNAL_ERROR);
    let i = unsafe { run.exit.internal };
    assert_eq!((i.suberror, i.ndata), (1, 0));
}

#[test]
fn mmio_exit() {
    let mut pio = [0u8; 4096];
    let mut run = ktvmx_run::zeroed();
    let e = ktvmx_exit::Mmio { gpa: 0xfee0_00b0, is_write: true, len: 4, data: [1, 0, 0, 0, 0, 0, 0, 0] };
    exit_to_run(&e, &mut run, &mut pio);
    assert_eq!(run.exit_reason, KTVMX_EXIT_MMIO);
    let m = unsafe { run.exit.mmio };
    assert_eq!((m.phys_addr, m.len, m.is_write), (0xfee0_00b0, 4, 1));
    assert_eq!(m.data[0], 1);
}

#[test]
fn a_new_exit_clears_the_previous_union() {
    let mut pio = [0u8; 4096];
    let mut run = ktvmx_run::zeroed();
    exit_to_run(&ktvmx_exit::InternalError { suberror: 7 }, &mut run, &mut pio);
    exit_to_run(&ktvmx_exit::Hlt, &mut run, &mut pio);
    let pad = unsafe { run.exit.padding };
    assert!(pad.iter().all(|&b| b == 0), "stale exit data leaked to userspace");
}
