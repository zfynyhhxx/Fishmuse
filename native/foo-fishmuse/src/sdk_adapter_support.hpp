#pragma once

#include "playback_facade.hpp"

#include <functional>
#include <memory>
#include <string>

namespace fishmuse::foobar {

[[nodiscard]] float foobar_db_from_normalized(double volume);
[[nodiscard]] double normalized_from_foobar_db(float decibels);

class queued_main_thread_dispatcher final : public main_thread_dispatcher {
public:
    using callback = std::function<void()>;
    using post_callback = std::function<void(callback)>;
    using is_main_thread_callback = std::function<bool()>;

    queued_main_thread_dispatcher(post_callback post,
                                  is_main_thread_callback is_main_thread);
    queued_main_thread_dispatcher(const queued_main_thread_dispatcher&) = delete;
    queued_main_thread_dispatcher& operator=(const queued_main_thread_dispatcher&) = delete;
    ~queued_main_thread_dispatcher() override;

    void invoke(callback action) override;
    void clear_pending() noexcept override;

private:
    struct implementation;
    std::unique_ptr<implementation> implementation_;
};

class deferred_event_dispatcher final {
public:
    using callback = std::function<void()>;
    using post_callback = std::function<void(callback)>;
    using event_handler = std::function<void(std::string)>;

    deferred_event_dispatcher(post_callback post, event_handler handler);
    deferred_event_dispatcher(const deferred_event_dispatcher&) = delete;
    deferred_event_dispatcher& operator=(const deferred_event_dispatcher&) = delete;
    ~deferred_event_dispatcher();

    void enqueue(std::string event);
    void clear_pending() noexcept;

private:
    struct implementation;
    std::shared_ptr<implementation> implementation_;
};

}  // namespace fishmuse::foobar
