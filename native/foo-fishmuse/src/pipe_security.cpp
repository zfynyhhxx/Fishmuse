#include "pipe_security.hpp"

#include <bcrypt.h>
#include <sddl.h>

#include <array>
#include <iomanip>
#include <memory>
#include <sstream>
#include <stdexcept>
#include <vector>

namespace fishmuse::foobar {
namespace {

constexpr std::wstring_view system_sid = L"S-1-5-18";

struct handle_closer final {
    void operator()(void* handle) const noexcept {
        if (handle != nullptr && handle != INVALID_HANDLE_VALUE) {
            CloseHandle(static_cast<HANDLE>(handle));
        }
    }
};

using unique_handle = std::unique_ptr<void, handle_closer>;

[[noreturn]] void win32_fail(const char* operation) {
    throw std::runtime_error(std::string(operation) + " failed with Win32 error " +
                             std::to_string(GetLastError()));
}

std::wstring sid_string_from_token(const HANDLE token) {
    DWORD size = 0;
    static_cast<void>(GetTokenInformation(token, TokenUser, nullptr, 0, &size));
    if (size == 0U || GetLastError() != ERROR_INSUFFICIENT_BUFFER) {
        win32_fail("GetTokenInformation(size)");
    }
    std::vector<std::byte> buffer(size);
    if (!GetTokenInformation(token, TokenUser, buffer.data(), size, &size)) {
        win32_fail("GetTokenInformation(TokenUser)");
    }
    const auto* user = reinterpret_cast<const TOKEN_USER*>(buffer.data());
    LPWSTR text = nullptr;
    if (!ConvertSidToStringSidW(user->User.Sid, &text)) {
        win32_fail("ConvertSidToStringSidW");
    }
    const std::wstring result(text);
    LocalFree(text);
    return result;
}

std::wstring current_process_sid() {
    HANDLE raw_token = nullptr;
    if (!OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw_token)) {
        win32_fail("OpenProcessToken");
    }
    const unique_handle token(raw_token);
    return sid_string_from_token(token.get());
}

std::string utf8(const std::wstring_view value) {
    if (value.empty()) {
        return {};
    }
    const auto required = WideCharToMultiByte(
        CP_UTF8, WC_ERR_INVALID_CHARS, value.data(), static_cast<int>(value.size()),
        nullptr, 0, nullptr, nullptr);
    if (required <= 0) {
        win32_fail("WideCharToMultiByte(size)");
    }
    std::string result(static_cast<std::size_t>(required), '\0');
    if (WideCharToMultiByte(CP_UTF8, WC_ERR_INVALID_CHARS, value.data(),
                            static_cast<int>(value.size()), result.data(), required,
                            nullptr, nullptr) != required) {
        win32_fail("WideCharToMultiByte(convert)");
    }
    return result;
}

std::wstring sha256_hex(const std::string_view value) {
    BCRYPT_ALG_HANDLE algorithm = nullptr;
    if (!BCRYPT_SUCCESS(
            BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0U))) {
        throw std::runtime_error("BCryptOpenAlgorithmProvider failed");
    }

    DWORD object_size = 0;
    DWORD result_size = 0;
    DWORD hash_size = 0;
    if (!BCRYPT_SUCCESS(BCryptGetProperty(algorithm, BCRYPT_OBJECT_LENGTH,
                                          reinterpret_cast<PUCHAR>(&object_size),
                                          sizeof(object_size), &result_size, 0U)) ||
        !BCRYPT_SUCCESS(BCryptGetProperty(algorithm, BCRYPT_HASH_LENGTH,
                                          reinterpret_cast<PUCHAR>(&hash_size),
                                          sizeof(hash_size), &result_size, 0U))) {
        BCryptCloseAlgorithmProvider(algorithm, 0U);
        throw std::runtime_error("BCryptGetProperty failed");
    }

    std::vector<UCHAR> object(object_size);
    std::vector<UCHAR> hash(hash_size);
    BCRYPT_HASH_HANDLE hash_handle = nullptr;
    const auto created = BCryptCreateHash(algorithm, &hash_handle, object.data(), object_size,
                                          nullptr, 0U, 0U);
    const auto hashed = created >= 0
                            ? BCryptHashData(hash_handle,
                                             reinterpret_cast<PUCHAR>(
                                                 const_cast<char*>(value.data())),
                                             static_cast<ULONG>(value.size()), 0U)
                            : created;
    const auto finished = hashed >= 0
                              ? BCryptFinishHash(hash_handle, hash.data(), hash_size, 0U)
                              : hashed;
    if (hash_handle != nullptr) {
        BCryptDestroyHash(hash_handle);
    }
    BCryptCloseAlgorithmProvider(algorithm, 0U);
    if (!BCRYPT_SUCCESS(finished)) {
        throw std::runtime_error("SHA-256 hashing failed");
    }

