# Out-of-tree build against the Rust-enabled kernel build dir made by
# scripts/build_kernel.sh (see docs/setup.md).
#   make -C module            # all stages
#   make -C module STAGE=2    # only module/s01..s02 (selftest=N > STAGE reports "not built")
KDIR ?= $(HOME)/WS/linux_ktvmx
STAGE ?= 11
all:
	$(MAKE) -C $(KDIR) M=$(CURDIR) LLVM=1 STAGE=$(STAGE) modules
clean:
	$(MAKE) -C $(KDIR) M=$(CURDIR) clean
