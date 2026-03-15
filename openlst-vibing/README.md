# openlst-vibing

Rust rewrite workspace for `openlst-firmware`.

## Scope

This workspace ports protocol and firmware logic into Rust crates:

- `openlst-core`: `no_std` core logic and protocol types
- `openlst-radio`: radio application loop skeleton
- `openlst-bootloader`: bootloader decision flow skeleton
- `openlst-sim`: host simulator for command handling and scheduler behavior

## Status

The original firmware runs on TI CC1110 (8051). Rust does not currently provide an official Tier target for that MCU family, so this rewrite focuses on behavior-compatible logic and clean interfaces that can be bound to hardware-specific implementations.

Current rewrite includes:

- packet header/message definitions
- command handling behavior parity for ACK/NACK/time/reboot/telemetry/callsign/ranging
- scheduler behavior parity for auto reboot and 10 Hz tasks
- software CRC16 implementation for host-side validation
- `RadioHal`/`BootloaderHal` abstraction layer for plugging in CC1110 hardware backends
- optional `cc1110-real-mmio` feature in `openlst-radio` for volatile register access (`unsafe`)

## Real MMIO feature

`openlst-radio` exposes a `cc1110-real-mmio` feature that enables a `VolatileRegisterIo` implementation using `read_volatile`/`write_volatile`.

Build check:

```bash
cargo check -p openlst-radio --features cc1110-real-mmio
```

This feature is a low-level backend primitive only; using it at runtime requires a valid mapped register base address and target-specific startup/linker support.

Runtime base address can be provided as hex via `OPENLST_MMIO_BASE` (for example `OPENLST_MMIO_BASE=0x0000`).

## Low-level ISR/DMA shims

`openlst-radio` exposes `cc1110-lowlevel` with:

- linker-visible ISR shim symbols (`rf_isr`, `t1_isr`, `uart0_rx_isr`, `uart1_rx_isr`)
- CC1110 DMA descriptor layout (`repr(C, packed)`, 8-byte descriptor)
- exported DMA descriptor symbols (`DMA_DESC_RF`, `DMA_DESC_AES_IN`, `DMA_DESC_AES_OUT`) in `.cc1110_dma`

Build check:

```bash
cargo check -p openlst-radio --features cc1110-lowlevel
```

Combined low-level + real MMIO:

```bash
cargo check -p openlst-radio --features cc1110-lowlevel,cc1110-real-mmio
```

Linker template and integration notes are in [openlst-radio/linker](openlst-radio/linker).

Workspace cargo config in [openlst-vibing/.cargo/config.toml](openlst-vibing/.cargo/config.toml) adds:

- target profile `cc1110-none-elf` with automatic linker script arg
- alias `radio-cc1110` for convenience builds

Example:

```bash
cargo radio-cc1110
```

## Rust build/sign/flash script

Script: [openlst-vibing/scripts/build_sign_flash_rust.sh](scripts/build_sign_flash_rust.sh)

This script:

1. builds Rust radio firmware
2. converts ELF to HEX
3. starts `radio_mux` in the background
4. signs the HEX
5. flashes via `bootload_radio`

By default, `radio_mux` stays alive while signing/flashing and is cleaned up when the script exits.

Example:

```bash
./scripts/build_sign_flash_rust.sh --signing-key 1546b4ec69f6266fb034b1958b830843
```

Keep `radio_mux` running after script completion:

```bash
./scripts/build_sign_flash_rust.sh --signing-key <key> --keep-mux
```

Open `radio_terminal` after flashing (still with `radio_mux` running):

```bash
./scripts/build_sign_flash_rust.sh --signing-key <key> --terminal
```

## CRC lookup-table option

`openlst-core` exposes a `crc-lut` feature that swaps the bitwise CRC16 implementation for a table-driven variant.

```bash
cargo check -p openlst-core --features crc-lut
```

This is typically faster on many targets at the cost of additional read-only table storage.

## Quick start

```bash
cd openlst-vibing
cargo check
cargo run -p openlst-sim
```
