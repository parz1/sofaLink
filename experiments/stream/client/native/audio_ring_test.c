#include <assert.h>
#include <stdio.h>
#include <stdlib.h>
#include <pthread.h>
#include <unistd.h>
#include "audio_ring.h"
static void put(AudioRing *r, uint32_t frames, int64_t media) {
    assert(ring_space(r) >= frames);
    uint32_t w = atomic_load(&r->write);
    for (uint32_t i=0;i<frames;++i) {
        r->samples[((w+i)&(PCM_CAPACITY-1))*2]=123;
        r->samples[((w+i)&(PCM_CAPACITY-1))*2+1]=-123;
    }
    ring_publish(r,w,frames,media);
}
static atomic_int done;
static void *producer(void *context) {
    AudioRing *r=context;int64_t pcm,media;
    for (int i=0;i<2000;++i) {
        while(ring_space(r)<480) {ring_timestamp(r,0,&pcm,&media);usleep(10);}
        put(r,480,(int64_t)i*10000000LL);
        ring_timestamp(r,0,&pcm,&media);
    }
    atomic_store_explicit(&done,1,memory_order_release);return NULL;
}
static void *consumer(void *context) {
    AudioRing *r=context;int16_t dest[480];
    while(!atomic_load_explicit(&done,memory_order_acquire)
        || atomic_load(&r->write)!=atomic_load(&r->read)) {
        ring_consume(r,dest,240);
        for(int i=0;i<240;++i) assert((dest[i*2]==123 && dest[i*2+1]==-123)
            || (dest[i*2]==0 && dest[i*2+1]==0));
        usleep(10);
    }
    return NULL;
}
int main(void) {
    AudioRing *r=calloc(1,sizeof(*r)); assert(r);
    int16_t dest[1920]; int64_t pcm,media;
    ring_init(r,960,0);put(r,480,1000000000LL);
    assert(ring_space(r)==480);
    ring_consume(r,dest,960);
    assert(dest[0]==123 && dest[959]==-123 && dest[960]==0);
    assert(ring_timestamp(r,240,&pcm,&media) && pcm==240 && media==1005000000LL);
    assert(!ring_timestamp(r,600,&pcm,&media)); /* device silence is not PCM proof */
    put(r,480,1010000000LL);ring_consume(r,dest,480);
    assert(ring_timestamp(r,1080,&pcm,&media) && pcm==600 && media==1012500000LL);
    assert(atomic_load(&r->silence)==480 && atomic_load(&r->underruns)==1);
    for (int i=0;i<100;++i) {put(r,480,1020000000LL+i*10000000LL);ring_consume(r,dest,480);ring_timestamp(r,r->device_frames-240,&pcm,&media);}
    assert(dest[0]==123 && dest[959]==-123); /* wraparound */
    ring_init(r,960,50000);assert(!ring_timestamp(r,1080,&pcm,&media));
    put(r,480,7000000000LL);ring_consume(r,dest,480);
    assert(ring_timestamp(r,50240,&pcm,&media) && pcm==240 && media==7005000000LL);
    for (unsigned i=0;i<CLOCK_CAPACITY+10;++i) ring_consume(r,dest,480);
    assert(atomic_load(&r->clock_drops)>0); /* never block callback on telemetry */
    ring_init(r,960,0);atomic_init(&done,0);
    ring_init_format(r,882,0,44100);put(r,441,1000000000LL);ring_consume(r,dest,441);
    assert(ring_timestamp(r,220,&pcm,&media) && pcm==220 && media==1000000000LL+220LL*1000000000LL/44100);
    ring_init_format(r,882,1000,44100);put(r,441,2000000000LL);ring_consume(r,dest,441);
    assert(ring_timestamp(r,1220,&pcm,&media) && pcm==220 && media==2000000000LL+220LL*1000000000LL/44100);
    ring_init(r,960,0);
    pthread_t p,c;assert(!pthread_create(&p,NULL,producer,r));assert(!pthread_create(&c,NULL,consumer,r));
    pthread_join(p,NULL);pthread_join(c,NULL);
    assert(r->pcm_frames==2000LL*480 && atomic_load(&r->read)==atomic_load(&r->write));
    free(r);puts("Native audio ring / silence mapping / concurrent ownership checks PASS");return 0;
}
