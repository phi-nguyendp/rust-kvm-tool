//! Stage 3: the VMCS, its fields and their encodings.
//!
//! Kernel counterparts: arch/x86/kvm/vmx/vmcs.h (`vmcs_field_width`,
//! `vmcs_field_readonly`, `struct vmcs_hdr`), arch/x86/kvm/vmx/vmx_ops.h
//! (`vmcs_readl`, `vmcs_writel`, `vmcs_check*`: compile-time width checks),
//! arch/x86/kvm/vmx/vmx.c (`alloc_vmcs_cpu`, `vmx_vmcs_*`), arch/x86/include/asm/vmx.h
//! (`enum vmcs_field`, `VMCS_FIELD_*`), arch/x86/kvm/vmx/nested.c (`vmcs12_field_offset`
//! shows how KVM validates fields).
//! Book: Intel SDM Vol. 3C ch. 24 "Virtual Machine Control Structures" (24.2 format of
//! the VMCS region, 24.11.x VMREAD/VMWRITE, "Field encoding in the VMCS"), Appendix B
//! "Field Encoding in VMCS" and Appendix A.9 (IA32_VMX_VMCS_ENUM).
//!
//! A VMCS field is addressed by a 32-bit *encoding*, not an offset: the layout inside
//! the 4 KiB region is implementation-specific and software must only use VMREAD/VMWRITE.
#![allow(non_camel_case_types)]

use crate::vmcs_ops::VmcsOps;
use crate::vmx::*;
use crate::vmx_caps::{extract_bits, vmx_basic};

/// Encoding bits 14:13 (SDM Appendix B, "Structure of VMCS Component Encoding").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum vmcs_width {
    W16,
    W64,
    W32,
    /// Natural width: 64 bits on a 64-bit processor (always, for us).
    Natural,
}

/// Encoding bits 11:10.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum vmcs_type {
    Control,
    /// The "VM-exit information" fields: VMWRITE to them fails (unless VMX has the
    /// "VMWRITE to any field" capability in IA32_VMX_MISC; we do not use that).
    ReadOnly,
    Guest,
    Host,
}

fn extract_bits32(value: u32, hi: u32, lo: u32) -> u32 {
    // shifted and with the mask
    ((value >> lo) & ((1u32 << (hi - lo + 1)) - 1)) as u32
}

/// Encoding bit 0: 1 = access the *high* 32 bits of a 64-bit field.
pub fn vmcs_field_access_high(field: u32) -> bool {
    extract_bits32(field, 0, 0) == 1
}

/// Encoding bits 9:1.
pub fn vmcs_field_index(field: u32) -> u32 {
    extract_bits32(field, 9, 1)
}

/// Encoding bits 11:10. (kernel: `vmcs_field_type`)
pub fn vmcs_field_type(field: u32) -> vmcs_type {
    match extract_bits32(field, 11, 10) {
        0 => vmcs_type::Control,
        1 => vmcs_type::ReadOnly,
        2 => vmcs_type::Guest,
        _ => vmcs_type::Host,
    }
}

/// Encoding bits 14:13. (kernel: `vmcs_field_width`)
pub fn vmcs_field_width(field: u32) -> vmcs_width {
    match extract_bits32(field, 14, 13) {
        0 => vmcs_width::W16,
        1 => vmcs_width::W64,
        2 => vmcs_width::W32,
        _ => vmcs_width::Natural,
    }
}

/// Is it a VM-exit information field? (kernel: `vmcs_field_readonly`)
pub fn vmcs_field_readonly(field: u32) -> bool {
    vmcs_field_type(field) == vmcs_type::ReadOnly
}

/// Is the encoding well formed? Bit 12 and bits 31:15 must be 0, and the "high" access
/// type (bit 0) only exists for 64-bit-wide fields.
pub fn vmcs_field_valid_encoding(field: u32) -> bool {
    let bit12 = extract_bits32(field, 12, 12);
    let reserved_bit = extract_bits32(field, 31, 15);
    if bit12 != 0 || reserved_bit != 0 {
        return false;
    }
    if vmcs_field_width(field) != vmcs_width::W64 && vmcs_field_access_high(field) {
        return false;
    }
    true
}

/// How many bits the value has when accessed through this encoding: 16, 32 or 64.
/// A "full" access to a 64-bit or natural-width field is 64 bits; the "high" access
/// of a 64-bit field is the upper 32 bits (so 32 bits).
pub fn vmcs_field_bits(field: u32) -> u32 {
    match vmcs_field_width(field) {
        vmcs_width::W16 => 16,
        vmcs_width::W32 => 32,
        vmcs_width::W64 | vmcs_width::Natural => {
            if vmcs_field_access_high(field) {
                32
            } else {
                64
            }
        }
    }
}

