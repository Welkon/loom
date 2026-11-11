// PMS150G adapter for the hx711 unit. mini-C, not C: build with the PADAUK IDE.
//
// Wiring (see board/README.md):
//   PA.3 -> HX711 SCK   (output)
//   PA.4 <- HX711 DOUT  (digital input, no pull-up)
//
// PAC bit = 1 is OUTPUT on this family, the inverse of the TRIS convention.
// The generated `loom adapter hx711` output is standard C; only its signatures
// carry over here.

#include	"extern.h"

void	HX711_init(void)
{
	PADIER	=	0b0001_1000;	// PA3 output, PA4 digital input
	PAC		=	0b0000_1000;	// 1 = output, so only PA3
	PAPH	=	0b1110_0000;	// PA5..PA7 pull-up; PA4 must float with the HX711
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
