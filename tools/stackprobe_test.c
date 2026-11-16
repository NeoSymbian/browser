// SPDX-License-Identifier: Apache-2.0
// Self-test for stackprobe.so: threads touch known stack depths; the reports must match within a small slack.
//   gcc -O1 -o stackprobe_test stackprobe_test.c -lpthread
//   STACKPROBE_OUT=out.txt LD_PRELOAD=./stackprobe.so ./stackprobe_test && ./stackprobe_check.sh out.txt
#define _GNU_SOURCE
#include <pthread.h>
#include <stdio.h>
#include <string.h>

static const size_t depths[] = {16 << 10, 128 << 10, 700 << 10};

static void *touch(void *arg) {
    size_t n = *(const size_t *)arg;
    volatile char buf[n]; // a VLA puts exactly n bytes on this thread's stack
    memset((char *)buf, 1, n);
    char name[24]; // pthread names are capped at 15 chars; ours are short
    snprintf(name, sizeof name, "d%zu", n >> 10);
    pthread_setname_np(pthread_self(), name);
    return (void *)(size_t)buf[n / 2];
}

int main(void) {
    pthread_attr_t a;
    pthread_attr_init(&a);
    pthread_attr_setstacksize(&a, 1 << 20);
    pthread_t t[3];
    for (int i = 0; i < 3; i++) pthread_create(&t[i], &a, touch, (void *)&depths[i]);
    for (int i = 0; i < 3; i++) pthread_join(t[i], NULL);
    return 0;
}
