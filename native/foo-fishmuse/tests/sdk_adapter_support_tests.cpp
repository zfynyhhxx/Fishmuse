#include "sdk_adapter_support.hpp"
#include "test_support.hpp"

#include <chrono>
#include <cmath>
#include <condition_variable>
#include <exception>
#include <functional>
#include <mutex>
#include <optional>
#include <string>
#include <thread>
#include <vector>

namespace {

using fishmuse::test::require;

void normalized_volume_round_trips_through_foobar_decibels() {
    using fishmuse::foobar::foobar_db_from_normalized;
    using fishmuse::foobar::normalized_from_foobar_db;

    require(foobar_db_from_normalized(1.0) == 0.0F,
            "full protocol volume must map to zero dB");
    require(foobar_db_from_normalized(0.0) == -100.0F,
            "zero protocol volume must map to foobar mute");
    require(std::abs(foobar_db_from_normalized(0.5) + 6.0206F) < 0.001F,
            "half protocol volume must use an amplitude-to-dB curve");
    require(std::abs(normalized_from_foobar_db(-6.0206F) - 0.5) < 0.0001,
            "foobar dB must invert to normalized protocol volume");
    require(normalized_from_foobar_db(-100.0F) == 0.0,
            "foobar mute must map to zero protocol volume");
}

void queued_dispatcher_runs_work_on_the_supplied_main_thread() {
    using fishmuse::foobar::queued_main_thread_dispatcher;

    std::mutex mutex;
    std::condition_variable ready;
    std::optional<std::function<void()>> queued;
    bool main_thread = false;
    bool ran_on_main_thread = false;
    std::exception_ptr worker_error;

    queued_main_thread_dispatcher dispatcher(
        [&](std::function<void()> callback) {
            {
                std::scoped_lock lock(mutex);
                queued = std::move(callback);
            }
            ready.notify_one();
        },
        [&] { return main_thread; });

    std::thread worker([&] {
        try {
            dispatcher.invoke([&] { ran_on_main_thread = main_thread; });
        } catch (...) {
            worker_error = std::current_exception();
        }
    });

    std::function<void()> callback;
    {
        std::unique_lock lock(mutex);
        require(ready.wait_for(lock, std::chrono::seconds(2), [&] { return queued.has_value(); }),
                "worker task must be posted to the main thread");
        callback = std::move(*queued);
    }
    main_thread = true;
    callback();
    worker.join();

    require(worker_error == nullptr, "main-thread dispatch must complete without an error");
    require(ran_on_main_thread, "backend action must execute on the supplied main thread");
}

void shutdown_cancels_queued_main_thread_work_without_running_it() {
    using fishmuse::foobar::queued_main_thread_dispatcher;

    std::mutex mutex;
    std::condition_variable ready;
    std::optional<std::function<void()>> queued;
    bool executed = false;
    std::string worker_error;

    queued_main_thread_dispatcher dispatcher(
        [&](std::function<void()> callback) {
            {
                std::scoped_lock lock(mutex);
                queued = std::move(callback);
            }
            ready.notify_one();
        },
        [] { return false; });

    std::thread worker([&] {
        try {
            dispatcher.invoke([&] { executed = true; });
        } catch (const std::exception& error) {
            worker_error = error.what();
        }
    });

    std::function<void()> callback;
    {
        std::unique_lock lock(mutex);
        require(ready.wait_for(lock, std::chrono::seconds(2), [&] { return queued.has_value(); }),
                "worker task must reach the pending queue");
        callback = std::move(*queued);
    }
    dispatcher.clear_pending();
    worker.join();
    callback();

    require(!executed, "shutdown must prevent a queued foobar action from running");
    require(worker_error.starts_with("backend_unavailable:"),
            "cancelled dispatch must report backend_unavailable");
}

void playback_callbacks_are_deferred_and_cancelled_on_shutdown() {
    using fishmuse::foobar::deferred_event_dispatcher;

    std::vector<std::function<void()>> queued;
    std::vector<std::string> delivered;
    deferred_event_dispatcher dispatcher(
        [&](std::function<void()> callback) { queued.push_back(std::move(callback)); },
        [&](std::string event) { delivered.push_back(std::move(event)); });

    dispatcher.enqueue("state_changed");
    require(delivered.empty(),
            "foobar playback callback must not query or publish inside the callback stack");
    require(queued.size() == 1U, "playback callback must enqueue one deferred task");
    queued.front()();
    require(delivered == std::vector<std::string>{"state_changed"},
            "deferred playback event must retain its event kind");

    dispatcher.enqueue("position");
    dispatcher.clear_pending();
    queued.back()();
    require(delivered.size() == 1U,
            "component shutdown must cancel deferred playback event publication");
}

}  // namespace

void run_sdk_adapter_support_tests() {
    normalized_volume_round_trips_through_foobar_decibels();
    queued_dispatcher_runs_work_on_the_supplied_main_thread();
    shutdown_cancels_queued_main_thread_work_without_running_it();
    playback_callbacks_are_deferred_and_cancelled_on_shutdown();
}
