#include "SoundTouch.h"
#include <cstdint>
#include <memory>
#ifdef HITO_NATIVE_PROBE
#include <atomic>
static std::atomic<std::uint64_t> created{0};
static std::atomic<std::uint64_t> destroyed{0};
#endif

struct Processor {
    soundtouch::SoundTouch engine;

    Processor(std::uint32_t rate, std::uint32_t channels, double speed) {
        engine.setSampleRate(rate);
        engine.setChannels(channels);
        engine.setTempo(speed);
#ifdef HITO_NATIVE_PROBE
        created.fetch_add(1, std::memory_order_relaxed);
#endif
    }

    ~Processor() {
#ifdef HITO_NATIVE_PROBE
        destroyed.fetch_add(1, std::memory_order_relaxed);
#endif
    }
};

extern "C" {
void *hito_tempo_create(std::uint32_t rate, std::uint32_t channels, double speed) noexcept {
    try {
        return new Processor(rate, channels, speed);
    } catch (...) {
        return nullptr;
    }
}

void hito_tempo_destroy(void *handle) noexcept {
    delete static_cast<Processor *>(handle);
}

bool hito_tempo_put(void *handle, const float *samples, std::uint32_t frames) noexcept {
    try {
        static_cast<Processor *>(handle)->engine.putSamples(samples, frames);
        return true;
    } catch (...) {
        return false;
    }
}

std::int64_t hito_tempo_receive(void *handle, float *samples, std::uint32_t frames) noexcept {
    try {
        return static_cast<Processor *>(handle)->engine.receiveSamples(samples, frames);
    } catch (...) {
        return -1;
    }
}

bool hito_tempo_flush(void *handle) noexcept {
    try {
        static_cast<Processor *>(handle)->engine.flush();
        return true;
    } catch (...) {
        return false;
    }
}

#ifdef HITO_NATIVE_PROBE
std::uint64_t hito_tempo_created() noexcept { return created.load(std::memory_order_relaxed); }
std::uint64_t hito_tempo_destroyed() noexcept { return destroyed.load(std::memory_order_relaxed); }
#endif
}