    std::wostringstream output;
    output << std::hex << std::setfill(L'0');
    for (const auto byte : hash) {
        output << std::setw(2) << static_cast<unsigned int>(byte);
    }
    return output.str();
}

PSECURITY_DESCRIPTOR create_descriptor(const std::wstring_view user_sid) {
    const std::wstring sddl = L"D:P(A;;GA;;;SY)(A;;GA;;;" + std::wstring(user_sid) + L")";
    PSECURITY_DESCRIPTOR descriptor = nullptr;
    if (!ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl.c_str(), SDDL_REVISION_1, &descriptor, nullptr)) {
        win32_fail("ConvertStringSecurityDescriptorToSecurityDescriptorW");
    }
    return descriptor;
}

class impersonation_guard final {
public:
    explicit impersonation_guard(const HANDLE pipe_handle) {
        if (!ImpersonateNamedPipeClient(pipe_handle)) {
            win32_fail("ImpersonateNamedPipeClient");
        }
        active_ = true;
    }

    impersonation_guard(const impersonation_guard&) = delete;
    impersonation_guard& operator=(const impersonation_guard&) = delete;

    ~impersonation_guard() {
        if (active_) {
            static_cast<void>(RevertToSelf());
        }
    }

private:
    bool active_ = false;
};

}  // namespace

std::wstring pipe_name_for_sid(const std::wstring_view sid) {
    if (sid.empty()) {
        throw std::invalid_argument("pipe name requires a SID");
    }
    return std::wstring(L"\\\\.\\pipe\\FishMuse.Foobar.v1.") + sha256_hex(utf8(sid));
}

pipe_security pipe_security::for_current_user() {
    auto sid = current_process_sid();
    auto descriptor = create_descriptor(sid);
    auto name = pipe_name_for_sid(sid);
    return pipe_security(std::move(sid), std::move(name), descriptor);
}

pipe_security::pipe_security(std::wstring current_user_sid,
                             std::wstring pipe_name,
                             PSECURITY_DESCRIPTOR descriptor)
    : current_user_sid_(std::move(current_user_sid)),
      pipe_name_(std::move(pipe_name)),
      descriptor_(descriptor),
      attributes_{sizeof(SECURITY_ATTRIBUTES), descriptor_, FALSE} {}

pipe_security::pipe_security(pipe_security&& other) noexcept
    : current_user_sid_(std::move(other.current_user_sid_)),
      pipe_name_(std::move(other.pipe_name_)),
      descriptor_(other.descriptor_),
      attributes_{sizeof(SECURITY_ATTRIBUTES), descriptor_, FALSE} {
    other.descriptor_ = nullptr;
    other.attributes_.lpSecurityDescriptor = nullptr;
}

pipe_security& pipe_security::operator=(pipe_security&& other) noexcept {
    if (this != &other) {
        release();
        current_user_sid_ = std::move(other.current_user_sid_);
        pipe_name_ = std::move(other.pipe_name_);
        descriptor_ = other.descriptor_;
        attributes_ = {sizeof(SECURITY_ATTRIBUTES), descriptor_, FALSE};
        other.descriptor_ = nullptr;
        other.attributes_.lpSecurityDescriptor = nullptr;
    }
    return *this;
}

pipe_security::~pipe_security() {
    release();
}

void pipe_security::release() noexcept {
    if (descriptor_ != nullptr) {
        LocalFree(descriptor_);
        descriptor_ = nullptr;
        attributes_.lpSecurityDescriptor = nullptr;
    }
}

const std::wstring& pipe_security::current_user_sid() const noexcept {
    return current_user_sid_;
}

std::vector<std::wstring> pipe_security::allowed_sid_strings() const {
    return {current_user_sid_, std::wstring(system_sid)};
}

bool pipe_security::client_sid_is_authorized(const std::wstring_view sid) const noexcept {
    return sid == current_user_sid_;
}

bool pipe_security::connected_client_is_authorized(const HANDLE pipe_handle) const {
    impersonation_guard impersonation(pipe_handle);
    HANDLE raw_token = nullptr;
    if (!OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, TRUE, &raw_token)) {
        win32_fail("OpenThreadToken");
    }
    const unique_handle token(raw_token);
    return client_sid_is_authorized(sid_string_from_token(token.get()));
}

const std::wstring& pipe_security::pipe_name() const noexcept {
    return pipe_name_;
}

const SECURITY_ATTRIBUTES& pipe_security::attributes() const noexcept {
    return attributes_;
}

}  // namespace fishmuse::foobar
