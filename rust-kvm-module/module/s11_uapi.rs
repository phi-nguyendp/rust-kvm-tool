// SPDX-License-Identifier: GPL-2.0
//! Kernel side of stage 11: `/dev/ktvmx`, a KVM-shaped miscdevice. NOT COMPILED by the
//! course author (no Rust-enabled kernel was available): expect to fix small API
//! mismatches against ~/WS/linux_mainline/rust (miscdevice.rs, uaccess.rs, mm/virt.rs).
//!
//! DESIGN (simpler than KVM, deliberately): KVM has three fd levels (/dev/kvm -> VM fd ->
//! vCPU fd). Here one fd is all of them: opening /dev/ktvmx gives a fresh VM with one vCPU.
//! `KTVMX_CREATE_VM` and `KTVMX_CREATE_VCPU` therefore do not create anything new: they
//! check the order and return 0 (the userspace example `dup()`s the fd where KVM would
//! return a new one; a ~6 line patch for rust-kvm-tool, see stages/11-uapi.md).
//! Guest RAM is module-owned memory that `KTVMX_SET_USER_MEMORY_REGION` *copies into*
//! from userspace (KVM instead maps the user pages themselves; see "Think about").
//!
//! The ONE edit outside this stage's files: `Ktvmx` in module/ktvmx.rs must become an
//! `InPlaceModule` that owns the registration returned by `register()` below (the same
//! shape as samples/rust/rust_misc_device.rs).
//!
//! Book: none for the ABI; see Documentation/virt/kvm/api.rst (sections 4.1-4.17).
//! Kernel: virt/kvm/kvm_main.c (kvm_dev_ioctl, kvm_vm_ioctl, kvm_vcpu_ioctl,
//! kvm_vcpu_mmap), arch/x86/kvm/x86.c (kvm_arch_vcpu_ioctl_run, kvm_arch_vcpu_ioctl_*regs).

use crate::hw::PageBox;
use crate::vmx_uapi::*;
use core::mem::MaybeUninit;
use kernel::{
    fs::File,
    mm::virt::VmaNew,
    miscdevice::{MiscDevice, MiscDeviceOptions, MiscDeviceRegistration},
    new_mutex,
    page::Page,
    prelude::*,
    sync::Mutex,
    uaccess::{UserPtr, UserSlice, UserSliceReader, UserSliceWriter},
};

/// How much guest RAM this module is willing to back (passed to `memslot_check`).
pub(crate) const MAX_GUEST_RAM: u64 = 64 << 20;

// ---- the VM behind one open fd ------------------------------------------------------

/// Everything behind one open fd. Add the fields your stages 3-8 produced (the vcpu with
/// its VMCS and software GPRs, the EPT root) next to the ones below.
pub(crate) struct Vm {
    /// Guest RAM pages, page `i` backs guest-physical `ram_gpa + i * 4096`.
    pub(crate) ram: KVec<PageBox>,
    pub(crate) ram_gpa: u64,
    /// Shared with userspace via mmap: `ktvmx_run` (page 0) and the PIO data page.
    pub(crate) run_page: Page,
    pub(crate) pio_page: Page,
    /// The last exit was an `in`: on the next KTVMX_RUN copy the pio data into RAX
    /// first (kvm: `complete_userspace_io` / `complete_fast_pio_in`).
    pub(crate) pending_io_in: Option<u8>,
    // pub(crate) vcpu: YourVcpuVmx,
    // pub(crate) ept: YourEptRoot,
}

// The functions below are the module's internal API. `selftest()` and the ioctl handler
// call the same ones, so passing the selftest means the ioctls are wired correctly.
// They return `Err(ENOSYS)` until you write them (so an unfinished stage reports FAIL
// instead of panicking the kernel).

/// Create the VM and its vCPU: allocate run/pio pages (zeroed), build the VMCS with your
/// stage 3-5 code, set up an (empty) EPT (stage 8). (kernel: kvm_create_vm + kvm_vm_ioctl_create_vcpu)
pub(crate) fn ktvmx_vm_create() -> Result<Vm> {
    // TODO(stage 11)
    Err(ENOSYS)
}

