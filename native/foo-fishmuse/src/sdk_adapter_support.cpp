#include "sdk_adapter_support.hpp"

#include <algorithm>
#include <cmath>
#include <condition_variable>
#include <exception>
#include <mutex>
#include <stdexcept>
#include <utility>
#include <vector>

namespace fishmuse::foobar {

float foobar_db_from_normalized(const double volume) {
    if (!std::isfinite(volume) || volume < 0.0 || volume > 1.0) {
        throw std::invalid_argument("normalized volume must be between zero and one");
    }
    if (volume == 0.0) {
        return -100.0F;
    }
    return static_cast<float>(std::max(-100.0, 20.0 * std::log10(volume)));
}

double normalized_from_foobar_db(const float decibels) {
    if (!std::isfinite(decibels)) {
        throw std::invalid_argument("foobar volume must be finite");
    }
    if (decibels <= -100.0F) {
        return 0.0;
    }
    if (decibels >= 0.0F) {
        return 1.0;
    }
    return std::pow(10.0, static_cast<double>(decibels) / 20.0);
}

namespace {

class queued_task final {
public:
    explicit queued_task(std::function<void()> action) : action_(std::move(action)) {}

    void run() noexcept {
        {
            std::scoped_lock lock(mutex_);
            if (state_ == state::cancelled) {
                return;
            }
            state_ = state::running;
        }
        try {
            action_();
        } catch (...) {
            error_ = std::current_exception();
        }
        {
            std::scoped_lock lock(mutex_);
            state_ = state::completed;
        }
        completed_.notify_all();
    }

    void cancel() noexcept {
        {
            std::scoped_lock lock(mutex_);
            if (state_ != state::pending) {
                return;
            }
            state_ = state::cancelled;
        }
        completed_.notify_all();
    }

    void wait() {
        std::unique_lock lock(mutex_);
        completed_.wait(lock, [this] {
            return state_ == state::completed || state_ == state::cancelled;
        });
        if (state_ == state::cancelled) {
            throw std::runtime_error("backend_unavailable: main-thread task was cancelled");
        }
        if (error_) {
            std::rethrow_exception(error_);
        }
    }

private:
    enum class state {
        pending,
        running,
        completed,
        cancelled,
    };

    std::mutex mutex_;
    std::condition_variable completed_;
    std::function<void()> action_;
    std::exception_ptr error_;
    state state_ = state::pending;
};

}  // namespace

struct queued_main_thread_dispatcher::implementation final {
    implementation(post_callback post_value, is_main_thread_callback is_main_thread_value)
        : post(std::move(post_value)), is_main_thread(std::move(is_main_thread_value)) {
        if (!post || !is_main_thread) {
            throw std::invalid_argument("main-thread dispatcher callbacks are required");
        }
    }

    void remove(const std::shared_ptr<queued_task>& task) {
        std::scoped_lock lock(mutex);
        std::erase(pending, task);
    }

    std::mutex mutex;
    post_callback post;
    is_main_thread_callback is_main_thread;
    std::vector<std::shared_ptr<queued_task>> pending;
    bool accepting = true;
};

queued_main_thread_dispatcher::queued_main_thread_dispatcher(
    post_callback post,
    is_main_thread_callback is_main_thread)
    : implementation_(
          std::make_unique<implementation>(std::move(post), std::move(is_main_thread))) {}

queued_main_thread_dispatcher::~queued_main_thread_dispatcher() {
    clear_pending();
}

void queued_main_thread_dispatcher::invoke(callback action) {
    if (!action) {
        throw std::invalid_argument("main-thread action is required");
    }

    {
        std::scoped_lock lock(implementation_->mutex);
        if (!implementation_->accepting) {
            throw std::runtime_error("backend_unavailable: component is shutting down");
        }
    }
    if (implementation_->is_main_thread()) {
        action();
        return;
    }

    auto task = std::make_shared<queued_task>(std::move(action));
    {
        std::scoped_lock lock(implementation_->mutex);
        if (!implementation_->accepting) {
            throw std::runtime_error("backend_unavailable: component is shutting down");
        }
        implementation_->pending.push_back(task);
    }

    try {
        implementation_->post([task] { task->run(); });
    } catch (...) {
        task->cancel();
        implementation_->remove(task);
        throw;
    }

    try {
        task->wait();
    } catch (...) {
        implementation_->remove(task);
        throw;
    }
    implementation_->remove(task);
}

void queued_main_thread_dispatcher::clear_pending() noexcept {
    std::vector<std::shared_ptr<queued_task>> pending;
    {
        std::scoped_lock lock(implementation_->mutex);
        implementation_->accepting = false;
        pending.swap(implementation_->pending);
    }
    for (const auto& task : pending) {
        task->cancel();
    }
}

struct deferred_event_dispatcher::implementation final {
    implementation(post_callback post_value, event_handler handler_value)
        : post(std::move(post_value)), handler(std::move(handler_value)) {
        if (!post || !handler) {
            throw std::invalid_argument("deferred event callbacks are required");
        }
    }

    std::mutex mutex;
    post_callback post;
    event_handler handler;
    bool accepting = true;
};

deferred_event_dispatcher::deferred_event_dispatcher(post_callback post,
                                                     event_handler handler)
    : implementation_(
          std::make_shared<implementation>(std::move(post), std::move(handler))) {}

deferred_event_dispatcher::~deferred_event_dispatcher() {
    clear_pending();
}

void deferred_event_dispatcher::enqueue(std::string event) {
    const auto implementation = implementation_;
    {
        std::scoped_lock lock(implementation->mutex);
        if (!implementation->accepting) {
            return;
        }
    }
    implementation->post([implementation, event = std::move(event)]() mutable {
        event_handler handler;
        {
            std::scoped_lock lock(implementation->mutex);
            if (!implementation->accepting || !implementation->handler) {
                return;
            }
            handler = implementation->handler;
        }
        handler(std::move(event));
    });
}

void deferred_event_dispatcher::clear_pending() noexcept {
    std::scoped_lock lock(implementation_->mutex);
    implementation_->accepting = false;
    implementation_->handler = {};
}

}  // namespace fishmuse::foobar
