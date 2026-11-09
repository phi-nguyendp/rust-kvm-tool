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
// in both builds. Their items are `pub` API under cargo but private here, so most look
// unused to rustc until a later stage needs them: hence the dead_code allows.
#[path = "../src/vmx.rs"]
#[allow(dead_code, unreachable_pub, non_camel_case_types)]
mod vmx;
#[path = "../src/vmcs_ops.rs"]
#[allow(dead_code, unreachable_pub, non_camel_case_types)]
mod vmcs_ops;
#[path = "../src/vmx_caps.rs"]
#[allow(dead_code, unreachable_pub, non_camel_case_types)]
mod vmx_caps;
#[path = "../src/vmx_enable.rs"]
#[allow(dead_code, unreachable_pub, non_camel_case_types)]
mod vmx_enable;
#[path = "../src/vmcs_fields.rs"]
#[allow(dead_code, unreachable_pub, non_camel_case_types)]
mod vmcs_fields;
#[path = "../src/vmx_controls.rs"]
#[allow(dead_code, unreachable_pub, non_camel_case_types)]
mod vmx_controls;
#[path = "../src/vmx_state.rs"]
#[allow(dead_code, unreachable_pub, non_camel_case_types)]
mod vmx_state;
#[path = "../src/vmx_run.rs"]
#[allow(dead_code, unreachable_pub, non_camel_case_types)]
mod vmx_run;
#[path = "../src/vmx_exit.rs"]
#[allow(dead_code, unreachable_pub, non_camel_case_types)]
mod vmx_exit;
#[path = "../src/vmx_ept.rs"]
#[allow(dead_code, unreachable_pub, non_camel_case_types)]
mod vmx_ept;
#[path = "../src/vmx_events.rs"]
#[allow(dead_code, unreachable_pub, non_camel_case_types)]
mod vmx_events;
#[path = "../src/vmx_bitmaps.rs"]
#[allow(dead_code, unreachable_pub, non_camel_case_types)]
mod vmx_bitmaps;
#[path = "../src/vmx_uapi.rs"]
#[allow(dead_code, unreachable_pub, non_camel_case_types)]
mod vmx_uapi;

#[allow(dead_code, unreachable_pub)]
mod hw;
// Kernel halves are compiled only up to `make -C module STAGE=N` (default: all),
// which passes `--cfg ktvmx_s01 .. --cfg ktvmx_sNN` (see Kbuild). The shared
// `../src` files above are always compiled, like `cargo build` does.
#[cfg(ktvmx_s01)]
mod s01_caps;
#[cfg(ktvmx_s02)]
mod s02_vmxon;
#[cfg(ktvmx_s03)]
mod s03_vmcs;
#[cfg(ktvmx_s04)]
mod s04_controls;
#[cfg(ktvmx_s05)]
mod s05_state;
#[cfg(ktvmx_s06)]
mod s06_run;
#[cfg(ktvmx_s07)]
mod s07_exit;
#[cfg(ktvmx_s08)]
mod s08_ept;
#[cfg(ktvmx_s09)]
mod s09_events;
#[cfg(ktvmx_s10)]
mod s10_bitmaps;
#[cfg(ktvmx_s11)]
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

/// Stand-in selftest for a stage left out of the build by `STAGE=N`.
#[allow(dead_code)]
fn not_built() -> core::result::Result<(), &'static str> {
    Err("not built, rebuild with make -C module STAGE=<n>")
}

/// `selftest` of stage module `$m` if its `--cfg` is set, else `not_built`.
macro_rules! stage_test {
    ($cfg:ident, $m:ident) => {{
        #[cfg($cfg)]
        let f: fn() -> core::result::Result<(), &'static str> = $m::selftest;
        #[cfg(not($cfg))]
        let f: fn() -> core::result::Result<(), &'static str> = not_built;
        f
    }};
}

/// (stage number, name, test).
const STAGES: &[(i64, &str, fn() -> core::result::Result<(), &'static str>)] = &[
    (1, "caps", stage_test!(ktvmx_s01, s01_caps)),
    (2, "vmxon", stage_test!(ktvmx_s02, s02_vmxon)),
    (3, "vmcs", stage_test!(ktvmx_s03, s03_vmcs)),
    (4, "controls", stage_test!(ktvmx_s04, s04_controls)),
    (5, "state", stage_test!(ktvmx_s05, s05_state)),
    (6, "run", stage_test!(ktvmx_s06, s06_run)),
    (7, "exit", stage_test!(ktvmx_s07, s07_exit)),
    (8, "ept", stage_test!(ktvmx_s08, s08_ept)),
    (9, "events", stage_test!(ktvmx_s09, s09_events)),
    (10, "bitmaps", stage_test!(ktvmx_s10, s10_bitmaps)),
    (11, "uapi", stage_test!(ktvmx_s11, s11_uapi)),
];

struct Ktvmx;

impl kernel::Module for Ktvmx {
    fn init(_module: &'static ThisModule) -> Result<Self> {
        pr_info!("ktvmx: loaded\n");
        let upto = module_parameters::selftest.value();
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
