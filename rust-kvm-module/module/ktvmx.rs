// SPDX-License-Identifier: GPL-2.0
//! ktvmx: a toy VMX hypervisor, written as a Rust Linux module.
//!
//! Layout: the hardware-independent logic is shared with the host-side crate
//! (`../src/*.rs`, `no_std`, unit-tested by `cargo test`); `module/sNN_*.rs` hold the
//! kernel-only parts (inline asm, devices) and a `selftest()` per stage that is run at
//! `insmod ktvmx.ko selftest=N` and logs `ktvmx: stageNN <name>: PASS|FAIL (why)`.
//! See ../COURSE.md and ../docs/setup.md.

use kernel::prelude::*;

// Shared pure-logic files. Same module names as in lib.rs so `crate::vmx::*` resolves
// in both builds.
#[path = "../src/vmx.rs"]
mod vmx;
#[path = "../src/vmcs_ops.rs"]
mod vmcs_ops;
#[path = "../src/vmx_caps.rs"]
mod vmx_caps;
#[path = "../src/vmx_enable.rs"]
mod vmx_enable;
#[path = "../src/vmcs_fields.rs"]
mod vmcs_fields;
#[path = "../src/vmx_controls.rs"]
mod vmx_controls;
#[path = "../src/vmx_state.rs"]
mod vmx_state;
#[path = "../src/vmx_run.rs"]
mod vmx_run;
#[path = "../src/vmx_exit.rs"]
mod vmx_exit;
#[path = "../src/vmx_ept.rs"]
mod vmx_ept;
#[path = "../src/vmx_events.rs"]
mod vmx_events;
#[path = "../src/vmx_bitmaps.rs"]
mod vmx_bitmaps;
#[path = "../src/vmx_uapi.rs"]
mod vmx_uapi;

mod hw;
mod s01_caps;
mod s02_vmxon;
mod s03_vmcs;
mod s04_controls;
mod s05_state;
mod s06_run;
mod s07_exit;
mod s08_ept;
mod s09_events;
mod s10_bitmaps;
mod s11_uapi;

module! {
    type: Ktvmx,
    name: "ktvmx",
    authors: ["you"],
    description: "Learning VMX hypervisor in Rust",
    license: "GPL",
    params: {
        selftest: i64 {
            default: 0,
            description: "run the in-kernel self tests of stages 1..=N at load time",
        },
    },
}

/// (stage number, name, test).
const STAGES: &[(i64, &str, fn() -> core::result::Result<(), &'static str>)] = &[
    (1, "caps", s01_caps::selftest),
    (2, "vmxon", s02_vmxon::selftest),
    (3, "vmcs", s03_vmcs::selftest),
    (4, "controls", s04_controls::selftest),
    (5, "state", s05_state::selftest),
    (6, "run", s06_run::selftest),
    (7, "exit", s07_exit::selftest),
    (8, "ept", s08_ept::selftest),
    (9, "events", s09_events::selftest),
    (10, "bitmaps", s10_bitmaps::selftest),
    (11, "uapi", s11_uapi::selftest),
];

struct Ktvmx;

impl kernel::Module for Ktvmx {
    fn init(_module: &'static ThisModule) -> Result<Self> {
        pr_info!("ktvmx: loaded\n");
        let upto = *module_parameters::selftest.value();
        for (n, name, test) in STAGES {
            if *n > upto {
                break;
            }
            match test() {
                Ok(()) => pr_info!("ktvmx: stage{:02} {}: PASS\n", n, name),
                Err(why) => {
                    pr_err!("ktvmx: stage{:02} {}: FAIL ({})\n", n, name, why);
                    // Stop at the first failing stage, later ones depend on it.
                    break;
                }
            }
        }
        Ok(Ktvmx)
    }
}

impl Drop for Ktvmx {
    fn drop(&mut self) {
        pr_info!("ktvmx: unloaded\n");
    }
}
