#include "framing.hpp"

#include <algorithm>
#include <cstdint>
#include <cstring>
#include <stdexcept>

namespace fishmuse::foobar {
namespace {

[[noreturn]] void fail(std::string_view code, std::string_view detail) {
    throw std::runtime_error(std::string(code) + ": " + std::string(detail));
}

constexpr std::size_t header_size = sizeof(std::uint32_t);

}  // namespace

std::vector<std::byte> encode_frame(const std::string_view payload) {
    if (payload.empty()) {
        fail("protocol_invalid", "frame payload must not be empty");
    }
    if (payload.size() > max_frame_size) {
        fail("frame_too_large", "frame exceeds 1 MiB");
    }

    const auto length = static_cast<std::uint32_t>(payload.size());
    std::vector<std::byte> frame(header_size + payload.size());
    frame[0] = static_cast<std::byte>(length & 0xffU);
    frame[1] = static_cast<std::byte>((length >> 8U) & 0xffU);
    frame[2] = static_cast<std::byte>((length >> 16U) & 0xffU);
    frame[3] = static_cast<std::byte>((length >> 24U) & 0xffU);
    std::memcpy(frame.data() + header_size, payload.data(), payload.size());
    return frame;
}

std::vector<std::string> frame_decoder::push(const std::span<const std::byte> bytes) {
    buffer_.insert(buffer_.end(), bytes.begin(), bytes.end());
    std::vector<std::string> frames;

    while (buffer_.size() >= header_size) {
        const auto length = static_cast<std::uint32_t>(std::to_integer<unsigned char>(buffer_[0])) |
                            (static_cast<std::uint32_t>(
                                 std::to_integer<unsigned char>(buffer_[1]))
                             << 8U) |
                            (static_cast<std::uint32_t>(
                                 std::to_integer<unsigned char>(buffer_[2]))
                             << 16U) |
                            (static_cast<std::uint32_t>(
                                 std::to_integer<unsigned char>(buffer_[3]))
                             << 24U);
        if (length == 0U) {
            buffer_.clear();
            fail("protocol_invalid", "zero-length frame");
        }
        if (length > max_frame_size) {
            buffer_.clear();
            fail("frame_too_large", "frame exceeds 1 MiB");
        }

        const auto frame_end = header_size + static_cast<std::size_t>(length);
        if (buffer_.size() < frame_end) {
            break;
        }

        const auto* payload = reinterpret_cast<const char*>(buffer_.data() + header_size);
        frames.emplace_back(payload, static_cast<std::size_t>(length));
        buffer_.erase(buffer_.begin(), buffer_.begin() + static_cast<std::ptrdiff_t>(frame_end));
    }

    return frames;
}

void frame_decoder::finish() const {
    if (!buffer_.empty()) {
        fail("protocol_invalid", "truncated frame");
    }
}

}  // namespace fishmuse::foobar
