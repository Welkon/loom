LOOM ?= ../../../emb-loom
CC ?= cc
CFLAGS ?= -std=c99 -Wall -Wextra -Werror -pedantic -O1 -DBOARD_HOST
INCLUDES := -Iboard -I$(LOOM)/drivers/hx711
SOURCES := board/fake_board.c board/ports_hx711.c tests/test_adapter.c
UNIT := $(LOOM)/drivers/hx711/hx711.c

.PHONY: test clean

test: adapter_test
	./adapter_test

adapter_test: $(SOURCES) $(UNIT) board/target.h board/ports_hx711.h
	$(CC) $(CFLAGS) $(INCLUDES) $(SOURCES) $(UNIT) -o $@

clean:
	rm -f adapter_test
