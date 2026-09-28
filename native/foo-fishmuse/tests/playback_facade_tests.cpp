#include "playback_facade.hpp"
#include "protocol.hpp"
#include "test_support.hpp"

#include <chrono>
#include <condition_variable>
#include <filesystem>
#include <fstream>
#include <functional>
#include <future>
#include <iterator>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

namespace {

using fishmuse::test::require;
using fishmuse::test::require_protocol_error;

std::string read_vector(const std::string_view name) {
    const auto path = std::filesystem::path(FISHMUSE_PROTOCOL_DIR) / "vectors" / name;
    std::ifstream input(path, std::ios::binary);
    require(input.good(), "golden vector must be readable");
    return {std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>()};
}

fishmuse::foobar::envelope command(const nlohmann::json& command_payload) {
    auto document = nlohmann::json::parse(read_vector("play.request.json"));
    document["payload"]["command"] = command_payload;
    return fishmuse::foobar::decode_json(document.dump());
}

class fake_dispatcher final : public fishmuse::foobar::main_thread_dispatcher {
public:
    void invoke(std::function<void()> action) override {
        require(!inside_, "main-thread dispatch must not nest");
        inside_ = true;
        action();
        inside_ = false;
        ++invocations;
    }

    void clear_pending() noexcept override {
        ++clear_calls;
    }

    [[nodiscard]] bool inside() const noexcept { return inside_; }

    std::size_t invocations = 0;
    std::size_t clear_calls = 0;

private:
    bool inside_ = false;
};

class fake_backend final : public fishmuse::foobar::playback_backend {
public:
    explicit fake_backend(const fake_dispatcher& dispatcher) : dispatcher_(dispatcher) {}

    void play(const fishmuse::foobar::play_request& request) override {
        record("play");
        require(request.track_id == "0199a1b2-c3d4-7004-8000-000000000001",
                "play must retain TrackId");
        require(request.path == "C:\\FishMuseFixture\\Waltz for Debby.flac",
                "play must retain the trusted local UTF-8 path");
        state_.status = fishmuse::foobar::playback_status::playing;
        state_.track_id = request.track_id;
        ++state_.revision;
    }

    void pause() override {
        record("pause");
        state_.status = fishmuse::foobar::playback_status::paused;
        ++state_.revision;
    }

    void resume() override {
        record("resume");
        state_.status = fishmuse::foobar::playback_status::playing;
        ++state_.revision;
    }

    void stop() override {
        record("stop");
        state_.status = fishmuse::foobar::playback_status::stopped;
        state_.track_id.reset();
        ++state_.revision;
    }

    void seek(const std::uint64_t position_ms) override {
        record("seek");
        require(position_ms == 12345U, "seek must retain positionMs");
        state_.position_ms = position_ms;
        ++state_.revision;
    }

    void skip_next() override {
        record("skip_next");
        ++state_.revision;
    }

    void set_volume(const double volume) override {
        record("set_volume");
        require(volume == 0.5, "set_volume must retain normalized volume");
        state_.volume = volume;
        ++state_.revision;
    }

    [[nodiscard]] fishmuse::foobar::playback_snapshot snapshot() override {
        record("snapshot");
        return state_;
    }

    std::vector<std::string> calls;

private:
    void record(std::string name) {
        require(dispatcher_.inside(), "every backend call must run through main-thread dispatcher");
        calls.push_back(std::move(name));
    }

    const fake_dispatcher& dispatcher_;
    fishmuse::foobar::playback_snapshot state_{
        "0199a1b2-c3d4-7002-8000-000000000001",
        0U,
        fishmuse::foobar::playback_status::stopped,
        std::nullopt,
        0U,
        411000U,
        0.8,
    };
};

class blocking_dispatcher final : public fishmuse::foobar::main_thread_dispatcher {
public:
    void invoke(std::function<void()> action) override {
        std::unique_lock lock(mutex_);
        entered_ = true;
        changed_.notify_all();
        changed_.wait(lock, [this] { return released_; });
        if (cancelled_) {
            throw std::runtime_error("backend_unavailable: pending work cancelled");
        }
        lock.unlock();
        action();
    }

    void clear_pending() noexcept override {
        std::scoped_lock lock(mutex_);
        cancelled_ = true;
        released_ = true;
        changed_.notify_all();
    }

    void wait_until_entered() {
        std::unique_lock lock(mutex_);
        changed_.wait(lock, [this] { return entered_; });
    }

