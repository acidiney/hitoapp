#include "SoundTouch.h"
#include <cmath>
#include <cstdint>
#include <limits>
#include <memory>
#ifdef HITO_NATIVE_PROBE
#include <atomic>
static std::atomic<std::uint64_t> created{0};
static std::atomic<std::uint64_t> destroyed{0};
static std::atomic<bool> fail_next{false};
#endif

struct Processor {
    soundtouch::SoundTouch engine;
    double speed;
    std::uint64_t input_frames = 0;
    std::uint64_t output_frames = 0;
    bool drained = false;

    Processor(std::uint32_t rate, std::uint32_t channels, double value) : speed(value) {
        engine.setSampleRate(rate);
        engine.setChannels(channels);
        engine.setTempo(value);
#ifdef HITO_NATIVE_PROBE
        created.fetch_add(1, std::memory_order_relaxed);
#endif
    }

    bool flush() {
        if (drained) return true;
        const double target = std::round(static_cast<double>(input_frames) / speed);
        if (target >= static_cast<double>(std::numeric_limits<std::int64_t>::max())) return false;
        const auto expected = static_cast<std::uint64_t>(target);
        if (expected < output_frames) return false;
        const auto remaining = expected - output_frames;
        if (remaining > std::numeric_limits<unsigned int>::max()) return false;
        const int latency = engine.getSetting(SETTING_INITIAL_LATENCY);
        const int sequence = engine.getSetting(SETTING_NOMINAL_INPUT_SEQUENCE);
        if (latency < 0 || sequence <= 0) return false;
        const auto budget = static_cast<std::uint64_t>(latency) + sequence + 128;
        float silence[256] = {};
        std::uint64_t padded = 0;
        while (engine.numSamples() < remaining && padded < budget) {
            engine.putSamples(silence, 128);
            padded += 128;
        }
        if (engine.numSamples() < remaining) return false;
        if (engine.adjustAmountOfSamples(static_cast<unsigned int>(remaining)) != remaining) return false;
        drained = true;
        return true;
    }

    ~Processor() {
#ifdef HITO_NATIVE_PROBE
        destroyed.fetch_add(1, std::memory_order_relaxed);
#endif
    }
};

extern "C" {
void *hito_tempo_create(std::uint32_t rate, std::uint32_t channels, double speed) noexcept {
    if (rate == 0 || rate > 192000 || (channels != 1 && channels != 2) || !std::isfinite(speed) || speed < 0.5 || speed > 3.0) return nullptr;
    try {
        std::unique_ptr<Processor> processor(new Processor(rate, channels, speed));
#ifdef HITO_NATIVE_PROBE
        if (fail_next.exchange(false, std::memory_order_relaxed)) throw std::bad_alloc();
#endif
        return processor.release();
    } catch (...) {
        return nullptr;
    }
}

void hito_tempo_destroy(void *handle) noexcept {
    delete static_cast<Processor *>(handle);
}

bool hito_tempo_put(void *handle, const float *samples, std::uint32_t frames) noexcept {
    try {
        auto &processor = *static_cast<Processor *>(handle);
        if (processor.drained || frames > std::numeric_limits<std::uint64_t>::max() - processor.input_frames) return false;
        processor.engine.putSamples(samples, frames);
        processor.input_frames += frames;
        return true;
    } catch (...) {
        return false;
    }
}

std::int64_t hito_tempo_receive(void *handle, float *samples, std::uint32_t frames) noexcept {
    try {
        auto &processor = *static_cast<Processor *>(handle);
        const auto received = processor.engine.receiveSamples(samples, frames);
        processor.output_frames += received;
        return received;
    } catch (...) {
        return -1;
    }
}

bool hito_tempo_flush(void *handle) noexcept {
    try {
        return static_cast<Processor *>(handle)->flush();
    } catch (...) {
        return false;
    }
}

#ifdef HITO_NATIVE_PROBE
std::uint64_t hito_tempo_created() noexcept { return created.load(std::memory_order_relaxed); }
std::uint64_t hito_tempo_destroyed() noexcept { return destroyed.load(std::memory_order_relaxed); }
void hito_tempo_fail_next() noexcept { fail_next.store(true, std::memory_order_relaxed); }
#endif
}
