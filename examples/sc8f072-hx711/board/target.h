/* SCMCU SC8F072 board file for the hx711 example.
 *
 * Example wiring, not a board claim:
 *   RB1 -> HX711 SCK  (output)
 *   RB2 <- HX711 DOUT (input)
 *
 * Register names and addresses come from the vendor header SC8F072.h; see
 * emb-support skills/mcu-debug/references/sources/scmcu-sc8f072-gpio.md.
 * No vendor source is copied here.
 */
#ifndef BOARD_TARGET_H
#define BOARD_TARGET_H

#include <stdint.h>

#define BOARD_HX711_SCK_PIN  1u
#define BOARD_HX711_DOUT_PIN 2u

void board_delay_us(unsigned long microseconds);
void board_delay_ms(unsigned long milliseconds);

#if defined(BOARD_HOST)

/* Host build: the register file is plain memory and writes are observable. */
extern volatile uint8_t board_sfr[256];
extern void (*board_write_hook)(uint8_t pin, uint8_t level);
void board_pin_write(uint8_t pin, uint8_t level);
uint8_t board_pin_read(uint8_t pin);

#define BOARD_SET(pin, level) board_pin_write((pin), (level))
#define BOARD_GET(pin) board_pin_read(pin)

static inline void board_io_init(void)
{
    board_sfr[0] = 0;
}

#else

/* XC8 dialect. Keep this the only place that names a SFR. */
volatile unsigned char PORTA @ 0x86;
volatile unsigned char TRISA @ 0x85;
volatile unsigned char PORTB @ 0x06;
volatile unsigned char TRISB @ 0x05;
volatile unsigned char ANSEL0 @ 0x93;
volatile unsigned char ANSEL1 @ 0x94;

#define BOARD_SET(pin, level) \
    ((level) ? (PORTB |= (unsigned char)(1u << (pin))) \
             : (PORTB &= (unsigned char)~(1u << (pin))))
#define BOARD_GET(pin) ((PORTB >> (pin)) & 1u)

static inline void board_io_init(void)
{
    ANSEL1 = 0u;
    /* RB1 output, RB2 input. TRISB is 1 for input. */
    TRISB = (unsigned char)((TRISB | 0x04u) & ~0x02u);
}

#endif

#endif
