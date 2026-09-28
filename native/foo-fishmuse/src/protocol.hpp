#pragma once

#include <chrono>
#include <cstdint>
#include <optional>
#include <string>
#include <string_view>

#include <nlohmann/json.hpp>

namespace fishmuse::foobar {

inline constexpr std::uint16_t protocol_version = 1;

struct envelope final {
    nlohmann::json document;
    std::string kind;
};

[[nodiscard]] envelope decode_json(std::string_view input);
[[nodiscard]] std::string encode_json(const envelope& value);

class handshake_gate final {
public:
    using clock = std::chrono::steady_clock;

    explicit handshake_gate(
        std::chrono::milliseconds timeout = std::chrono::seconds(5),
        clock::time_point started = clock::now()) noexcept;

    void accept(const envelope& value, clock::time_point now = clock::now());
    [[nodiscard]] bool complete() const noexcept;
    [[nodiscard]] bool expired(clock::time_point now = clock::now()) const noexcept;

private:
    std::chrono::milliseconds timeout_;
    clock::time_point started_;
    bool complete_ = false;
};

class sequence_tracker final {
public:
    [[nodiscard]] bool accept(std::uint64_t sequence) noexcept;
    [[nodiscard]] std::optional<std::uint64_t> last_applied() const noexcept;
    void reset() noexcept;

private:
    std::optional<std::uint64_t> last_applied_;
};

}  // namespace fishmuse::foobar
