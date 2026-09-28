#pragma once

#include <cstddef>
#include <span>
#include <string>
#include <string_view>
#include <vector>

namespace fishmuse::foobar {

inline constexpr std::size_t max_frame_size = 1024U * 1024U;

[[nodiscard]] std::vector<std::byte> encode_frame(std::string_view payload);

class frame_decoder final {
public:
    [[nodiscard]] std::vector<std::string> push(std::span<const std::byte> bytes);
    void finish() const;

private:
    std::vector<std::byte> buffer_;
};

}  // namespace fishmuse::foobar
