// OpenLST
// Copyright (C) 2018 Planet Labs Inc.
// 
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
// 
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

#ifndef _BOARD_H
#define _BOARD_H

// We use a 27MHz clock
#define F_CLK 27000000

#define CUSTOM_BOARD_INIT 1
#define BOARD_HAS_TX_HOOK 1
#define BOARD_HAS_RX_HOOK 1
#define CONFIG_CAPABLE_RF_RX 1
#define CONFIG_CAPABLE_RF_TX 0

// Disable UART0
// #define UART0_ENABLED 0

// don't auto reboot
#define AUTO_REBOOT_SECONDS 0   //auto reboot disabled

// # Power setting
// PA_CONFIG of 192 (0xC0) is recommended by the datasheet (pg 207)
// for 33mA output at 433MHz. It is the highest setting listed in
// the datasheet.
// https://www.ti.com/lit/ds/symlink/cc1110-cc1111.pdf?ts=1719502976799&ref_url=https%253A%252F%252Fwww.ti.com%252Fproduct%252FCC1110-CC1111%253Fbm-verify%253DAAQAAAAJ_____zIZcrRTZEtbfeo0gPeq_ygxBa76nETJvCLoGOwDVmlBrSts_Urld8DbDGqSqUCw_EW1NzGjBCJ-Iq9NfK6WWA369Xjjv6LITcyj3vj0Y2QV2jGuaTlGqWXEJWYrrTH3KrxpcYW8z3LxiuYmLwWVyGtY5hCujrZCW1z9VLzKT_gLnDblSR8vrklzyaj8tFGUK0W7mdP7z9BMvnwdJIdMj0Q4-9gXi7IFDn2sIa7TOqfen_zbGn-pKgs3SPhH1l51GDau40MP7kL3I00fQVwEq1oHoGAvO4sH-oikEJV32eRFki2A6cGvsJ4
// Page 207
//#define RF_PA_CONFIG     0x12 // -30dbm at CC
#define RF_PA_CONFIG     0x0E // -20dbm, at CC
//#define RF_PA_CONFIG     0x1D // -15dbm, at CC
//#define RF_PA_CONFIG     0x34 // -10dbm, at CC
//#define RF_PA_CONFIG     0x2C // -5dbm, at CC
//#define RF_PA_CONFIG     0x60 // 0dbm, at CC
//#define RF_PA_CONFIG     0x84 // 5dbm, at CC
//#define RF_PA_CONFIG     0xC8 // 7dbm, at CC
//#define RF_PA_CONFIG     0xC0 // 10dbm, at CC


// # Frequency setting
// https://www.ti.com/lit/ds/symlink/cc1110-cc1111.pdf?ts=1719502976799&ref_url=https%253A%252F%252Fwww.ti.com%252Fproduct%252FCC1110-CC1111%253Fbm-verify%253DAAQAAAAJ_____zIZcrRTZEtbfeo0gPeq_ygxBa76nETJvCLoGOwDVmlBrSts_Urld8DbDGqSqUCw_EW1NzGjBCJ-Iq9NfK6WWA369Xjjv6LITcyj3vj0Y2QV2jGuaTlGqWXEJWYrrTH3KrxpcYW8z3LxiuYmLwWVyGtY5hCujrZCW1z9VLzKT_gLnDblSR8vrklzyaj8tFGUK0W7mdP7z9BMvnwdJIdMj0Q4-9gXi7IFDn2sIa7TOqfen_zbGn-pKgs3SPhH1l51GDau40MP7kL3I00fQVwEq1oHoGAvO4sH-oikEJV32eRFki2A6cGvsJ4
// Page 214
//#define RF_FREQ2 0x10 // 439.715
//#define RF_FREQ2 0x10 // 437.123
#define RF_FREQ2 0x10 // 434.200
//#define RF_FREQ2 0x10 // 434.205
//#define RF_FREQ2 0x10 // 434.210

//#define RF_FREQ1 0x49 // 439.715
//#define RF_FREQ1 0x30 // 437.123
#define RF_FREQ1 0x14 // 434.200
//#define RF_FREQ1 0x14 // 434.205
//#define RF_FREQ1 0x14 // 434.210

//#define RF_FREQ0 0x26 // 439.715
//#define RF_FREQ0 0x93 // 437.123
#define RF_FREQ0 0xDC // 434.200
//#define RF_FREQ0 0xE8 // 434.205
//#define RF_FREQ0 0xF4 // 434.210


// # Bandwith setting
//#define RF_CHAN_BW_E 3 // 60.267kHz
#define RF_CHAN_BW_E 0x00 // 562.500000kHz

//#define RF_CHAN_BW_M 3 // 60.267kHz
#define RF_CHAN_BW_M 0x01 // 562.500000kHz

// # Baudrate setting
// https://www.ti.com/lit/ds/symlink/cc1110-cc1111.pdf?ts=1719502976799&ref_url=https%253A%252F%252Fwww.ti.com%252Fproduct%252FCC1110-CC1111%253Fbm-verify%253DAAQAAAAJ_____zIZcrRTZEtbfeo0gPeq_ygxBa76nETJvCLoGOwDVmlBrSts_Urld8DbDGqSqUCw_EW1NzGjBCJ-Iq9NfK6WWA369Xjjv6LITcyj3vj0Y2QV2jGuaTlGqWXEJWYrrTH3KrxpcYW8z3LxiuYmLwWVyGtY5hCujrZCW1z9VLzKT_gLnDblSR8vrklzyaj8tFGUK0W7mdP7z9BMvnwdJIdMj0Q4-9gXi7IFDn2sIa7TOqfen_zbGn-pKgs3SPhH1l51GDau40MP7kL3I00fQVwEq1oHoGAvO4sH-oikEJV32eRFki2A6cGvsJ4
// Page 191
//#define RF_DRATE_E   8 // 7415 baud
//#define RF_DRATE_E   8 // 10k baud
//#define RF_DRATE_E   9 // 20k baud
//#define RF_DRATE_E   10 // 40k baud
#define RF_DRATE_E   10 // 50k baud (49.953)
//#define RF_DRATE_E   13 // 250k baud

//#define RF_DRATE_M  32 // 7415 baud
//#define RF_DRATE_M  132 // 10k baud
//#define RF_DRATE_M  132 // 20k baud
//#define RF_DRATE_M  132 // 40k baud
#define RF_DRATE_M  229 // 50k baud
//#define RF_DRATE_M  47 // 250k baud

// # Frequency diff between symbols
//#define RF_DEVIATN_E 1 // 3707 Hz
#define RF_DEVIATN_E 6 // 126.953125 kHz
//#define RF_DEVIATN_M 1 // 3707 Hz
#define RF_DEVIATN_M 2 // 126.953125 kHz

// Enable the power supply sense lines AN0 and AN1
#define ADCCFG_CONFIG 0b00000011

#define RADIO_RANGING_RESPONDER 1

void board_init(void);

#define BOARD_HAS_LED 1
void board_led_set(__bit led_on);

// These are macros to save space in the bootloader
// Enable bias to on-board 1W RF power amp (RF6504)
#define board_pre_tx() P2_0 = 1;
// Disable on-board power amp bias, to save power
#define board_pre_rx() P2_0 = 0;

#endif
