#include "framing.hpp"
#include "pipe_security.hpp"
#include "pipe_server.hpp"
#include "test_support.hpp"

#include <algorithm>
#include <atomic>
#include <chrono>
#include <filesystem>
#include <fstream>
#include <iterator>
#include <optional>
#include <string>
#include <thread>
#include <vector>

#include <sddl.h>

namespace {

using fishmuse::test::require;

class unique_handle final {
public:
    explicit unique_handle(const HANDLE value = INVALID_HANDLE_VALUE) : value_(value) {}
    unique_handle(const unique_handle&) = delete;
    unique_handle& operator=(const unique_handle&) = delete;
    ~unique_handle() {
        if (value_ != INVALID_HANDLE_VALUE && value_ != nullptr) {
            CloseHandle(value_);
        }
    }

    [[nodiscard]] HANDLE get() const noexcept { return value_; }

private:
    HANDLE value_;
};

std::string read_vector(const std::string_view name) {
    const auto path = std::filesystem::path(FISHMUSE_PROTOCOL_DIR) / "vectors" / name;
    std::ifstream input(path, std::ios::binary);
    require(input.good(), "golden vector must be readable");
    return {std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>()};
}

void write_frame(const HANDLE pipe, const std::string_view payload) {
    const auto frame = fishmuse::foobar::encode_frame(payload);
    DWORD written = 0;
    if (WriteFile(pipe, frame.data(), static_cast<DWORD>(frame.size()), &written, nullptr) ==
        FALSE) {
        const auto kind = payload.find("handshake.request") != std::string_view::npos
                              ? "handshake"
                              : "command";
        throw std::runtime_error(std::string("client ") + kind +
                                 " frame write failed with Win32 error " +
                                 std::to_string(GetLastError()));
    }
    require(written == frame.size(), "client frame write must be complete");
}

unique_handle connect_client(const std::wstring& pipe_name) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(2);
    while (std::chrono::steady_clock::now() < deadline) {
        const auto pipe = CreateFileW(pipe_name.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                                      OPEN_EXISTING, 0, nullptr);
        if (pipe != INVALID_HANDLE_VALUE) {
            return unique_handle(pipe);
        }
        const auto error = GetLastError();
        require(error == ERROR_FILE_NOT_FOUND || error == ERROR_PIPE_BUSY,
                "client connection failed unexpectedly");
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    throw std::runtime_error("timed out connecting to test pipe");
}

bool wait_until_disconnected(const HANDLE pipe) {
    const auto deadline = std::chrono::steady_clock::now() + std::chrono::seconds(2);
    while (std::chrono::steady_clock::now() < deadline) {
        DWORD available = 0;
        if (!PeekNamedPipe(pipe, nullptr, 0, nullptr, &available, nullptr)) {
            const auto error = GetLastError();
            return error == ERROR_BROKEN_PIPE || error == ERROR_PIPE_NOT_CONNECTED;
        }
        std::this_thread::sleep_for(std::chrono::milliseconds(10));
    }
    return false;
}

void require_real_dacl(const fishmuse::foobar::pipe_security& security) {
    BOOL present = FALSE;
    BOOL defaulted = FALSE;
    PACL dacl = nullptr;
    require(GetSecurityDescriptorDacl(security.attributes().lpSecurityDescriptor, &present, &dacl,
                                      &defaulted) != FALSE,
            "security descriptor DACL must be readable");
    require(present != FALSE && dacl != nullptr, "pipe must have an explicit DACL");

    ACL_SIZE_INFORMATION information{};
    require(GetAclInformation(dacl, &information, sizeof(information), AclSizeInformation) != FALSE,
            "pipe DACL metadata must be readable");
    require(information.AceCount == 2U, "pipe DACL must contain exactly two ACEs");

    std::vector<std::wstring> sids;
    for (DWORD index = 0; index < information.AceCount; ++index) {
        void* raw_ace = nullptr;
        require(GetAce(dacl, index, &raw_ace) != FALSE, "pipe DACL ACE must be readable");
        const auto* header = static_cast<const ACE_HEADER*>(raw_ace);
        require(header->AceType == ACCESS_ALLOWED_ACE_TYPE,
                "pipe DACL must contain allow ACEs only");
        require((header->AceFlags & INHERITED_ACE) == 0U,
                "pipe DACL must not inherit broader permissions");
        const auto* ace = static_cast<const ACCESS_ALLOWED_ACE*>(raw_ace);
        require((ace->Mask & GENERIC_ALL) == GENERIC_ALL,
                "each allowed SID must receive full pipe access");
        LPWSTR sid_text = nullptr;
        require(ConvertSidToStringSidW(const_cast<DWORD*>(&ace->SidStart), &sid_text) != FALSE,
                "pipe DACL SID must be convertible");
        sids.emplace_back(sid_text);
        LocalFree(sid_text);
    }
    require(std::ranges::find(sids, security.current_user_sid()) != sids.end(),
            "real DACL must allow the current user");
    require(std::ranges::find(sids, L"S-1-5-18") != sids.end(),
            "real DACL must allow SYSTEM");
}

void real_connected_client_authorization(const fishmuse::foobar::pipe_security& security) {
    const unique_handle server(CreateNamedPipeW(
        security.pipe_name().c_str(), PIPE_ACCESS_DUPLEX,
        PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS, 1U,
        4096U, 4096U, 0U,
        const_cast<SECURITY_ATTRIBUTES*>(&security.attributes())));
    require(server.get() != INVALID_HANDLE_VALUE, "test server pipe must be created");
    const unique_handle client(CreateFileW(security.pipe_name().c_str(),
                                           GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                                           OPEN_EXISTING, 0, nullptr));
    require(client.get() != INVALID_HANDLE_VALUE, "test client pipe must connect");
    const auto connected = ConnectNamedPipe(server.get(), nullptr);
    require(connected != FALSE || GetLastError() == ERROR_PIPE_CONNECTED,
            "test server must observe client connection");
    constexpr char probe = 'x';
    DWORD written = 0;
    require(WriteFile(client.get(), &probe, 1U, &written, nullptr) != FALSE && written == 1U,
            "test client must write before impersonation");
    char received = 0;
    DWORD read = 0;
    require(ReadFile(server.get(), &received, 1U, &read, nullptr) != FALSE && read == 1U &&
                received == probe,
            "test server must read before impersonation");
    require(security.connected_client_is_authorized(server.get()),
            "connected current-user token must be authorized");
    static_cast<void>(DisconnectNamedPipe(server.get()));
}

void real_pipe_accepts_current_user_after_handshake() {
    using fishmuse::foobar::envelope;
    using fishmuse::foobar::named_pipe_server;

    const unique_handle two_messages(CreateEventW(nullptr, TRUE, FALSE, nullptr));
    require(two_messages.get() != nullptr, "test event must be created");
    std::atomic_uint32_t handled = 0;
    named_pipe_server server(
        fishmuse::foobar::pipe_security::for_current_user(),
        [&](const envelope&) -> std::optional<std::string> {
            if (++handled == 2U) {
                SetEvent(two_messages.get());
            }
            return std::nullopt;
        },
        std::chrono::milliseconds(500));
    const auto pipe_name = server.pipe_name();
    server.start();
    const auto client = connect_client(pipe_name);
    write_frame(client.get(), read_vector("handshake.request.json"));
    write_frame(client.get(), read_vector("play.request.json"));
    require(WaitForSingleObject(two_messages.get(), 2000U) == WAIT_OBJECT_0,
            "authorized client messages must reach the server");
    require(handled.load() == 2U, "handshake and command must each be handled once");
    server.stop();
    require(!server.running(), "pipe server must stop cleanly");
}

void command_before_handshake_and_timeout_disconnect() {
    using fishmuse::foobar::envelope;
    using fishmuse::foobar::named_pipe_server;

    std::atomic_uint32_t handled = 0;
    named_pipe_server forged(
        fishmuse::foobar::pipe_security::for_current_user(),
        [&](const envelope&) -> std::optional<std::string> {
            ++handled;
            return std::nullopt;
        },
        std::chrono::milliseconds(200));
    const auto forged_name = forged.pipe_name();
    forged.start();
    const auto forged_client = connect_client(forged_name);
    write_frame(forged_client.get(), read_vector("play.request.json"));
    require(wait_until_disconnected(forged_client.get()),
            "command before handshake must disconnect the client");
    require(handled.load() == 0U, "forged pre-handshake command must not reach handler");
    forged.stop();

    named_pipe_server silent(
        fishmuse::foobar::pipe_security::for_current_user(),
        [&](const envelope&) -> std::optional<std::string> {
            ++handled;
            return std::nullopt;
        },
        std::chrono::milliseconds(100));
    const auto silent_name = silent.pipe_name();
    silent.start();
    const auto silent_client = connect_client(silent_name);
    require(wait_until_disconnected(silent_client.get()),
            "client that omits handshake must be disconnected at deadline");
    require(handled.load() == 0U, "silent client must not reach handler");
    silent.stop();
}

}  // namespace