/// Back guest-physical `[gpa, gpa + pages*4096)` with fresh RAM pages filled by
/// `fill(page_index, page)` (the ioctl reads them from userspace, the selftest from a
/// kernel buffer), and map each into the EPT (stage 8: read/write/execute, WB).
/// Replacing the region is allowed: free the old pages after unmapping + INVEPT.
/// (kernel: kvm_set_memory_region -> kvm_arch_prepare_memory_region)
pub(crate) fn ktvmx_set_memory(
    vm: &mut Vm,
    gpa: u64,
    pages: usize,
    fill: &mut dyn FnMut(usize, &mut [u8; 4096]) -> Result,
) -> Result {
    let _ = (vm, gpa, pages, fill);
    // TODO(stage 11)
    Err(ENOSYS)
}

/// `sregs_to_vmcs` + (stage 10) `vmx_set_cr0/vmx_set_cr4` so the hardware CR0/CR4 get
/// their fixed bits. The VMCS must be current while you write it (VMPTRLD).
pub(crate) fn ktvmx_set_sregs(vm: &mut Vm, sregs: &ktvmx_sregs) -> Result {
    let _ = (vm, sregs);
    Err(ENOSYS)
}
pub(crate) fn ktvmx_get_sregs(vm: &mut Vm) -> Result<ktvmx_sregs> {
    let _ = vm;
    Err(ENOSYS)
}
/// `regs_to_vmcs` with the vCPU's software GPR array.
pub(crate) fn ktvmx_set_regs(vm: &mut Vm, regs: &ktvmx_regs) -> Result {
    let _ = (vm, regs);
    Err(ENOSYS)
}
pub(crate) fn ktvmx_get_regs(vm: &mut Vm) -> Result<ktvmx_regs> {
    let _ = vm;
    Err(ENOSYS)
}

/// KTVMX_RUN: if `vm.pending_io_in` is set, complete that `in` from the pio page (low
/// `size` bytes of RAX), then loop { vmx_vcpu_run (stage 6); vmx_handle_exit (stage 7) }
/// until an exit that userspace must see: port I/O that is not the in-kernel serial
/// emulation (OUT: remember nothing; IN: set `pending_io_in`), HLT, triple fault, failed
/// entry, anything unhandled (`InternalError`). Interrupts: stage 9's exits are handled
/// inside the loop. Between iterations call `cond_resched()`-equivalent / check
/// signals so a guest spinning in a loop cannot hang the CPU forever (a pending signal =
/// return `Err(EINTR)`). (kernel: kvm_arch_vcpu_ioctl_run / vcpu_run / vcpu_enter_guest)
pub(crate) fn ktvmx_vcpu_run(vm: &mut Vm) -> Result<ktvmx_exit> {
    let _ = vm;
    // TODO(stage 11)
    Err(ENOSYS)
}

// ---- the misc device -----------------------------------------------------------------

#[pin_data]
pub(crate) struct KtvmxDev {}

/// Per-open-file state: the VM, created lazily by KTVMX_CREATE_VM.
#[pin_data]
pub(crate) struct KtvmxFile {
    #[pin]
    vm: Mutex<Option<Vm>>,
}

/// Registration helper for ktvmx.rs (see the module comment above).
pub(crate) fn register() -> impl PinInit<MiscDeviceRegistration<KtvmxDev>, Error> {
    let options = MiscDeviceOptions { name: c"ktvmx" };
    // SAFETY: the registration is stored in the module struct and dropped on unload;
    // `mem::forget()` is never called on it (same contract as rust_misc_device.rs).
    unsafe { MiscDeviceRegistration::register(options, try_pin_init!(KtvmxDev {})) }
}

/// Read a plain `#[repr(C)]` struct of integers from userspace (PROVIDED).
fn read_struct<T: Copy>(mut r: UserSliceReader) -> Result<T> {
    let mut v = MaybeUninit::<T>::zeroed();
    // SAFETY: viewing the zeroed storage as bytes; T is plain old data.
    let bytes = unsafe {
        core::slice::from_raw_parts_mut(v.as_mut_ptr() as *mut MaybeUninit<u8>, core::mem::size_of::<T>())
    };
    r.read_raw(bytes)?;
    // SAFETY: fully initialised by zeroed() + read_raw.
    Ok(unsafe { v.assume_init() })
}

