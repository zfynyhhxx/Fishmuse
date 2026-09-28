#pragma once

#include <string>
#include <string_view>
#include <vector>

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>

namespace fishmuse::foobar {

[[nodiscard]] std::wstring pipe_name_for_sid(std::wstring_view sid);

class pipe_security final {
public:
    static pipe_security for_current_user();

    pipe_security(const pipe_security&) = delete;
    pipe_security& operator=(const pipe_security&) = delete;
    pipe_security(pipe_security&& other) noexcept;
    pipe_security& operator=(pipe_security&& other) noexcept;
    ~pipe_security();

    [[nodiscard]] const std::wstring& current_user_sid() const noexcept;
    [[nodiscard]] std::vector<std::wstring> allowed_sid_strings() const;
    [[nodiscard]] bool client_sid_is_authorized(std::wstring_view sid) const noexcept;
    [[nodiscard]] bool connected_client_is_authorized(HANDLE pipe_handle) const;
    [[nodiscard]] const std::wstring& pipe_name() const noexcept;
    [[nodiscard]] const SECURITY_ATTRIBUTES& attributes() const noexcept;

private:
    pipe_security(std::wstring current_user_sid,
                  std::wstring pipe_name,
                  PSECURITY_DESCRIPTOR descriptor);
    void release() noexcept;

    std::wstring current_user_sid_;
    std::wstring pipe_name_;
    PSECURITY_DESCRIPTOR descriptor_ = nullptr;
    SECURITY_ATTRIBUTES attributes_{};
};

}  // namespace fishmuse::foobar