    void release_for_cleanup() noexcept {
        std::scoped_lock lock(mutex_);
        released_ = true;
        changed_.notify_all();
    }

private:
    std::mutex mutex_;
    std::condition_variable changed_;
    bool entered_ = false;
    bool released_ = false;
    bool cancelled_ = false;
};

class passive_backend final : public fishmuse::foobar::playback_backend {
public:
    void play(const fishmuse::foobar::play_request&) override {}
    void pause() override {}
    void resume() override {}
    void stop() override {}
    void seek(std::uint64_t) override {}
    void skip_next() override {}
    void set_volume(double) override {}
    [[nodiscard]] fishmuse::foobar::playback_snapshot snapshot() override {
        return {
            "0199a1b2-c3d4-7002-8000-000000000001",
            0U,
            fishmuse::foobar::playback_status::stopped,
            std::nullopt,
            0U,
            std::nullopt,
            1.0,
        };
    }
};

void commands_are_marshalled_and_return_snapshot() {
    auto dispatcher = std::make_shared<fake_dispatcher>();
    auto backend = std::make_shared<fake_backend>(*dispatcher);
    fishmuse::foobar::playback_facade facade(backend, dispatcher);

    const auto play = fishmuse::foobar::decode_json(read_vector("play.request.json"));
    const auto playing = facade.execute(play);
    require(playing.status == fishmuse::foobar::playback_status::playing,
            "play must return authoritative playing snapshot");

    static_cast<void>(facade.execute(command({{"name", "pause"}})));
    static_cast<void>(facade.execute(command({{"name", "resume"}})));
    static_cast<void>(facade.execute(command({{"name", "seek"}, {"positionMs", 12345U}})));
    static_cast<void>(facade.execute(command({{"name", "skip_next"}})));
    static_cast<void>(facade.execute(command({{"name", "set_volume"}, {"volume", 0.5}})));
    static_cast<void>(facade.execute(command({{"name", "get_state"}})));
    const auto stopped = facade.execute(command({{"name", "stop"}}));

    require(stopped.status == fishmuse::foobar::playback_status::stopped,
            "stop must return authoritative stopped snapshot");
    require(dispatcher->invocations == 8U, "each command must dispatch exactly once");
    require(backend->calls ==
                std::vector<std::string>{
                    "play", "snapshot", "pause", "snapshot", "resume", "snapshot",
                    "seek", "snapshot", "skip_next", "snapshot", "set_volume", "snapshot",
                    "snapshot", "stop", "snapshot",
                },
            "facade must perform one side effect and one snapshot per command");
}

void shutdown_rejects_new_work_and_clears_dispatcher() {
    auto dispatcher = std::make_shared<fake_dispatcher>();
    auto backend = std::make_shared<fake_backend>(*dispatcher);
    fishmuse::foobar::playback_facade facade(backend, dispatcher);
    facade.begin_shutdown();
    require(dispatcher->clear_calls == 1U, "shutdown must clear pending main-thread work once");
    require_protocol_error(
        [&] { static_cast<void>(facade.execute(command({{"name", "pause"}}))); },
        "backend_unavailable");
    require(dispatcher->invocations == 0U, "shutdown must reject before dispatch");
}

void shutdown_cancels_a_pipe_request_without_waiting_for_the_main_thread() {
    using namespace std::chrono_literals;

    auto dispatcher = std::make_shared<blocking_dispatcher>();
    auto backend = std::make_shared<passive_backend>();
    fishmuse::foobar::playback_facade facade(backend, dispatcher);

    auto request = std::async(std::launch::async, [&] {
        static_cast<void>(facade.execute(command({{"name", "get_state"}})));
    });
    dispatcher->wait_until_entered();
    auto shutdown = std::async(std::launch::async, [&] { facade.begin_shutdown(); });

    const auto shutdown_status = shutdown.wait_for(250ms);
    if (shutdown_status != std::future_status::ready) {
        dispatcher->release_for_cleanup();
    }
    try {
        request.get();
    } catch (const std::runtime_error&) {
    }
    shutdown.get();

    require(shutdown_status == std::future_status::ready,
            "shutdown must cancel a queued pipe request without waiting on its facade lock");
}

}  // namespace

void run_playback_facade_tests() {
    commands_are_marshalled_and_return_snapshot();
    shutdown_rejects_new_work_and_clears_dispatcher();
    shutdown_cancels_a_pipe_request_without_waiting_for_the_main_thread();
}