/// The encoding that reaches the upper half of a *full-access* 64-bit field (`field + 1`);
/// `None` for anything else. (32-bit hosts needed this to write EPT_POINTER etc. in
/// two halves.)
pub fn vmcs_field_high(field: u32) -> Option<u32> {
    if vmcs_field_width(field) == vmcs_width::W64 && !vmcs_field_access_high(field) {
        Some(field + 1)
    } else {
        None
    }
}

/// Highest field index the CPU implements, from IA32_VMX_VMCS_ENUM (bits 9:1).
/// SDM Appendix A.9.
pub fn vmcs_enum_max_index(vmx_vmcs_enum: u64) -> u32 {
    extract_bits(vmx_vmcs_enum, 9, 1)
}

/// Mask the value a VMWRITE/VMREAD to `field` can carry (`vmcs_field_bits` wide).
pub fn vmcs_field_mask(field: u32) -> u64 {
    let field_bits = vmcs_field_bits(field);
    if field_bits == 64 {
        !0u64
    } else {
        (1u64 << field_bits) - 1
    }
}

/// A checked VMWRITE on top of `VmcsOps`, with the errors the hardware would give
/// (VM-instruction error numbers from vmx.rs):
/// - malformed encoding, or index above `vmcs_enum_max_index(vmx_vmcs_enum)`:
///   `Err(VMXERR_UNSUPPORTED_VMCS_COMPONENT)`, and nothing is written;
/// - read-only field: `Err(VMXERR_VMWRITE_READ_ONLY_VMCS_COMPONENT)`, nothing written;
/// - otherwise write `value & vmcs_field_mask(field)` (the hardware ignores the upper
///   bits of a narrower field) with `vmcs_writel`.
pub fn vmcs_checked_write<V: VmcsOps>(
    vmcs: &mut V,
    vmx_vmcs_enum: u64,
    field: u32,
    value: u64,
) -> Result<(), u32> {
    if !vmcs_field_valid_encoding(field)
        || vmcs_field_index(field) > vmcs_enum_max_index(vmx_vmcs_enum)
    {
        return Err(VMXERR_UNSUPPORTED_VMCS_COMPONENT);
    }
    if vmcs_field_readonly(field) {
        return Err(VMXERR_VMWRITE_READ_ONLY_VMCS_COMPONENT);
    }

    vmcs.vmcs_write64(field, value & vmcs_field_mask(field));
    Ok(())
}

/// A checked VMREAD: same validity checks (`Err(VMXERR_UNSUPPORTED_VMCS_COMPONENT)`), but
/// read-only fields are fine. The result is masked with `vmcs_field_mask`.
pub fn vmcs_checked_read<V: VmcsOps>(vmcs: &V, vmx_vmcs_enum: u64, field: u32) -> Result<u64, u32> {
    if !vmcs_field_valid_encoding(field)
        || vmcs_field_index(field) > vmcs_enum_max_index(vmx_vmcs_enum)
    {
        return Err(VMXERR_UNSUPPORTED_VMCS_COMPONENT);
    }
    let value = vmcs.vmcs_read64(field);
    Ok(value & vmcs_field_mask(field))
}

/// Initialise a VMCS region in place. Layout (SDM 24.2): dword 0 = revision identifier
/// in bits 30:0 and the *shadow-VMCS indicator* in bit 31; dword 1 = VMX-abort
/// indicator (0); the rest is implementation-specific (leave it zero).
/// Zero the first `basic.vmcs_size` bytes of `region`, then write the header.
/// Errors if `basic.vmcs_size` is 0 or above 4096, or `region` is shorter than it.
/// (kernel: `alloc_vmcs_cpu`, `struct vmcs_hdr { u32 revision_id:31; u32 shadow_vmcs:1; }`)
pub fn vmcs_region_init(
    region: &mut [u8],
    basic: &vmx_basic,
    shadow: bool,
) -> Result<(), &'static str> {
    if basic.vmcs_size == 0 || basic.vmcs_size > 4096 || region.len() < basic.vmcs_size as usize {
        return Err("size mistmach");
    }
    region[..basic.vmcs_size as usize].fill(0x0);

    let mut dword0 = basic.revision_id;
    if shadow {
        dword0 |= 0x1 << 31;
    }
    region[..4].copy_from_slice(&dword0.to_ne_bytes());
    Ok(())
}