void run_pipe_security_tests() {
    using fishmuse::foobar::pipe_security;
    using fishmuse::test::require;

    require(
        fishmuse::foobar::pipe_name_for_sid(L"S-1-5-21-1-2-3-1001") ==
            L"\\\\.\\pipe\\FishMuse.Foobar.v1."
            L"c169ebe52e9c0ba43200ce3a6af1b392219cdaf6006bba3e879ccb699a245fae",
        "pipe-name SID hash must be SHA-256 over canonical SID UTF-8 bytes");

    const auto security = pipe_security::for_current_user();
    const auto allowed = security.allowed_sid_strings();
    require(allowed.size() == 2, "pipe DACL must contain exactly two allow ACEs");
    require(std::ranges::find(allowed, security.current_user_sid()) != allowed.end(),
            "pipe DACL must allow the current user");
    require(std::ranges::find(allowed, L"S-1-5-18") != allowed.end(),
            "pipe DACL must allow SYSTEM");
    require(security.client_sid_is_authorized(security.current_user_sid()),
            "current user token must be authorized");
    require(!security.client_sid_is_authorized(L"S-1-5-32-544"),
            "a different SID must be rejected even if it knows the pipe name");
    require(security.pipe_name().starts_with(L"\\\\.\\pipe\\FishMuse.Foobar.v1."),
            "pipe name must use the versioned current-user prefix");
    require(security.pipe_name().find(security.current_user_sid()) == std::wstring::npos,
            "pipe name must hash rather than reveal the raw SID");
    require_real_dacl(security);
    real_connected_client_authorization(security);
    real_pipe_accepts_current_user_after_handshake();
    command_before_handshake_and_timeout_disconnect();
}
