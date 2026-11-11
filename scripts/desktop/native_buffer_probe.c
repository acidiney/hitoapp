#define _GNU_SOURCE
#include <pulse/pulseaudio.h>
#include <alsa/asoundlib.h>
#include <stdatomic.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <time.h>
#include <sys/time.h>
#include <unistd.h>

#define MAX_RECORDS 8192
#define MAX_FRAMES (30 * 48000)
#define MAX_STREAMS 8

enum { REGISTRATION=1, WRITE_BEGIN, WRITE_END, UNDERFLOW, PEEK, DROP, ATTRIBUTES,
       TIMING, ARM, HOLD, STATE, DISCONNECT };
struct record { atomic_uint_fast64_t committed; uint64_t kind, stream, at_ns; int64_t value[16]; };
struct slot {
    atomic_uintptr_t pointer;
    uint64_t id, written, cursor, pending_length;
    const void *pending_pointer;
    bool pending, disconnected;
    pa_stream_notify_cb_t callback;
    void *userdata;
};
static struct record records[MAX_RECORDS];
static struct slot streams[MAX_STREAMS];
static atomic_uint_fast64_t record_count, pcm_count, invalid, overflow, hold_count;
static atomic_bool armed;
static uint64_t last_completion_ns, armed_source, source_frames;
static snd_pcm_uframes_t actual_buffer, actual_period;
static int (*next_get_params)(snd_pcm_t *, snd_pcm_uframes_t *, snd_pcm_uframes_t *);
static int (*next_sw_params)(snd_pcm_t *, snd_pcm_sw_params_t *);
static float pcm[MAX_FRAMES];
static int ledger_fd=-1, pcm_fd=-1;
static uint64_t role_id;
static int (*next_write)(pa_stream*,const void*,size_t,pa_free_cb_t,int64_t,pa_seek_mode_t);
static void (*next_underflow)(pa_stream*,pa_stream_notify_cb_t,void*);
static int (*next_peek)(pa_stream*,const void**,size_t*);
static int (*next_drop)(pa_stream*);
static int (*next_disconnect)(pa_stream*);

static uint64_t now_ns(void) {
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC,&t);
    return (uint64_t)t.tv_sec*1000000000ULL+(uint64_t)t.tv_nsec;
}

static void event(uint64_t kind, uint64_t stream, const int64_t *values, size_t count) {
    uint64_t index=atomic_fetch_add_explicit(&record_count,1,memory_order_relaxed);
    if (index>=MAX_RECORDS) { atomic_store(&overflow,1); return; }
    struct record *r=&records[index];
    r->kind=kind; r->stream=stream; r->at_ns=now_ns();
    for (size_t i=0;i<count && i<16;i++) r->value[i]=values[i];
    atomic_store_explicit(&r->committed,index+1,memory_order_release);
}
#define EVENT(kind,stream,...) do { int64_t values_[]={__VA_ARGS__}; event(kind,stream,values_,sizeof(values_)/sizeof(values_[0])); } while(0)

static struct slot *slot(pa_stream *s) {
    uintptr_t p=(uintptr_t)s;
    for (size_t i=0;i<MAX_STREAMS;i++)
        if (atomic_load(&streams[i].pointer)==p) return &streams[i];
    for (size_t i=0;i<MAX_STREAMS;i++) {
        uintptr_t empty=0;
        if (atomic_compare_exchange_strong(&streams[i].pointer,&empty,p)) {
            streams[i].id=i+1;
            return &streams[i];
        }
    }
    atomic_store(&invalid,1); return NULL;
}

static void attributes(pa_stream *s, struct slot *entry) {
    const pa_buffer_attr *a=pa_stream_get_buffer_attr(s);
    const pa_sample_spec *spec=pa_stream_get_sample_spec(s);
    if (!a || !spec) { atomic_store(&invalid,1); return; }
    EVENT(ATTRIBUTES,entry->id,a->maxlength,a->tlength,a->prebuf,a->minreq,a->fragsize,
          spec->rate,spec->channels,spec->format,pa_stream_get_index(s));
}

static void underflow_callback(pa_stream *s, void *userdata) {
    struct slot *entry=userdata;
    int64_t index=pa_stream_get_underflow_index(s);
    EVENT(UNDERFLOW,entry->id,index,entry->written,index>=0 && entry->written<=(uint64_t)index,
          entry->callback!=NULL);
    if (entry->callback) entry->callback(s,entry->userdata);
}

void pa_stream_set_underflow_callback(pa_stream *s,pa_stream_notify_cb_t callback,void *userdata) {
    struct slot *entry=slot(s);
    if (!entry) { next_underflow(s,callback,userdata); return; }
    if (entry->disconnected) {
        entry->written=0; entry->cursor=0; entry->pending=false; entry->disconnected=false;
    }
    entry->callback=callback; entry->userdata=userdata;
    EVENT(REGISTRATION,entry->id,callback!=NULL);
    next_underflow(s,callback?underflow_callback:NULL,callback?entry:userdata);
}

