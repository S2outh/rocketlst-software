# CC1110 Linker Integration Notes

This folder provides a linker template for wiring `openlst-radio` low-level symbols into a target build.

## Files

- `cc1110-template.ld`: template memory/section layout with ISR and DMA sections.

## Exported symbols from Rust

When feature `cc1110-lowlevel` is enabled, `openlst-radio` exports:

- ISR shim functions:
  - `rf_isr`
  - `t1_isr`
  - `uart0_rx_isr`
  - `uart1_rx_isr`
- DMA descriptor statics:
  - `DMA_DESC_RF`
  - `DMA_DESC_AES_IN`
  - `DMA_DESC_AES_OUT`

## Symbol placement intent

- Place ISR handlers in flash and map into your startup/vector scheme.
- Place DMA descriptors in XRAM (`.cc1110_dma`) so peripheral DMA can access descriptor bytes.

## Rust build flags (example)

If your toolchain supports custom linker scripts directly:

```bash
cargo rustc -p openlst-radio --features cc1110-lowlevel,cc1110-real-mmio \
  -- -C link-arg=-Topenlst-radio/linker/cc1110-template.ld
```

Adjust for your target triple, linker flavor, and startup object files.
