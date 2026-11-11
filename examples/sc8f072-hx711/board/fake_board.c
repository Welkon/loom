#include "target.h"

volatile uint8_t board_sfr[256];
void (*board_write_hook)(uint8_t pin, uint8_t level);

void board_pin_write(uint8_t pin, uint8_t level)
{
    if (level) board_sfr[0] |= (uint8_t)(1u << pin);
    else board_sfr[0] &= (uint8_t)~(1u << pin);
    if (board_write_hook) board_write_hook(pin, level);
}

uint8_t board_pin_read(uint8_t pin)
{
    return (uint8_t)((board_sfr[0] >> pin) & 1u);
}

void board_delay_us(unsigned long microseconds) { (void)microseconds; }
void board_delay_ms(unsigned long milliseconds) { (void)milliseconds; }
