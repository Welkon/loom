# PMS150G adapter example

A Padauk PMS150G project adapter for the `hx711` unit: the pin table, the
mini-C adapter, and what remains unverified.

```
board/README.md         wiring, part facts, adapter, fit check
```

Unlike `examples/sc8f072-hx711/`, this example has no `make test`: mini-C is not
C, so the adapter cannot go through the fake-register host test. Read
`board/README.md` for the reasoning and the two honest options.

The catalog unit is unchanged and keeps its own host test.
