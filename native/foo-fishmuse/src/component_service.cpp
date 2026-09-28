#include "component_service.hpp"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <objbase.h>

#include <algorithm>
#include <chrono>
#include <cctype>
#include <stdexcept>
#include <string_view>
#include <utility>

#include <nlohmann/json.hpp>

namespace fishmuse::foobar {
namespace {

using json = nlohmann::json;

std::string new_uuid() {
    GUID guid{};
    if (FAILED(CoCreateGuid(&guid))) {
        throw std::runtime_error("CoCreateGuid failed");
    }
    wchar_t text[39]{};
    if (StringFromGUID2(guid, text, static_cast<int>(std::size(text))) != 39) {
        throw std::runtime_error("StringFromGUID2 failed");
    }
    std::string result;
    result.reserve(36U);
    for (std::size_t index = 1U; index < 37U; ++index) {
        result.push_back(static_cast<char>(std::tolower(static_cast<unsigned char>(text[index]))));
    }
    return result;
}

std::uint64_t unix_time_ms() {
    const auto elapsed = std::chrono::system_clock::now().time_since_epoch();
    return static_cast<std::uint64_t>(
        std::chrono::duration_cast<std::chrono::milliseconds>(elapsed).count());
}

std::string status_name(const playback_status status) {
    switch (status) {
        case playback_status::stopped:
            return "stopped";
        case playback_status::loading:
            return "loading";
        case playback_status::playing:
            return "playing";
        case playback_status::paused:
            return "paused";
        case playback_status::unavailable:
            return "unavailable";
    }
    throw std::logic_error("unknown playback status");
}

json snapshot_json(const playback_snapshot& snapshot) {
    return {
        {"sessionId", snapshot.session_id},
        {"revision", snapshot.revision},
        {"status", status_name(snapshot.status)},
        {"trackId", snapshot.track_id ? json(*snapshot.track_id) : json(nullptr)},
        {"positionMs", snapshot.position_ms},
        {"durationMs", snapshot.duration_ms ? json(*snapshot.duration_ms) : json(nullptr)},
        {"volume", snapshot.volume},
        {"backend", "foobar2000"},
    };
}

json envelope_json(const std::string_view kind,
                   const std::optional<std::string_view> correlation_id,
                   const std::optional<std::uint64_t> sequence,
                   json payload) {
    return {
        {"protocolVersion", protocol_version},
        {"messageId", new_uuid()},
        {"correlationId", correlation_id ? json(*correlation_id) : json(nullptr)},
        {"sentAtUnixMs", unix_time_ms()},
        {"kind", kind},
        {"sequence", sequence ? json(*sequence) : json(nullptr)},
        {"payload", std::move(payload)},
    };
}

std::string validated_wire_message(json document) {
    return encode_json(decode_json(document.dump()));
}

}  // namespace

component_service::component_service(std::shared_ptr<playback_facade> facade,
                                     std::string plugin_version,
                                     const std::uint32_t process_id,
                                     std::string session_id)
    : facade_(std::move(facade)),
      plugin_version_(std::move(plugin_version)),
      process_id_(process_id),
      session_id_(std::move(session_id)) {
    if (!facade_ || plugin_version_.empty() || plugin_version_.size() > 128U ||
        process_id_ == 0U) {
        throw std::invalid_argument("component service configuration is invalid");
    }
    const json session(session_id_);
    const auto probe = envelope_json(
        "state.snapshot", std::nullopt, 0U,
        {{"sessionId", session}, {"revision", 0U}, {"status", "stopped"},
         {"trackId", nullptr}, {"positionMs", 0U}, {"durationMs", nullptr},
         {"volume", 1.0}, {"backend", "foobar2000"}});
    static_cast<void>(decode_json(probe.dump()));
}

std::optional<std::string> component_service::handle(const envelope& message) {
    if (message.kind == "handshake.request") {
        return handshake_response(message);
    }
    if (message.kind == "command.request") {
        return command_response(message);
    }
    if (message.kind == "ping") {
        const auto& request_id = message.document.at("messageId").get_ref<const std::string&>();
        return validated_wire_message(envelope_json(
            "pong", request_id, std::nullopt,
            {{"nonce", message.document.at("payload").at("nonce")}}));
    }
    return error_response(message, "protocol_invalid", "Unexpected client message.", false);
}

std::string component_service::state_snapshot_message(
    const playback_snapshot& snapshot) const {
    return validated_wire_message(envelope_json("state.snapshot", std::nullopt,
                                                snapshot.revision, snapshot_json(snapshot)));
}

std::string component_service::playback_event_message(
    std::string event,
    const playback_snapshot& snapshot,
    std::optional<std::string> error_code) const {
    return validated_wire_message(envelope_json(
        "playback.event", std::nullopt, snapshot.revision,
        {{"event", std::move(event)},
         {"snapshot", snapshot_json(snapshot)},
         {"errorCode", error_code ? json(std::move(*error_code)) : json(nullptr)}}));
}

void component_service::begin_shutdown() {
    facade_->begin_shutdown();
}

std::string component_service::handshake_response(const envelope& request) const {
    const auto& request_id = request.document.at("messageId").get_ref<const std::string&>();
    const auto& nonce = request.document.at("payload").at("nonce");
    return validated_wire_message(envelope_json(
        "handshake.response", request_id, std::nullopt,
        {{"pluginVersion", plugin_version_},
         {"selectedProtocolVersion", protocol_version},
         {"processId", process_id_},
         {"nonce", nonce},
         {"sessionId", session_id_},
         {"capabilities", json::array({"play", "pause", "resume", "stop", "seek",
                                        "skip_next", "set_volume", "get_state"})}}));
}

std::string component_service::command_response(const envelope& request) {
    const auto& payload = request.document.at("payload");
    const auto operation_id = payload.at("operationId").get<std::string>();
    const auto fingerprint = payload.at("command").dump();
    const auto claim = operations_.claim(operation_id, fingerprint);
    if (claim.kind == operation_claim_kind::replay) {
        return claim.result;
    }
    if (claim.kind == operation_claim_kind::conflict) {
        return error_response(request, "operation_conflict",
                              "The operation identifier was already used for another command.",
                              false);
    }
    if (claim.kind == operation_claim_kind::in_flight) {
        return error_response(request, "operation_conflict",
                              "The operation is still in progress.", true);
    }

    std::string response;
    try {
        const auto snapshot = facade_->execute(request);
        const auto& request_id = request.document.at("messageId").get_ref<const std::string&>();
        response = validated_wire_message(envelope_json(
            "command.ack", request_id, std::nullopt,
            {{"operationId", operation_id},
             {"accepted", true},
             {"snapshot", snapshot_json(snapshot)}}));
    } catch (const std::exception& error) {
        const std::string_view detail(error.what());
        const auto unavailable = detail.starts_with("backend_unavailable:");
        response = error_response(request, unavailable ? "backend_unavailable" : "playback_failed",
                                  unavailable ? "The playback backend is unavailable."
                                              : "The playback command failed.",
                                  unavailable);
    }
    operations_.complete(operation_id, fingerprint, response);
    return response;
}

std::string component_service::error_response(const envelope& request,
                                              std::string code,
                                              std::string message,
                                              const bool retryable) const {
    const auto& request_id = request.document.at("messageId").get_ref<const std::string&>();
    return validated_wire_message(envelope_json(
        "error.response", request_id, std::nullopt,
        {{"code", std::move(code)}, {"message", std::move(message)}, {"retryable", retryable}}));
}

}  // namespace fishmuse::foobar
