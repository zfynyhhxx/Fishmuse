#include "pipe_server.hpp"

#include "framing.hpp"

#include <algorithm>
#include <array>
#include <atomic>
#include <chrono>
#include <limits>
#include <memory>
#include <stdexcept>
#include <thread>
#include <utility>

namespace fishmuse::foobar {

operation_cache::operation_cache(const std::size_t capacity)
    : capacity_(std::max<std::size_t>(capacity, 1024U)) {}

operation_claim operation_cache::claim(std::string operation_id,
                                       std::string command_fingerprint) {
    std::scoped_lock lock(mutex_);
    if (const auto found = entries_.find(operation_id); found != entries_.end()) {
        auto& value = found->second;
        if (value.fingerprint != command_fingerprint) {
            return {operation_claim_kind::conflict, {}};
        }
        touch(value, found->first);
        if (!value.complete) {
            return {operation_claim_kind::in_flight, {}};
        }
        return {operation_claim_kind::replay, value.result};
    }

    recency_.push_front(operation_id);
    auto [inserted, was_inserted] = entries_.emplace(
        std::move(operation_id),
        entry{std::move(command_fingerprint), {}, false, recency_.begin()});
    if (!was_inserted) {
        throw std::logic_error("operation cache insertion failed");
    }
    static_cast<void>(inserted);
    return {operation_claim_kind::execute, {}};
}

void operation_cache::complete(const std::string_view operation_id,
                               const std::string_view command_fingerprint,
                               std::string result) {
    std::scoped_lock lock(mutex_);
    const auto found = entries_.find(std::string(operation_id));
    if (found == entries_.end() || found->second.fingerprint != command_fingerprint) {
        throw std::invalid_argument("operation completion does not match claim");
    }
    found->second.result = std::move(result);
    found->second.complete = true;
    touch(found->second, found->first);
    evict_completed();
}

std::size_t operation_cache::size() const {
    std::scoped_lock lock(mutex_);
    return entries_.size();
}

void operation_cache::touch(entry& value, const std::string& operation_id) {
    recency_.erase(value.recency);
    recency_.push_front(operation_id);
    value.recency = recency_.begin();
}

void operation_cache::evict_completed() {
    auto candidate = recency_.rbegin();
    while (entries_.size() > capacity_ && candidate != recency_.rend()) {
        const auto found = entries_.find(*candidate);
        if (found == entries_.end() || !found->second.complete) {
            ++candidate;
            continue;
        }
        const auto erase_position = std::next(candidate).base();
        entries_.erase(found);
        candidate = std::make_reverse_iterator(recency_.erase(erase_position));
    }
}

bounded_message_queue::bounded_message_queue(const std::size_t capacity)
    : capacity_(std::max<std::size_t>(capacity, 1U)) {}

bool bounded_message_queue::try_push_incremental(std::string message) {
    std::scoped_lock lock(mutex_);
    if (snapshot_resync_required_ || messages_.size() >= capacity_) {
        snapshot_resync_required_ = true;
        return false;
    }
    messages_.push_back(std::move(message));
    return true;
}

void bounded_message_queue::replace_with_snapshot(std::string snapshot) {
    std::scoped_lock lock(mutex_);
    messages_.clear();
    messages_.push_back(std::move(snapshot));
    snapshot_resync_required_ = false;
}

bool bounded_message_queue::snapshot_resync_required() const {
    std::scoped_lock lock(mutex_);
    return snapshot_resync_required_;
}

std::string bounded_message_queue::pop() {
    std::scoped_lock lock(mutex_);
    if (messages_.empty()) {
        throw std::out_of_range("message queue is empty");
    }
    auto message = std::move(messages_.front());
    messages_.pop_front();
    return message;
}

std::optional<std::string> bounded_message_queue::try_pop() {
    std::scoped_lock lock(mutex_);
    if (messages_.empty()) {
        return std::nullopt;
    }
    auto message = std::move(messages_.front());
    messages_.pop_front();
    return message;
}

std::size_t bounded_message_queue::size() const {
    std::scoped_lock lock(mutex_);
    return messages_.size();
}

namespace {

struct handle_closer final {
    void operator()(void* handle) const noexcept {
        if (handle != nullptr && handle != INVALID_HANDLE_VALUE) {
            CloseHandle(static_cast<HANDLE>(handle));
        }
    }
};

using unique_handle = std::unique_ptr<void, handle_closer>;

unique_handle create_event(const bool manual_reset) {
    const auto event = CreateEventW(nullptr, manual_reset ? TRUE : FALSE, FALSE, nullptr);
    if (event == nullptr) {
        throw std::runtime_error("CreateEventW failed");
    }
    return unique_handle(event);
}

DWORD wait_timeout_until(const handshake_gate::clock::time_point deadline) {
    const auto now = handshake_gate::clock::now();
    if (now >= deadline) {
        return 0U;
    }
    const auto remaining =
        std::chrono::duration_cast<std::chrono::milliseconds>(deadline - now).count();
    return static_cast<DWORD>(
        std::min<std::int64_t>(remaining, std::numeric_limits<DWORD>::max()));
}

bool wait_for_overlapped(const HANDLE pipe,
                         OVERLAPPED& overlapped,
                         const HANDLE stop_event,
                         const DWORD timeout,
                         DWORD& transferred) {
    const std::array handles = {stop_event, overlapped.hEvent};
    const auto wait = WaitForMultipleObjects(static_cast<DWORD>(handles.size()), handles.data(),
                                             FALSE, timeout);
    if (wait != WAIT_OBJECT_0 + 1U) {
        static_cast<void>(CancelIoEx(pipe, &overlapped));
        static_cast<void>(GetOverlappedResult(pipe, &overlapped, &transferred, TRUE));
        return false;
    }
    return GetOverlappedResult(pipe, &overlapped, &transferred, FALSE) != FALSE;
}

bool write_payload(const HANDLE pipe,
                   const HANDLE stop_event,
                   const std::string_view payload) {
    const auto frame = encode_frame(payload);
    std::size_t offset = 0;
    while (offset < frame.size()) {
        auto write_event = create_event(false);
        OVERLAPPED overlapped{};
        overlapped.hEvent = write_event.get();
        DWORD transferred = 0;
        const auto remaining = frame.size() - offset;
        const auto request =
            static_cast<DWORD>(std::min<std::size_t>(remaining,
                                                    std::numeric_limits<DWORD>::max()));
        const auto wrote = WriteFile(pipe, frame.data() + offset, request, nullptr, &overlapped);
        if (wrote == FALSE) {
            const auto error = GetLastError();
            if (error != ERROR_IO_PENDING ||
                !wait_for_overlapped(pipe, overlapped, stop_event, 1000U, transferred)) {
                return false;
            }
        } else if (!GetOverlappedResult(pipe, &overlapped, &transferred, TRUE)) {
            return false;
        }
        if (transferred == 0U) {
            return false;
        }
        offset += transferred;
    }
    return true;
}

}  // namespace

struct named_pipe_server::implementation final {
    implementation(pipe_security security_value,
                   message_handler handler_value,
                   const std::chrono::milliseconds timeout,
                   const std::size_t queue_capacity)
        : security(std::move(security_value)),
          handler(std::move(handler_value)),
          handshake_timeout(timeout),
          queue(queue_capacity),
          stop_event(create_event(true)),
          queue_event(create_event(false)) {
        if (!handler || handshake_timeout.count() <= 0) {
            throw std::invalid_argument("pipe server requires a handler and positive timeout");
        }
    }