/// Write a plain struct to userspace (PROVIDED).
fn write_struct<T: Copy>(mut w: UserSliceWriter, v: &T) -> Result {
    // SAFETY: T is plain old data; reading its bytes is fine.
    let bytes =
        unsafe { core::slice::from_raw_parts(v as *const T as *const u8, core::mem::size_of::<T>()) };
    w.write_slice(bytes)
}

#[vtable]
impl MiscDevice for KtvmxDev {
    type File = Pin<KBox<KtvmxFile>>;

    fn open(_file: &File, _misc: &MiscDeviceRegistration<Self>) -> Result<Self::File> {
        KBox::try_pin_init(try_pin_init!(KtvmxFile { vm <- new_mutex!(None) }), GFP_KERNEL)
    }

    /// The ioctl dispatcher. PROVIDED, except the two `todo!()`s: you decide how the
    /// run page is refreshed after KTVMX_RUN.
    fn ioctl(me: Pin<&KtvmxFile>, _file: &File, cmd: u32, arg: usize) -> Result<isize> {
        let uarg = UserPtr::from_addr(arg);
        let size = ioc_size(cmd);
        let mut guard = me.vm.lock();

        // Only these three work before KTVMX_CREATE_VM.
        match cmd {
            KTVMX_GET_API_VERSION => return Ok(KTVMX_API_VERSION as isize),
            KTVMX_GET_VCPU_MMAP_SIZE => return Ok(KTVMX_VCPU_MMAP_SIZE as isize),
            KTVMX_CREATE_VM => {
                if guard.is_some() {
                    return Err(EEXIST);
                }
                *guard = Some(ktvmx_vm_create()?);
                return Ok(0);
            }
            _ => {}
        }
        let vm = guard.as_mut().ok_or(EINVAL)?;
        match cmd {
            // single vCPU, already created together with the VM
            KTVMX_CREATE_VCPU => Ok(0),
            KTVMX_SET_USER_MEMORY_REGION => {
                let r: ktvmx_userspace_memory_region =
                    read_struct(UserSlice::new(uarg, size).reader())?;
                memslot_check(&r, MAX_GUEST_RAM).map_err(|e| Error::from_errno(-e))?;
                let (gpa, pages, uaddr) =
                    (r.guest_phys_addr, (r.memory_size / 4096) as usize, r.userspace_addr as usize);
                ktvmx_set_memory(vm, gpa, pages, &mut |i, page| {
                    UserSlice::new(UserPtr::from_addr(uaddr + i * 4096), 4096)
                        .reader()
                        .read_slice(page)
                })?;
                Ok(0)
            }
            KTVMX_SET_REGS => {
                ktvmx_set_regs(vm, &read_struct(UserSlice::new(uarg, size).reader())?)?;
                Ok(0)
            }
            KTVMX_GET_REGS => {
                write_struct(UserSlice::new(uarg, size).writer(), &ktvmx_get_regs(vm)?)?;
                Ok(0)
            }
            KTVMX_SET_SREGS => {
                ktvmx_set_sregs(vm, &read_struct(UserSlice::new(uarg, size).reader())?)?;
                Ok(0)
            }
            KTVMX_GET_SREGS => {
                write_struct(UserSlice::new(uarg, size).writer(), &ktvmx_get_sregs(vm)?)?;
                Ok(0)
            }
            KTVMX_RUN => {
                let exit = ktvmx_vcpu_run(vm)?;
                let _ = exit;
                // TODO(stage 11): serialise `exit` with `exit_to_run` into a local
                // `ktvmx_run` + `[u8; 4096]`, then copy both into `vm.run_page` /
                // `vm.pio_page` (`Page::write_raw`, unsafe: offset+len within the page).
                // Keep the fields userspace owns (`immediate_exit`, ...) as they were.
                todo!("stage 11: publish the exit in the shared run page")
            }
            _ => Err(ENOTTY),
        }
    }

