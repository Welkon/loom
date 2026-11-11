# PMS150G board file — hx711

Example project adapter for a Padauk PMS150G. This directory holds the pin table
and the mini-C adapter; it has no host test, because mini-C is not C (see
[Verification](#verification)).

```
firmware/drivers/hx711/   installed by `loom add hx711`
board/README.md           this file: wiring, pins, part facts
board/ports_hx711.C       mini-C adapter
```

```sh
loom add hx711             # catalog unit, unchanged
loom adapter hx711         # writes board/ports_hx711.{h,c} in C
```

`loom adapter` writes standard C. On a Padauk target the generated file is a
reference for the signatures only: translate the three bodies into mini-C by
hand, as `board/ports_hx711.C` shows.

## Part

| Fact | Value | Source |
| --- | --- | --- |
| ROM | `400h` words | `INC_PDK/PMS150G.INC`, generated map `PMS150G.md` |
| RAM | `0x40` words | same |
| FPPA | `1` | same |
| Port pins | `PA.0`, `PA.3`-`PA.7` (`PA 0x10 : 0xF9`) | same |
| Direction register | `PAC` `0x11`, **1 = output** | same |
| Pull-up register | `PAPH` `0x12` | same |
| Digital input enable | `PADIER` `0x0D` | same |

`PA.1` and `PA.2` do not exist on this part. The register has the bits; the
package does not.

## Wiring

| Signal | Pin | Direction | Notes |
| --- | --- | --- | --- |
| HX711 `SCK` | `PA.3` | output | driven low at init, one pulse per bit |
| HX711 `DOUT` | `PA.4` | input | digital input enabled, no pull-up: the HX711 drives it |
| HX711 `VCC`/`GND` | — | — | 3.3 V rail shared with the part |

`PA.0` and `PA.5`-`PA.7` stay unassigned. Give unused pins a defined level (pull-up
via `PAPH`, or drive them) or the part leaks current in sleep.

## Adapter

```c
// board/ports_hx711.C — mini-C, not C
#include	"extern.h"

void	HX711_init(void)
{
	PADIER	=	0b0001_1000;	// PA3 output, PA4 digital input
	PAC		=	0b0000_1000;	// 1 = output, so only PA3
	PAPH	=	0b1110_0000;	// PA5..PA7 pull-up, not PA4
	PA.3	=	0;
}

bit	HX711_dout(void)
{
	return	PA.4;
}

void	HX711_sck(bit level)
{
	PA.3	=	level;
}

void	HX711_delay_us(byte microseconds)
{
	.delay	microseconds;
}
```

Mapping to the catalog port (`hx711_port_t`):

| Port field | mini-C function |
| --- | --- |
| `read_dout` | `HX711_dout()` — reads `PA.4` |
| `set_sck` | `HX711_sck(level)` — writes `PA.3` |
| `delay_us` | `HX711_delay_us(us)` — `.delay` |

## Fit check

`hx711` keeps a `hx711_port_t` plus a gain byte in the caller's struct, so the
unit itself is a handful of RAM words. The 24-bit shift loop, the sign
extension, and the ready poll all run from that struct. `400h` ROM words is the
real constraint, not RAM: the unit is C99 with no allocation, but a 1K-word OTP
part leaves little room for the rest of the product.

This is an estimate from the unit source, not a build. Compile and read the map
file before committing the part.

## Verification

| What | Status |
| --- | --- |
| Register names, addresses, pin mask | from the IDE `.INC`, generated map in `emb-support` |
| Unit behaviour (`0x123456`, sign extension, 25 clocks) | host-tested in the catalog, unchanged |
| Adapter compiles | **not verified** — mini-C needs the PADAUK IDE |
| Adapter behaves on a board | **not verified** |

mini-C is not C: `PAC.0 = 1;`, `.delay 400;`, and `$ PA.0 out,high;` are
compiler directives or bit syntax a standard compiler rejects. So this adapter
cannot use the fake-register host test that
`emb-loom/docs/adapters.md` describes for SCMCU and other C targets. The catalog
unit keeps its host test; the adapter is bench-only until an IDE build exists.

## Provenance

| Field | Value |
| --- | --- |
| Unit | `hx711` from <https://loom.pcbsmt.me>, MIT |
| Device facts | `PADAUK_Tool/1.05/INC_PDK/PMS150G.INC` via `references/sources/padauk/PMS150G.md` |
| mini-C idioms | `vendor-sdks/padauk-mcu-demo` on nexus, `src/zh-sc/基本示例/IO type/PMS15A/PMS15A_IO/` |
| Licence | device files and demo tree state none; only facts and idioms are reproduced |
