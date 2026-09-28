#pragma once

#include "pipe_server.hpp"
#include "playback_facade.hpp"
#include "protocol.hpp"

#include <cstdint>
#include <memory>
#include <optional>
#include <string>

namespace fishmuse::foobar {

class component_service final {
public:
    component_service(std::shared_ptr<playback_facade> facade,
                      std::string plugin_version,
                      std::uint32_t process_id,
                      std::string session_id);

    [[nodiscard]] std::optional<std::string> handle(const envelope& message);
    [[nodiscard]] std::string state_snapshot_message(const playback_snapshot& snapshot) const;
    [[nodiscard]] std::string playback_event_message(
        std::string event,
        const playback_snapshot& snapshot,
        std::optional<std::string> error_code) const;
    void begin_shutdown();

private:
    [[nodiscard]] std::string handshake_response(const envelope& request) const;
    [[nodiscard]] std::string command_response(const envelope& request);
    [[nodiscard]] std::string error_response(const envelope& request,
                                             std::string code,
                                             std::string message,
                                             bool retryable) const;

    std::shared_ptr<playback_facade> facade_;
    std::string plugin_version_;
    std::uint32_t process_id_;
    std::string session_id_;
    operation_cache operations_{1024U};
};

}  // namespace fishmuse::foobar
