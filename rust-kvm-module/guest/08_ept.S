# Stage 8 guest: real mode, entered at CS:IP = 0000:1000 (needs EPT + unrestricted guest).
# Stores AL to guest-physical 0x9000, a page the host deliberately leaves unmapped in the
# EPT: the store takes an EPT violation, the host maps the page and resumes, the CPU
# re-executes the store, then HLT.
# Expected: EPT violation with GUEST_PHYSICAL_ADDRESS = 0x9000, write access, no
# permissions; afterwards AX = 0x1234 and the byte at 0x9000 = 0x34.
	.code16
	.globl _start
_start:
	mov	$0x1234, %ax
	mov	%al, 0x9000
	hlt
