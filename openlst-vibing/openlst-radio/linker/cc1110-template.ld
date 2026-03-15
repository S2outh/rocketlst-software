/*
 * CC1110 linker template for openlst-radio low-level integration.
 *
 * This is a template (not yet tied to a specific Rust target triple).
 * Adapt section names and entry point to your final compiler/linker flow.
 */

MEMORY
{
  FLASH (rx)  : ORIGIN = 0x0000, LENGTH = 0x8000
  XRAM  (rwx) : ORIGIN = 0xF000, LENGTH = 0x1000
}

SECTIONS
{
  .text :
  {
    *(.text .text.*)
    *(.rodata .rodata.*)
  } > FLASH

  .data :
  {
    *(.data .data.*)
  } > XRAM AT > FLASH

  .bss (NOLOAD) :
  {
    *(.bss .bss.* COMMON)
  } > XRAM

  /*
   * Low-level ISR shim symbols exported by openlst-radio/src/lowlevel.rs:
   *   rf_isr
   *   t1_isr
   *   uart0_rx_isr
   *   uart1_rx_isr
   *
   * If your startup code builds a vector table, map those handlers here.
   * Keep directives ensure linker doesn't discard them.
   */
  .vectors :
  {
    KEEP(*(.vectors .vectors.*))
    KEEP(*(.text.rf_isr))
    KEEP(*(.text.t1_isr))
    KEEP(*(.text.uart0_rx_isr))
    KEEP(*(.text.uart1_rx_isr))
  } > FLASH

  /*
   * DMA descriptor placement for low-level feature.
   * Exported symbols:
   *   DMA_DESC_RF
   *   DMA_DESC_AES_IN
   *   DMA_DESC_AES_OUT
   */
  .cc1110_dma (NOLOAD) : ALIGN(1)
  {
    KEEP(*(.cc1110_dma))
  } > XRAM
}