    ~implementation() {
        stop();
    }

    void start() {
        bool expected = false;
        if (!running.compare_exchange_strong(expected, true)) {
            throw std::logic_error("pipe server is already running");
        }
        ResetEvent(stop_event.get());
        worker = std::thread([this] { run(); });
    }

    void stop() noexcept {
        if (worker.joinable()) {
            SetEvent(stop_event.get());
            worker.join();
        }
        running.store(false);
    }

    void run() noexcept {
        try {
            while (WaitForSingleObject(stop_event.get(), 0U) != WAIT_OBJECT_0) {
                const auto raw_pipe = CreateNamedPipeW(
                    security.pipe_name().c_str(),
                    PIPE_ACCESS_DUPLEX | FILE_FLAG_OVERLAPPED,
                    PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                    1U, 64U * 1024U, 64U * 1024U, 0U,
                    const_cast<SECURITY_ATTRIBUTES*>(&security.attributes()));
                if (raw_pipe == INVALID_HANDLE_VALUE) {
                    break;
                }
                const unique_handle pipe(raw_pipe);
                if (!connect(pipe.get())) {
                    continue;
                }
                try {
                    serve(pipe.get());
                } catch (const std::exception&) {
                }
                static_cast<void>(DisconnectNamedPipe(pipe.get()));
            }
        } catch (const std::exception&) {
        }
        running.store(false);
    }

    bool connect(const HANDLE pipe) const {
        auto connected_event = create_event(false);
        OVERLAPPED overlapped{};
        overlapped.hEvent = connected_event.get();
        if (ConnectNamedPipe(pipe, &overlapped) != FALSE) {
            return true;
        }
        const auto error = GetLastError();
        if (error == ERROR_PIPE_CONNECTED) {
            return true;
        }
        if (error != ERROR_IO_PENDING) {
            return false;
        }
        DWORD transferred = 0;
        return wait_for_overlapped(pipe, overlapped, stop_event.get(), INFINITE, transferred);
    }

