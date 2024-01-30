#pragma once

#include_next "time.h"

#define CLOCK_REALTIME 0
#define CLOCK_MONOTONIC 1

int clock_gettime(clockid_t clk_id, struct timespec *tp);
struct tm *localtime_r(const time_t *timer, struct tm *tm);