#include "pipe_probe.hpp"

#include "pipe_security.hpp"
#include "pipe_server.hpp"

#include <chrono>
#include <fstream>
#include <optional>
#include <stdexcept>
#include <string>
#include <thread>

namespace fishmuse::test {
namespace {

std::string utf8(const std::wstring_view value) {
    const auto required = WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                                               static_cast<int>(value.size()), nullptr, 0,
                                               nullptr, nullptr);
    if (required <= 0) {
        throw std::runtime_error("pipe name UTF-8 sizing failed");
    }
    std::string result(static_cast<std::size_t>(required), '\0');
    if (WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                            static_cast<int>(value.size()), result.data(), required,
                            nullptr, nullptr) != required) {
        throw std::runtime_error("pipe name UTF-8 conversion failed");
    }
    return result;
}

}  // namespace

int run_auth_probe_server(const std::filesystem::path& ready_file,
                          const std::filesystem::path& stop_file) {
    using fishmuse::foobar::envelope;
    using fishmuse::foobar::named_pipe_server;
    using fishmuse::foobar::pipe_security;

    named_pipe_server server(
        pipe_security::for_current_user(),
        [](const envelope&) -> std::optional<std::string> { return std::nullopt; },
        std::chrono::seconds(10));
    const auto& wide_name = server.pipe_name();
    const auto pipe_name = utf8(wide_name);
    server.start();
    {
        std::ofstream ready(ready_file, std::ios::binary | std::ios::trunc);
        if (!ready) {
            throw std::runtime_error("auth probe ready file could not be created");
        }
        ready << pipe_name;
    }

    const auto deadline = std::chrono::steady_clock::now() + std::chrono::minutes(5);
    while (!std::filesystem::exists(stop_file) && std::chrono::steady_clock::now() < deadline) {
        std::this_thread::sleep_for(std::chrono::milliseconds(25));
    }
    server.stop();
    return std::filesystem::exists(stop_file) ? 0 : 70;
}

int run_auth_probe_client(const std::wstring_view pipe_name) {
    const std::wstring name(pipe_name);
    const auto pipe = CreateFileW(name.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                                  OPEN_EXISTING, 0, nullptr);
    if (pipe != INVALID_HANDLE_VALUE) {
        CloseHandle(pipe);
        return 0;
    }
    return GetLastError() == ERROR_ACCESS_DENIED ? 23 : 24;
}

}  // namespace fishmuse::test
