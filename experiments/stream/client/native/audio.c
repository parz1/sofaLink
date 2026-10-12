#include <jni.h>
#include <aaudio/AAudio.h>
#include <stdlib.h>
#include <time.h>
#include "audio_ring.h"
typedef struct { AAudioStream *stream; AudioRing ring; atomic_int error; int requested, started, prime; } Output;
static Output *output(jlong handle) { return (Output *)(intptr_t)handle; }
static aaudio_data_callback_result_t consume(AAudioStream *stream, void *context, void *data, int32_t frames) {
    ring_consume(&((Output *)context)->ring, data, (uint32_t)frames);
    return AAUDIO_CALLBACK_RESULT_CONTINUE;
}
static void failed(AAudioStream *stream, void *context, aaudio_result_t result) {
    atomic_store_explicit(&((Output *)context)->error, result, memory_order_release);
}
static int start_ready(Output *v) {
    uint32_t queued = atomic_load_explicit(&v->ring.write, memory_order_acquire)
        - atomic_load_explicit(&v->ring.read, memory_order_acquire);
    if (v->requested && !v->started && queued >= (uint32_t)v->prime) {
        int result = AAudioStream_requestStart(v->stream);
        if (result != AAUDIO_OK) return result;
        v->started = 1;
    }
    return AAUDIO_OK;
}
JNIEXPORT jlong JNICALL Java_dev_livingroom_streamlab_AudioOutput_openNative(JNIEnv *env, jclass cls, jboolean exclusive, jint device, jint sample_rate) {
    if (sample_rate != 44100 && sample_rate != 48000) return 0;
    AAudioStreamBuilder *builder = NULL;
    Output *v = calloc(1, sizeof(*v));
    if (!v) return 0;
    atomic_init(&v->error, 0);
    if (AAudio_createStreamBuilder(&builder) != AAUDIO_OK) { free(v); return 0; }
    AAudioStreamBuilder_setDirection(builder, AAUDIO_DIRECTION_OUTPUT);
    if (device > 0) AAudioStreamBuilder_setDeviceId(builder, device);
    AAudioStreamBuilder_setFormat(builder, AAUDIO_FORMAT_PCM_I16);
    AAudioStreamBuilder_setSampleRate(builder, sample_rate);
    AAudioStreamBuilder_setChannelCount(builder, 2);
    AAudioStreamBuilder_setUsage(builder, AAUDIO_USAGE_GAME);
    AAudioStreamBuilder_setContentType(builder, AAUDIO_CONTENT_TYPE_MUSIC);
    AAudioStreamBuilder_setPerformanceMode(builder, AAUDIO_PERFORMANCE_MODE_LOW_LATENCY);
    AAudioStreamBuilder_setSharingMode(builder, exclusive ? AAUDIO_SHARING_MODE_EXCLUSIVE : AAUDIO_SHARING_MODE_SHARED);
    AAudioStreamBuilder_setDataCallback(builder, consume, v);
    AAudioStreamBuilder_setErrorCallback(builder, failed, v);
    int result = AAudioStreamBuilder_openStream(builder, &v->stream);
    AAudioStreamBuilder_delete(builder);
    if (result != AAUDIO_OK) { free(v); return 0; }
    int burst = AAudioStream_getFramesPerBurst(v->stream);
    /* At most 20 ms on fast devices; slow backends require two callback bursts
     * to absorb producer scheduling jitter without starving the callback. */
    uint32_t budget = (uint32_t)sample_rate / 50;
    uint32_t limit = (uint32_t)burst * 2 > budget ? (uint32_t)burst * 2 : budget;
    if (limit > PCM_CAPACITY || AAudioStream_getSampleRate(v->stream) != sample_rate
        || AAudioStream_getChannelCount(v->stream) != 2
        || AAudioStream_getFormat(v->stream) != AAUDIO_FORMAT_PCM_I16) {
        AAudioStream_close(v->stream); free(v); return 0;
    }
    ring_init_format(&v->ring, limit, 0, (uint32_t)sample_rate);
    /* Prime both bursts on a slow backend before its initial device fill.
     * Fast devices retain a 10 ms startup threshold within the 20 ms ring. */
    v->prime = burst > sample_rate / 100 ? burst * 2 : sample_rate / 100;
    AAudioStream_setBufferSizeInFrames(v->stream, burst * 2);
    return (jlong)(intptr_t)v;
}
JNIEXPORT jint JNICALL Java_dev_livingroom_streamlab_AudioOutput_writeNative(JNIEnv *env, jclass cls, jlong handle, jbyteArray bytes, jint offset, jint length, jlong media) {
    Output *v = output(handle);
    int error = atomic_load_explicit(&v->error, memory_order_acquire);
    if (error) return error;
    uint32_t frames = (uint32_t)length / 4, space = ring_space(&v->ring);
    if (frames > space) frames = space;
    if (!frames) return 0;
    uint32_t write = atomic_load_explicit(&v->ring.write, memory_order_relaxed);
    uint32_t index = write & (PCM_CAPACITY - 1), first = PCM_CAPACITY - index;
    if (first > frames) first = frames;
    (*env)->GetByteArrayRegion(env, bytes, offset, first * 4, (jbyte *)(v->ring.samples + index * 2));
    if ((*env)->ExceptionCheck(env)) return -1;
    if (frames > first) (*env)->GetByteArrayRegion(env, bytes, offset + first * 4, (frames - first) * 4, (jbyte *)v->ring.samples);
    if ((*env)->ExceptionCheck(env)) return -1;
    ring_publish(&v->ring, write, frames, media);
    error = start_ready(v);
    return error < 0 ? error : (jint)frames * 4;
}
JNIEXPORT jboolean JNICALL Java_dev_livingroom_streamlab_AudioOutput_timestampNative(JNIEnv *env, jclass cls, jlong handle, jlongArray values) {
    Output *v = output(handle); int64_t frame, time, pcm, media;
    if (AAudioStream_getTimestamp(v->stream, CLOCK_MONOTONIC, &frame, &time) != AAUDIO_OK) return JNI_FALSE;
    if (!ring_timestamp(&v->ring, frame, &pcm, &media)) return JNI_FALSE;
    jlong result[3] = {pcm, time, media};
    (*env)->SetLongArrayRegion(env, values, 0, 3, result); return JNI_TRUE;
}
JNIEXPORT jint JNICALL Java_dev_livingroom_streamlab_AudioOutput_statNative(JNIEnv *env, jclass cls, jlong handle, jint kind) {
    Output *v = output(handle); AAudioStream *s = v->stream;
    if (kind == 0) return AAudioStream_getBufferSizeInFrames(s);
    if (kind == 1) return AAudioStream_getXRunCount(s);
    if (kind == 3) return AAudioStream_getFramesPerBurst(s);
    if (kind == 4) return AAudioStream_getBufferCapacityInFrames(s);
    if (kind == 5) return atomic_load_explicit(&v->ring.underruns, memory_order_relaxed);
    if (kind == 6) return atomic_load_explicit(&v->ring.silence, memory_order_relaxed);
    if (kind == 7) return atomic_load_explicit(&v->ring.clock_drops, memory_order_relaxed);
    if (kind == 8) return v->ring.limit;
    if (kind == 9) return AAudioStream_getDeviceId(s);
    if (kind == 10) return AAudioStream_getSharingMode(s);
    return AAudioStream_getPerformanceMode(s);
}
static int wait_state(AAudioStream *s, aaudio_stream_state_t target) {
    for (int i = 0; i < 10; ++i) {
        aaudio_stream_state_t state = AAudioStream_getState(s), next;
        if (state == target) return AAUDIO_OK;
        if (state == AAUDIO_STREAM_STATE_DISCONNECTED) return AAUDIO_ERROR_DISCONNECTED;
        int result = AAudioStream_waitForStateChange(s, state, &next, 100000000LL);
        if (result != AAUDIO_OK && result != AAUDIO_ERROR_TIMEOUT) return result;
    }
    return AAUDIO_ERROR_TIMEOUT;
}
JNIEXPORT jint JNICALL Java_dev_livingroom_streamlab_AudioOutput_commandNative(JNIEnv *env, jclass cls, jlong handle, jint command) {
    Output *v = output(handle); AAudioStream *s = v->stream; int result;
    if (command == 0) { v->requested = 1; return start_ready(v); }
    if (command == 1) {
        if (v->started) {
            result = AAudioStream_requestPause(s); if (result != AAUDIO_OK) return result;
            result = wait_state(s, AAUDIO_STREAM_STATE_PAUSED); if (result != AAUDIO_OK) return result;
            result = AAudioStream_requestFlush(s); if (result != AAUDIO_OK) return result;
            result = wait_state(s, AAUDIO_STREAM_STATE_FLUSHED); if (result != AAUDIO_OK) return result;
        }
        ring_init_format(&v->ring, v->ring.limit, AAudioStream_getFramesWritten(s), (uint32_t)AAudioStream_getSampleRate(s));
        v->requested = v->started = 0; return AAUDIO_OK;
    }
    result = AAudioStream_close(s);
    if (result == AAUDIO_OK) free(v); /* Never free userdata while a callback can still run. */
    return result;
}
