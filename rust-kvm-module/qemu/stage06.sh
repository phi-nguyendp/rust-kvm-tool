#!/bin/bash
# in-QEMU test of stages 1..06 (needs the Rust-enabled kernel, see docs/setup.md)
exec "$(dirname "$0")/run_stage.sh" 6