    void serve(const HANDLE pipe) {
        const auto started = handshake_gate::clock::now();
        const auto deadline = started + handshake_timeout;
        handshake_gate gate(handshake_timeout, started);
        frame_decoder decoder;
        std::array<std::byte, 64U * 1024U> buffer{};
        bool authorized = false;

        while (WaitForSingleObject(stop_event.get(), 0U) != WAIT_OBJECT_0) {
            auto read_event = create_event(false);
            OVERLAPPED overlapped{};
            overlapped.hEvent = read_event.get();
            const auto reading =
                ReadFile(pipe, buffer.data(), static_cast<DWORD>(buffer.size()), nullptr, &overlapped);
            if (reading == FALSE && GetLastError() != ERROR_IO_PENDING) {
                return;
            }

            bool read_complete = reading != FALSE;
            while (!read_complete) {
                const std::array handles = {stop_event.get(), queue_event.get(), read_event.get()};
                const auto timeout = gate.complete() ? INFINITE : wait_timeout_until(deadline);
                const auto wait = WaitForMultipleObjects(static_cast<DWORD>(handles.size()),
                                                         handles.data(), FALSE, timeout);
                if (wait == WAIT_TIMEOUT || wait == WAIT_OBJECT_0) {
                    static_cast<void>(CancelIoEx(pipe, &overlapped));
                    DWORD ignored = 0;
                    static_cast<void>(GetOverlappedResult(pipe, &overlapped, &ignored, TRUE));
                    return;
                }
                if (wait == WAIT_OBJECT_0 + 1U) {
                    if (gate.complete() && !drain_queue(pipe)) {
                        static_cast<void>(CancelIoEx(pipe, &overlapped));
                        DWORD ignored = 0;
                        static_cast<void>(GetOverlappedResult(pipe, &overlapped, &ignored, TRUE));
                        return;
                    }
                    continue;
                }
                if (wait == WAIT_OBJECT_0 + 2U) {
                    read_complete = true;
                    break;
                }
                static_cast<void>(CancelIoEx(pipe, &overlapped));
                return;
            }

            DWORD transferred = 0;
            if (!GetOverlappedResult(pipe, &overlapped, &transferred, TRUE) || transferred == 0U) {
                return;
            }
            if (!authorized) {
                if (!security.connected_client_is_authorized(pipe)) {
                    return;
                }
                authorized = true;
            }
            const auto frames = decoder.push(std::span(buffer).first(transferred));
            for (const auto& frame : frames) {
                const auto message = decode_json(frame);
                gate.accept(message);
                if (const auto response = handler(message); response &&
                    !write_payload(pipe, stop_event.get(), *response)) {
                    return;
                }
            }
            if (gate.complete() && !drain_queue(pipe)) {
                return;
            }
        }
    }

    bool drain_queue(const HANDLE pipe) {
        while (const auto message = queue.try_pop()) {
            if (!write_payload(pipe, stop_event.get(), *message)) {
                return false;
            }
        }
        return true;
    }

    pipe_security security;
    message_handler handler;
    std::chrono::milliseconds handshake_timeout;
    bounded_message_queue queue;
    unique_handle stop_event;
    unique_handle queue_event;
    std::atomic_bool running = false;
    std::thread worker;
};

named_pipe_server::named_pipe_server(pipe_security security,
                                     message_handler handler,
                                     const std::chrono::milliseconds handshake_timeout,
                                     const std::size_t write_queue_capacity)
    : implementation_(std::make_unique<implementation>(
          std::move(security), std::move(handler), handshake_timeout, write_queue_capacity)) {}

named_pipe_server::~named_pipe_server() = default;

void named_pipe_server::start() {
    implementation_->start();
}

void named_pipe_server::stop() noexcept {
    implementation_->stop();
}

bool named_pipe_server::running() const noexcept {
    return implementation_->running.load();
}

const std::wstring& named_pipe_server::pipe_name() const noexcept {
    return implementation_->security.pipe_name();
}

bool named_pipe_server::publish_incremental(std::string message) {
    const auto published = implementation_->queue.try_push_incremental(std::move(message));
    if (published) {
        SetEvent(implementation_->queue_event.get());
    }
    return published;
}

void named_pipe_server::publish_snapshot(std::string snapshot) {
    implementation_->queue.replace_with_snapshot(std::move(snapshot));
    SetEvent(implementation_->queue_event.get());
}

bool named_pipe_server::snapshot_resync_required() const {
    return implementation_->queue.snapshot_resync_required();
}

}  // namespace fishmuse::foobar
