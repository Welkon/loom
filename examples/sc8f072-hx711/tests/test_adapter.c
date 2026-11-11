/* Host test for the SC8F072 adapter. No hardware, no XC8.
 *
 * board/fake_board.c turns BOARD_SET/BOARD_GET into a byte array plus a write
 * hook. The hook models the HX711 shift register, so the unit under test is
 * the adapter, not the chip.
 */
#include <stdio.h>
#include <string.h>

#include "target.h"
#include "ports_hx711.h"

static int failures = 0;
#define CHECK(cond) do { \
    if (!(cond)) { \
        fprintf(stderr, "FAIL %s:%d: %s\n", __FILE__, __LINE__, #cond); \
        failures++; \
    } \
} while (0)

typedef struct {
    uint32_t bits;
    uint8_t count;
    uint8_t pin_high;
} device_t;

static device_t device;

static void model_write(uint8_t pin, uint8_t level)
{
    if (pin != BOARD_HX711_SCK_PIN) return;
    if (level) {
        device.pin_high++;
        if (device.count < 24) {
            const uint8_t bit = (uint8_t)((device.bits >> (23 - device.count)) & 1u);
            if (bit) board_sfr[0] |= (uint8_t)(1u << BOARD_HX711_DOUT_PIN);
            else board_sfr[0] &= (uint8_t)~(1u << BOARD_HX711_DOUT_PIN);
        } else {
            board_sfr[0] &= (uint8_t)~(1u << BOARD_HX711_DOUT_PIN);
        }
    } else if (device.count < 24) {
        device.count++;
    }
}

int main(void)
{
    hx711_t scale;
    int32_t value;

    board_write_hook = model_write;
    board_io_init();
    CHECK(hx711_init(&scale, board_hx711_port()) == 1);

    CHECK(hx711_ready(&scale) == 1); /* DOUT low: data available */
    board_sfr[0] |= (uint8_t)(1u << BOARD_HX711_DOUT_PIN);
    CHECK(hx711_ready(&scale) == 0);
    board_sfr[0] &= (uint8_t)~(1u << BOARD_HX711_DOUT_PIN);

    memset(&device, 0, sizeof(device));
    device.bits = 0x123456;
    CHECK(hx711_read(&scale, &value) == 1);
    CHECK(value == 0x123456);
    CHECK(device.pin_high == 25); /* 24 data clocks + 1 gain clock */

    memset(&device, 0, sizeof(device));
    device.bits = 0xFFFFFE; /* sign extension */
    CHECK(hx711_read(&scale, &value) == 1);
    CHECK(value == -2);

    if (failures) return 1;
    puts("sc8f072-hx711: ok");
    return 0;
}