int snd_pcm_get_params(snd_pcm_t *pcm, snd_pcm_uframes_t *buffer, snd_pcm_uframes_t *period) {
    int result=next_get_params(pcm,buffer,period);
    if (result==0) { actual_buffer=*buffer; actual_period=*period; }
    return result;
}

int snd_pcm_sw_params(snd_pcm_t *pcm, snd_pcm_sw_params_t *params) {
    int result=next_sw_params(pcm,params);
    if (result==0 && actual_period) {
        snd_pcm_uframes_t threshold=0, avail=0;
        int a=snd_pcm_sw_params_get_start_threshold(params,&threshold);
        int b=snd_pcm_sw_params_get_avail_min(params,&avail);
        if (a<0 || b<0) atomic_store(&invalid,1);
        printf("\n{\"event\":\"transport_configured\",\"rate\":48000,\"channels\":2,\"buffer_frames\":%lu,\"period_frames\":%lu,\"start_threshold_frames\":%lu,\"avail_min_frames\":%lu}\n",
            actual_buffer,actual_period,threshold,avail);
        fflush(stdout);
    }
    return result;
}

int pa_stream_write(pa_stream *s,const void *data,size_t length,pa_free_cb_t free_cb,int64_t offset,pa_seek_mode_t seek) {
    struct slot *entry=slot(s);
    if (!entry) return next_write(s,data,length,free_cb,offset,seek);
    if (entry->written==0) attributes(s,entry);
    uint64_t witness=atomic_load(&pcm_count);
    const pa_sample_spec *spec=pa_stream_get_sample_spec(s);
    size_t frames=length/8;
    if (role_id!=1 || !spec || spec->format!=PA_SAMPLE_FLOAT32LE || spec->rate!=48000 || spec->channels!=2 || length%8) {
        atomic_store(&invalid,1);
    } else {
        witness=atomic_fetch_add(&pcm_count,frames);
        if (witness+frames>MAX_FRAMES) atomic_store(&overflow,1);
        else {
            const unsigned char *bytes=data;
            for (size_t i=0;i<frames;i++) memcpy(&pcm[witness+i],bytes+8*i,4);
        }
    }
    if (role_id==1 && !atomic_load(&invalid)) {
        const float *samples=data;
        uint64_t nonzero=0;
        for (size_t i=0;i<frames;i++) {
            if (samples[2*i]!=samples[2*i+1]) atomic_store(&invalid,1);
            nonzero+=samples[2*i]!=0;
        }
        if (nonzero && source_frames==230400) {
            if (!actual_period || 230400%actual_period || nonzero!=frames ||
                atomic_exchange(&armed,true) || atomic_load(&hold_count)) atomic_store(&invalid,1);
            armed_source=source_frames;
            EVENT(ARM,entry->id,source_frames,last_completion_ns);
        }
        if (source_frames<230400 && source_frames+nonzero>230400) atomic_store(&invalid,1);
        source_frames+=nonzero;
    }
    EVENT(WRITE_BEGIN,entry->id,length,offset,seek,entry->written,witness,frames,pa_stream_get_underflow_index(s));
    bool held=atomic_exchange(&armed,false);
    if (held) {
        atomic_fetch_add(&hold_count,1);
        uint64_t before=now_ns(), target=last_completion_ns+40000000ULL;
        struct timespec deadline={.tv_sec=(time_t)(target/1000000000ULL),.tv_nsec=(long)(target%1000000000ULL)};
        int result=0;
        while (now_ns()<target && (result=clock_nanosleep(CLOCK_MONOTONIC,TIMER_ABSTIME,&deadline,NULL))==EINTR) {}
        EVENT(HOLD,entry->id,armed_source,before,target,now_ns(),result,before<target);
    }
    uint64_t previous=last_completion_ns;
    uint64_t total_before=entry->written;
    int result=next_write(s,data,length,free_cb,offset,seek);
    uint64_t complete=now_ns();
    if (result>=0) entry->written+=length;
    EVENT(WRITE_END,entry->id,length,result,total_before,entry->written,witness,frames,
          pa_stream_get_underflow_index(s),held,previous?complete-previous:0,complete);
    if (result>=0) last_completion_ns=complete;
    return result;
}

int pa_stream_peek(pa_stream *s,const void **data,size_t *length) {
    int result=next_peek(s,data,length);
    struct slot *entry=slot(s);
    if (!entry) return result;
    if (result==0 && *length>0) {
        if (!entry->pending) {
            entry->pending=true; entry->pending_length=*length; entry->pending_pointer=*data;
        } else if (entry->pending_length!=*length || entry->pending_pointer!=*data) atomic_store(&invalid,1);
    }
    EVENT(PEEK,entry->id,result,result==0?*length:0,result==0 && *data==NULL,entry->cursor,entry->pending_length);
    return result;
}

