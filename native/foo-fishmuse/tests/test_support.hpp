#pragma once

#include <stdexcept>
#include <string>
#include <string_view>

namespace fishmuse::test {

inline void require(bool condition, std::string_view message) {
    if (!condition) {
        throw std::runtime_error(std::string(message));
    }
}

template <typename Callable>
void require_protocol_error(Callable&& callable, std::string_view expected_code) {
    try {
        callable();
    } catch (const std::exception& error) {
        require(std::string_view(error.what()).starts_with(expected_code),
                "unexpected protocol error code");
        return;
    }
    throw std::runtime_error("expected protocol error");
}

}  // namespace fishmuse::test