    /// mmap of the fd = the vCPU's run area: page 0 `ktvmx_run`, page 1 PIO data
    /// (`KTVMX_VCPU_MMAP_SIZE` bytes, offset 0 only). Hint: `vma.set_mixedmap()` returns a
    /// `&VmaMixedMap`; `vm_insert_page(vma.start(), &page)` maps one `kernel::page::Page`
    /// (that is why `run_page`/`pio_page` are `Page`s and not `PageBox`es). Check
    /// `vma.end() - vma.start() == KTVMX_VCPU_MMAP_SIZE` and `vma.pgoff() == 0`, else EINVAL.
    /// (kernel: kvm_vcpu_mmap / kvm_vcpu_fault)
    fn mmap(
        me: Pin<&KtvmxFile>,
        _file: &File,
        vma: &VmaNew,
    ) -> Result {
        let _ = (me, vma);
        // TODO(stage 11)
        Err(ENOSYS)
    }
}

// ---- selftest --------------------------------------------------------------------------

// guest/07_hello.S, assembled (`make -C guest 07_hello.rs`): prints "Hello from the guest\n".
const HELLO: [u8; 40] = [
    0xfc, 0xbe, 0x12, 0x10, 0xba, 0xf8, 0x03, 0xac, 0x84, 0xc0, 0x74, 0x03, 0xee, 0xeb, 0xf8,
    0xf4, 0xeb, 0xfd, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x66, 0x72, 0x6f, 0x6d, 0x20, 0x74,
    0x68, 0x65, 0x20, 0x67, 0x75, 0x65, 0x73, 0x74, 0x0a, 0x00,
];
const EXPECTED: &[u8] = b"Hello from the guest\n";

/// Real-mode state for a flat 16-bit payload (what examples/ktvmx_run.rs sends).
fn real_mode_sregs() -> ktvmx_sregs {
    let mut s = ktvmx_sregs::default();
    let data = |sel| ar_to_segment(sel, 0, 0xffff, 0x93);
    s.ds = data(0);
    s.es = data(0);
    s.fs = data(0);
    s.gs = data(0);
    s.ss = data(0);
    s.cs = ar_to_segment(0, 0, 0xffff, 0x9b);
    s.tr = ar_to_segment(0, 0, 0xffff, 0x8b);
    s.ldt = ar_to_segment(0, 0, 0xffff, 0x82);
    s.idt = ktvmx_dtable { base: 0, limit: 0x3ff, padding: [0; 3] };
    s.cr0 = 0; // real mode; stage 10's vmx_set_cr0 adds NE
    s
}

/// Runs the hello payload through exactly the internal functions the ioctls use.
pub(crate) fn selftest() -> core::result::Result<(), &'static str> {
    let mut vm = ktvmx_vm_create().map_err(|_| "ktvmx_vm_create failed (not written yet?)")?;
    ktvmx_set_memory(&mut vm, 0, 16, &mut |i, page| {
        if i == 1 {
            // GPA 0x1000
            page[..HELLO.len()].copy_from_slice(&HELLO);
        }
        Ok(())
    })
    .map_err(|_| "ktvmx_set_memory failed")?;
    ktvmx_set_sregs(&mut vm, &real_mode_sregs()).map_err(|_| "set_sregs failed")?;
    let regs = ktvmx_regs { rip: 0x1000, rflags: 2, ..Default::default() };
    ktvmx_set_regs(&mut vm, &regs).map_err(|_| "set_regs failed")?;

    let mut got = [0u8; 64];
    let mut n = 0;
    for _ in 0..100 {
        match ktvmx_vcpu_run(&mut vm).map_err(|_| "KTVMX_RUN failed")? {
            ktvmx_exit::Io { is_out: true, port: 0x3f8, data, .. } if n < got.len() => {
                got[n] = data[0];
                n += 1;
            }
            ktvmx_exit::Hlt => {
                return if &got[..n] == EXPECTED {
                    Ok(())
                } else {
                    Err("guest output differs from \"Hello from the guest\\n\"")
                };
            }
            _ => return Err("unexpected exit"),
        }
    }
    Err("guest did not halt")
}
