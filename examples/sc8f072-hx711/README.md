# SC8F072 adapter example

One catalog unit (`hx711`), one board file, one host test. The adapter is what
`loom adapter hx711` writes; the filled bodies are the only project work.

```
board/target.h        SFR shim and pin map; the only file that names a register
board/fake_board.c    host build: register array plus a write hook
board/ports_hx711.c   the adapter
tests/test_adapter.c  HX711 device model and behavior checks
```

## Run

```sh
make test
```

`LOOM` points at the catalog checkout (default `../../../emb-loom`):

```sh
make LOOM=/path/to/emb-loom test
```

## What this proves

- The generated skeleton compiles under `-std=c99 -Wall -Wextra -Werror -pedantic`.
- The adapter implements the unit's port contract, because the unit's own
  behavior assertions pass: `0x123456`, sign extension `-2`, and 25 clock
  pulses for gain 128.
- `boolsfr`/`PORTB` writes are the only chip access; the unit source is
  unmodified.

## What this does not prove

The XC8 branch has not been compiled by XC8 and has not run on a bench. The SFR
addresses are facts from the vendor header `SC8F072.h` (`PORTA` `0x86`, `TRISA`
`0x85`, `PORTB` `0x06`, `TRISB` `0x05`, `ANSEL1` `0x94`). The pin choice (RB1,
RB2) is example wiring, not a board. Treat timing, pull-up fit, and the analog
select as unverified until the board exists.

## Provenance

- Unit: `hx711` from <https://loom.pcbsmt.me>, MIT.
- Register facts: project copy of the SCMCU `SC8F072.h` header; names and
  addresses only, no vendor source copied.
- Chip note: `emb-support/skills/mcu-debug/references/sources/scmcu-sc8f072-gpio.md`.
