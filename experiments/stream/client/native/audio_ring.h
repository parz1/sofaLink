#ifndef STREAM_AUDIO_RING_H
#define STREAM_AUDIO_RING_H
#include <stdint.h>
#include <stdatomic.h>
#include <string.h>
#define PCM_CAPACITY 16384u
#define CLOCK_CAPACITY 1024u
typedef struct { int64_t device, pcm, media; uint32_t valid, total; } AudioSegment;
/* One network producer, one real-time consumer. All atomics are lock-free 32-bit
 * on supported Android ABIs; timestamps are published by FIFO ownership. */
typedef struct {
    int16_t samples[PCM_CAPACITY * 2];
    int64_t media[PCM_CAPACITY];
    atomic_uint read, write, clock_read, clock_write, underruns, silence, clock_drops;
    uint32_t limit;
    uint32_t sample_rate;
    int64_t device_frames, pcm_frames;
    AudioSegment clock[CLOCK_CAPACITY], history[CLOCK_CAPACITY];
    uint32_t history_write, history_count;
} AudioRing;
/* Only initialize while the callback is stopped. */
static void ring_init(AudioRing *r, uint32_t limit, int64_t device) {
    memset(r, 0, sizeof(*r)); r->limit = limit; r->device_frames = device;
    r->sample_rate = 48000;
    atomic_init(&r->read, 0); atomic_init(&r->write, 0);
    atomic_init(&r->clock_read, 0); atomic_init(&r->clock_write, 0);
    atomic_init(&r->underruns, 0); atomic_init(&r->silence, 0); atomic_init(&r->clock_drops, 0);
}
static void ring_init_format(AudioRing *r, uint32_t limit, int64_t device, uint32_t sample_rate) {
    ring_init(r, limit, device); r->sample_rate = sample_rate;
}
static uint32_t ring_space(AudioRing *r) {
    uint32_t used = atomic_load_explicit(&r->write, memory_order_relaxed)
        - atomic_load_explicit(&r->read, memory_order_acquire);
    return r->limit - used;
}
static void ring_publish(AudioRing *r, uint32_t start, uint32_t frames, int64_t media) {
    /* Exact floor timestamps with two divisions per packet, not per sample. */
    int64_t step = 1000000000LL / r->sample_rate;
    uint32_t remainder = (uint32_t)(1000000000LL % r->sample_rate), fraction = 0;
    for (uint32_t i = 0; i < frames; ++i) {
        r->media[(start + i) & (PCM_CAPACITY - 1)] = media;
        media += step; fraction += remainder;
        if (fraction >= r->sample_rate) { media++; fraction -= r->sample_rate; }
    }
    atomic_store_explicit(&r->write, start + frames, memory_order_release);
}
static void ring_consume(AudioRing *r, int16_t *dest, uint32_t frames) {
    uint32_t read = atomic_load_explicit(&r->read, memory_order_relaxed);
    uint32_t valid = atomic_load_explicit(&r->write, memory_order_acquire) - read;
    if (valid > frames) valid = frames;
    AudioSegment segment = {r->device_frames, r->pcm_frames,
        valid ? r->media[read & (PCM_CAPACITY - 1)] : 0, valid, frames};
    uint32_t index = read & (PCM_CAPACITY - 1), first = PCM_CAPACITY - index;
    if (first > valid) first = valid;
    memcpy(dest, r->samples + index * 2, first * 4);
    memcpy(dest + first * 2, r->samples, (valid - first) * 4);
    memset(dest + valid * 2, 0, (frames - valid) * 4);
    atomic_store_explicit(&r->read, read + valid, memory_order_release);
    if (valid < frames) {
        atomic_fetch_add_explicit(&r->underruns, 1, memory_order_relaxed);
        atomic_fetch_add_explicit(&r->silence, frames - valid, memory_order_relaxed);
    }
    r->device_frames += frames; r->pcm_frames += valid;
    uint32_t cw = atomic_load_explicit(&r->clock_write, memory_order_relaxed);
    if (cw - atomic_load_explicit(&r->clock_read, memory_order_acquire) < CLOCK_CAPACITY) {
        r->clock[cw & (CLOCK_CAPACITY - 1)] = segment;
        atomic_store_explicit(&r->clock_write, cw + 1, memory_order_release);
    } else atomic_fetch_add_explicit(&r->clock_drops, 1, memory_order_relaxed);
}
/* Worker-owned history maps a device timestamp to real PCM, including silence
 * inserted by starvation. Silence never counts as received media playback. */
static int ring_timestamp(AudioRing *r, int64_t device, int64_t *pcm, int64_t *media) {
    uint32_t cr = atomic_load_explicit(&r->clock_read, memory_order_relaxed);
    uint32_t cw = atomic_load_explicit(&r->clock_write, memory_order_acquire);
    while (cr != cw) {
        r->history[r->history_write++ & (CLOCK_CAPACITY - 1)] = r->clock[cr++ & (CLOCK_CAPACITY - 1)];
        if (r->history_count < CLOCK_CAPACITY) ++r->history_count;
    }
    atomic_store_explicit(&r->clock_read, cr, memory_order_release);
    for (uint32_t i = 0; i < r->history_count; ++i) {
        AudioSegment s = r->history[(r->history_write - 1 - i) & (CLOCK_CAPACITY - 1)];
        int64_t offset = device - s.device;
        if (offset >= 0 && offset < s.total) {
            if (offset >= s.valid) return 0;
            *pcm = s.pcm + offset; *media = s.media + offset * 1000000000LL / r->sample_rate;
            return 1;
        }
    }
    return 0;
}
#endif