int pa_stream_drop(pa_stream *s) {
    struct slot *entry=slot(s);
    uint64_t before=entry?entry->cursor:0, length=entry?entry->pending_length:0;
    bool pending=entry && entry->pending;
    bool hole=pending && entry->pending_pointer==NULL;
    int result=next_drop(s);
    if (entry && result==0 && pending) {
        entry->cursor+=length; entry->pending=false; entry->pending_length=0; entry->pending_pointer=NULL;
    }
    if (entry) EVENT(DROP,entry->id,result,before,length,entry->cursor,pending,hole);
    return result;
}

int pa_stream_disconnect(pa_stream *s) {
    int result=next_disconnect(s);
    struct slot *entry=slot(s);
    if (entry) { EVENT(DISCONNECT,entry->id,result,entry->written,entry->cursor); if (result==0) entry->disconnected=true; }
    return result;
}

static void write_all(int fd,const void *data,size_t length) {
    const unsigned char *p=data;
    while (length) {
        ssize_t n=write(fd,p,length);
        if (n<0 && errno==EINTR) continue;
        if (n<=0) _exit(126);
        p+=n; length-=(size_t)n;
    }
}

__attribute__((constructor)) static void begin(void) {
    const char *role=getenv("HITO_OBSERVER_ROLE"), *directory=getenv("HITO_OBSERVER_DIR");
    if (!role || !directory) _exit(125);
    const char *activation=getenv("HITO_NATIVE_BUFFER_PROBE"), *owner=getenv("HITO_NATIVE_BUFFER_OWNER_EXE");
    const char *server=getenv("PULSE_SERVER");
    char executable[1024];
    ssize_t executable_length=readlink("/proc/self/exe",executable,sizeof(executable)-1);
    if (!activation || strcmp(activation,"1") || !owner || !server ||
        strncmp(server,"unix:/tmp/hito-audio-",20) || getuid()!=10001 || executable_length<1) _exit(125);
    executable[executable_length]=0;
    if (strcmp(executable,owner)) _exit(125);
    role_id=!strcmp(role,"writer")?1:!strcmp(role,"client")?2:!strcmp(role,"sink")?3:0;
    if (!role_id || directory[0]!='/') _exit(125);
    char filename[1024];
    if (snprintf(filename,sizeof(filename),"%s/%s.observer.bin",directory,role)>=(int)sizeof(filename)) _exit(125);
    ledger_fd=open(filename,O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC,0600);
    if (ledger_fd<0) _exit(125);
    if (role_id==1) {
        if (snprintf(filename,sizeof(filename),"%s/writer-input.f32",directory)>=(int)sizeof(filename)) _exit(125);
        pcm_fd=open(filename,O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC,0600);
        if (pcm_fd<0) _exit(125);
    }
    *(void**)(&next_get_params)=dlsym(RTLD_NEXT,"snd_pcm_get_params");
    *(void**)(&next_sw_params)=dlsym(RTLD_NEXT,"snd_pcm_sw_params");
    *(void**)(&next_write)=dlsym(RTLD_NEXT,"pa_stream_write");
    *(void**)(&next_underflow)=dlsym(RTLD_NEXT,"pa_stream_set_underflow_callback");
    *(void**)(&next_peek)=dlsym(RTLD_NEXT,"pa_stream_peek");
    *(void**)(&next_drop)=dlsym(RTLD_NEXT,"pa_stream_drop");
    *(void**)(&next_disconnect)=dlsym(RTLD_NEXT,"pa_stream_disconnect");
    if (role_id==1 && (!next_get_params || !next_sw_params)) _exit(125);
    if (!next_write || !next_underflow || !next_peek || !next_drop || !next_disconnect) _exit(125);
}

__attribute__((destructor)) static void finish(void) {
    if (ledger_fd<0) return;
    uint64_t n=atomic_load(&record_count), used=atomic_load(&pcm_count);
    uint64_t header[]={1,role_id,(uint64_t)getpid(),n,atomic_load(&invalid),atomic_load(&overflow),used,
        atomic_load(&hold_count),CLOCK_MONOTONIC};
    write_all(ledger_fd,"HITOPULS",8); write_all(ledger_fd,header,sizeof(header));
    for (uint64_t i=0;i<n && i<MAX_RECORDS;i++) {
        struct record *r=&records[i];
        uint64_t prefix[]={i+1,atomic_load_explicit(&r->committed,memory_order_acquire),r->kind,r->stream,r->at_ns};
        write_all(ledger_fd,prefix,sizeof(prefix)); write_all(ledger_fd,r->value,sizeof(r->value));
    }
    write_all(ledger_fd,"EXITDONE",8); close(ledger_fd);
    if (pcm_fd>=0) { write_all(pcm_fd,pcm,(used<MAX_FRAMES?used:MAX_FRAMES)*sizeof(float)); close(pcm_fd); }
}
