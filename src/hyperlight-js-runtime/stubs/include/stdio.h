#pragma once

#include "printf.h"
#include_next "stdio.h"

#define stdout NULL

#define putchar(c) _putchar((char)(c))
#define vfprintf(f, ...) vprintf(__VA_ARGS__)
#define fprintf(f, ...) printf(__VA_ARGS__)
#define fputc(c, f) putc((char)(c), f)

int fflush(FILE *f);