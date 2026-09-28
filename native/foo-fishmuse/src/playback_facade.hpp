#pragma once

#include "protocol.hpp"

#include <cstdint>
#include <functional>
#include <memory>
#include <mutex>
#include <optional>
#include <string>

namespace fishmuse::foobar {

enum class playback_status {
    stopped,
    loading,
    playing,
    paused,
    unavailable,
};

struct playback_snapshot final {
    std::string session_id;
    std::uint64_t revision;
    playback_status status;
    std::optional<std::string> track_id;
    std::uint64_t position_ms;
    std::optional<std::uint64_t> duration_ms;
    double volume;
};

struct play_request final {
    std::string track_id;
    std::string path;
    std::optional<std::uint32_t> subsong_index;
    std::optional<std::uint64_t> start_ms;
    std::optional<std::uint64_t> end_ms;
};

class playback_backend {
public:
    virtual ~playback_backend() = default;

    virtual void play(const play_request& request) = 0;
    virtual void pause() = 0;
    virtual void resume() = 0;
    virtual void stop() = 0;
    virtual void seek(std::uint64_t position_ms) = 0;
    virtual void skip_next() = 0;
    virtual void set_volume(double volume) = 0;
    [[nodiscard]] virtual playback_snapshot snapshot() = 0;
};

class main_thread_dispatcher {
public:
    virtual ~main_thread_dispatcher() = default;

    virtual void invoke(std::function<void()> action) = 0;
    virtual void clear_pending() noexcept = 0;
};

class playback_facade final {
public:
    playback_facade(std::shared_ptr<playback_backend> backend,
                    std::shared_ptr<main_thread_dispatcher> dispatcher);

    [[nodiscard]] playback_snapshot execute(const envelope& request);
    void begin_shutdown();

private:
    std::shared_ptr<playback_backend> backend_;
    std::shared_ptr<main_thread_dispatcher> dispatcher_;
    std::mutex lifecycle_mutex_;
    bool accepting_ = true;
};

}  // namespace fishmuse::foobar
