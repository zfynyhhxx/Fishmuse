#pragma once

#include "pipe_security.hpp"
#include "protocol.hpp"

#include <chrono>
#include <cstddef>
#include <deque>
#include <functional>
#include <list>
#include <memory>
#include <mutex>
#include <optional>
#include <string>
#include <string_view>
#include <unordered_map>

namespace fishmuse::foobar {

enum class operation_claim_kind {
    execute,
    in_flight,
    replay,
    conflict,
};

struct operation_claim final {
    operation_claim_kind kind;
    std::string result;
};

class operation_cache final {
public:
    explicit operation_cache(std::size_t capacity);

    [[nodiscard]] operation_claim claim(std::string operation_id,
                                        std::string command_fingerprint);
    void complete(std::string_view operation_id,
                  std::string_view command_fingerprint,
                  std::string result);
    [[nodiscard]] std::size_t size() const;

private:
    struct entry final {
        std::string fingerprint;
        std::string result;
        bool complete = false;
        std::list<std::string>::iterator recency;
    };

    void touch(entry& value, const std::string& operation_id);
    void evict_completed();

    const std::size_t capacity_;
    mutable std::mutex mutex_;
    std::list<std::string> recency_;
    std::unordered_map<std::string, entry> entries_;
};

class bounded_message_queue final {
public:
    explicit bounded_message_queue(std::size_t capacity);

    [[nodiscard]] bool try_push_incremental(std::string message);
    void replace_with_snapshot(std::string snapshot);
    [[nodiscard]] bool snapshot_resync_required() const;
    [[nodiscard]] std::string pop();
    [[nodiscard]] std::optional<std::string> try_pop();
    [[nodiscard]] std::size_t size() const;

private:
    const std::size_t capacity_;
    mutable std::mutex mutex_;
    std::deque<std::string> messages_;
    bool snapshot_resync_required_ = false;
};

class named_pipe_server final {
public:
    using message_handler = std::function<std::optional<std::string>(const envelope&)>;

    named_pipe_server(pipe_security security,
                      message_handler handler,
                      std::chrono::milliseconds handshake_timeout = std::chrono::seconds(5),
                      std::size_t write_queue_capacity = 64U);
    named_pipe_server(const named_pipe_server&) = delete;
    named_pipe_server& operator=(const named_pipe_server&) = delete;
    ~named_pipe_server();

    void start();
    void stop() noexcept;
    [[nodiscard]] bool running() const noexcept;
    [[nodiscard]] const std::wstring& pipe_name() const noexcept;
    [[nodiscard]] bool publish_incremental(std::string message);
    void publish_snapshot(std::string snapshot);
    [[nodiscard]] bool snapshot_resync_required() const;

private:
    struct implementation;
    std::unique_ptr<implementation> implementation_;
};

}  // namespace fishmuse::foobar
