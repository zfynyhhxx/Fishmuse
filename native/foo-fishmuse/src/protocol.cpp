#include "protocol.hpp"

#include <algorithm>
#include <cmath>
#include <initializer_list>
#include <limits>
#include <stdexcept>
#include <string>
#include <unordered_set>
#include <vector>

namespace fishmuse::foobar {
namespace {

using json = nlohmann::json;

[[noreturn]] void fail(std::string_view code, std::string_view detail) {
    throw std::runtime_error(std::string(code) + ": " + std::string(detail));
}

void require_exact_keys(const json& value,
                        const std::initializer_list<std::string_view> keys,
                        const std::string_view context) {
    if (!value.is_object() || value.size() != keys.size()) {
        fail("protocol_invalid", std::string(context) + " fields are invalid");
    }
    for (const auto key : keys) {
        if (!value.contains(std::string(key))) {
            fail("protocol_invalid", std::string(context) + " fields are invalid");
        }
    }
}

std::uint64_t require_u64(const json& value, const std::string_view field) {
    if (value.is_number_unsigned()) {
        return value.get<std::uint64_t>();
    }
    if (value.is_number_integer()) {
        const auto signed_value = value.get<std::int64_t>();
        if (signed_value >= 0) {
            return static_cast<std::uint64_t>(signed_value);
        }
    }
    fail("protocol_invalid", std::string("invalid ") + std::string(field));
}

std::uint32_t require_u32(const json& value, const std::string_view field) {
    const auto number = require_u64(value, field);
    if (number == 0U || number > std::numeric_limits<std::uint32_t>::max()) {
        fail("protocol_invalid", std::string("invalid ") + std::string(field));
    }
    return static_cast<std::uint32_t>(number);
}

const std::string& require_text(const json& value,
                                const std::size_t maximum,
                                const std::string_view field) {
    if (!value.is_string()) {
        fail("protocol_invalid", std::string("invalid ") + std::string(field));
    }
    const auto& text = value.get_ref<const std::string&>();
    if (text.empty() || text.size() > maximum) {
        fail("protocol_invalid", std::string("invalid ") + std::string(field));
    }
    return text;
}

bool is_hex(const char value) {
    return (value >= '0' && value <= '9') || (value >= 'a' && value <= 'f') ||
           (value >= 'A' && value <= 'F');
}

void require_uuid(const json& value, const std::string_view field) {
    const auto& text = require_text(value, 36U, field);
    if (text.size() != 36U) {
        fail("protocol_invalid", std::string("invalid ") + std::string(field));
    }
    for (std::size_t index = 0; index < text.size(); ++index) {
        const auto hyphen = index == 8U || index == 13U || index == 18U || index == 23U;
        if ((hyphen && text[index] != '-') || (!hyphen && !is_hex(text[index]))) {
            fail("protocol_invalid", std::string("invalid ") + std::string(field));
        }
    }
}

void require_nullable_uuid(const json& value, const std::string_view field) {
    if (!value.is_null()) {
        require_uuid(value, field);
    }
}

void require_nonce(const json& value) {
    const auto& nonce = require_text(value, 32U, "nonce");
    if (nonce.size() != 32U ||
        !std::ranges::all_of(nonce, [](const char value) {
            return (value >= '0' && value <= '9') || (value >= 'a' && value <= 'f');
        })) {
        fail("protocol_invalid", "nonce must be 32 lowercase hex digits");
    }
}

double require_unit_interval(const json& value, const std::string_view field) {
    if (!value.is_number()) {
        fail("protocol_invalid", std::string("invalid ") + std::string(field));
    }
    const auto number = value.get<double>();
    if (!std::isfinite(number) || number < 0.0 || number > 1.0) {
        fail("protocol_invalid", std::string("invalid ") + std::string(field));
    }
    return number;
}

void validate_snapshot(const json& snapshot) {
    require_exact_keys(snapshot,
                       {"sessionId", "revision", "status", "trackId", "positionMs",
                        "durationMs", "volume", "backend"},
                       "state snapshot");
    require_uuid(snapshot.at("sessionId"), "sessionId");
    static_cast<void>(require_u64(snapshot.at("revision"), "revision"));
    const auto& status = require_text(snapshot.at("status"), 32U, "status");
    static const std::unordered_set<std::string> statuses = {
        "stopped", "loading", "playing", "paused", "unavailable",
    };
    if (!statuses.contains(status)) {
        fail("protocol_invalid", "invalid playback status");
    }
    require_nullable_uuid(snapshot.at("trackId"), "trackId");
    const auto position = require_u64(snapshot.at("positionMs"), "positionMs");
    if (!snapshot.at("durationMs").is_null() &&
        position > require_u64(snapshot.at("durationMs"), "durationMs")) {
        fail("protocol_invalid", "position exceeds duration");
    }
    static_cast<void>(require_unit_interval(snapshot.at("volume"), "volume"));
    if (!snapshot.at("backend").is_string() ||
        snapshot.at("backend").get_ref<const std::string&>() != "foobar2000") {
        fail("protocol_invalid", "invalid playback backend");
    }
}

void validate_command(const json& command) {
    if (!command.is_object() || !command.contains("name") ||
        !command.at("name").is_string()) {
        fail("protocol_invalid", "invalid command");
    }
    const auto& name = command.at("name").get_ref<const std::string&>();
    if (name == "play") {
        require_exact_keys(command,
                           {"name", "trackId", "path", "subsongIndex", "startMs", "endMs"},
                           "play command");
        require_uuid(command.at("trackId"), "trackId");
        static_cast<void>(require_text(command.at("path"), 32767U, "path"));
        if (!command.at("subsongIndex").is_null()) {
            const auto subsong = require_u64(command.at("subsongIndex"), "subsongIndex");
            if (subsong > std::numeric_limits<std::uint32_t>::max()) {
                fail("protocol_invalid", "invalid subsongIndex");
            }
        }
        std::optional<std::uint64_t> start;
        std::optional<std::uint64_t> end;
        if (!command.at("startMs").is_null()) {
            start = require_u64(command.at("startMs"), "startMs");
        }
        if (!command.at("endMs").is_null()) {
            end = require_u64(command.at("endMs"), "endMs");
        }
        if (start && end && *start >= *end) {
            fail("protocol_invalid", "invalid play range");
        }
        return;
    }
    if (name == "seek") {
        require_exact_keys(command, {"name", "positionMs"}, "seek command");
        static_cast<void>(require_u64(command.at("positionMs"), "positionMs"));
        return;
    }
    if (name == "set_volume") {
        require_exact_keys(command, {"name", "volume"}, "set volume command");
        static_cast<void>(require_unit_interval(command.at("volume"), "volume"));
        return;
    }
    if (name == "pause" || name == "resume" || name == "stop" ||
        name == "skip_next" || name == "get_state") {
        require_exact_keys(command, {"name"}, "simple command");
        return;
    }
    fail("protocol_invalid", "unknown command");
}

void validate_handshake_request(const json& payload) {
    require_exact_keys(payload,
                       {"appVersion", "supportedProtocolVersions", "processId", "nonce"},
                       "handshake request");
    static_cast<void>(require_text(payload.at("appVersion"), 128U, "appVersion"));
    static_cast<void>(require_u32(payload.at("processId"), "processId"));
    require_nonce(payload.at("nonce"));
    const auto& versions = payload.at("supportedProtocolVersions");
    if (!versions.is_array() || versions.empty()) {
        fail("protocol_invalid", "invalid supportedProtocolVersions");
    }
    std::unordered_set<std::uint64_t> unique;
    bool supports_v1 = false;
    for (const auto& version : versions) {
        const auto number = require_u64(version, "supportedProtocolVersions");
        if (number == 0U || number > std::numeric_limits<std::uint16_t>::max() ||
            !unique.insert(number).second) {
            fail("protocol_invalid", "invalid supportedProtocolVersions");
        }
        supports_v1 = supports_v1 || number == protocol_version;
    }
    if (!supports_v1) {
        fail("protocol_invalid", "protocol v1 is not supported by client");
    }
}

void validate_handshake_response(const json& payload) {
    require_exact_keys(payload,
                       {"pluginVersion", "selectedProtocolVersion", "processId", "nonce",
                        "sessionId", "capabilities"},
                       "handshake response");
    static_cast<void>(require_text(payload.at("pluginVersion"), 128U, "pluginVersion"));
    if (require_u64(payload.at("selectedProtocolVersion"), "selectedProtocolVersion") !=
        protocol_version) {
        fail("protocol_invalid", "invalid selectedProtocolVersion");
    }
    static_cast<void>(require_u32(payload.at("processId"), "processId"));
    require_nonce(payload.at("nonce"));
    require_uuid(payload.at("sessionId"), "sessionId");
    const auto& capabilities = payload.at("capabilities");
    if (!capabilities.is_array() || capabilities.empty()) {
        fail("protocol_invalid", "invalid capabilities");
    }
    static const std::unordered_set<std::string> allowed = {
        "play", "pause", "resume", "stop", "seek", "skip_next", "set_volume",
        "get_state",
    };
    std::unordered_set<std::string> unique;
    for (const auto& capability : capabilities) {
        const auto& name = require_text(capability, 32U, "capability");
        if (!allowed.contains(name) || !unique.insert(name).second) {
            fail("protocol_invalid", "invalid capabilities");
        }
    }
}

void validate_payload(const json& document, const std::string& kind) {
    const auto& payload = document.at("payload");
    const auto request_metadata = [&document] {
        if (!document.at("correlationId").is_null() || !document.at("sequence").is_null()) {
            fail("protocol_invalid", "request metadata is invalid");
        }
    };
    const auto response_metadata = [&document] {
        if (document.at("correlationId").is_null() || !document.at("sequence").is_null()) {
            fail("protocol_invalid", "response metadata is invalid");
        }
    };

    if (kind == "handshake.request") {
        request_metadata();
        validate_handshake_request(payload);
    } else if (kind == "handshake.response") {
        response_metadata();
        validate_handshake_response(payload);
    } else if (kind == "command.request") {
        request_metadata();
        require_exact_keys(payload, {"operationId", "command"}, "command request");
        require_uuid(payload.at("operationId"), "operationId");
        validate_command(payload.at("command"));
    } else if (kind == "command.ack") {
        response_metadata();
        require_exact_keys(payload, {"operationId", "accepted", "snapshot"}, "command ack");
        require_uuid(payload.at("operationId"), "operationId");
        if (!payload.at("accepted").is_boolean() || !payload.at("accepted").get<bool>()) {
            fail("protocol_invalid", "rejected command must use error.response");
        }
        validate_snapshot(payload.at("snapshot"));
    } else if (kind == "state.snapshot") {
        if (!document.at("correlationId").is_null()) {
            fail("protocol_invalid", "state snapshot correlationId is invalid");
        }
        validate_snapshot(payload);
        if (document.at("sequence").is_null() ||
            require_u64(document.at("sequence"), "sequence") !=
                require_u64(payload.at("revision"), "revision")) {
            fail("protocol_invalid", "event sequence must match snapshot revision");
        }
    } else if (kind == "playback.event") {
        if (!document.at("correlationId").is_null()) {
            fail("protocol_invalid", "playback event correlationId is invalid");
        }
        require_exact_keys(payload, {"event", "snapshot", "errorCode"}, "playback event");
        static const std::unordered_set<std::string> events = {
            "state_changed", "track_changed", "position", "volume_changed", "playback_error",
        };
        const auto& event = require_text(payload.at("event"), 32U, "event");
        if (!events.contains(event)) {
            fail("protocol_invalid", "invalid playback event");
        }
        validate_snapshot(payload.at("snapshot"));
        const auto has_error = !payload.at("errorCode").is_null();
        if (has_error) {
            static_cast<void>(require_text(payload.at("errorCode"), 128U, "errorCode"));
        }
        if ((event == "playback_error") != has_error) {
            fail("protocol_invalid", "invalid playback event errorCode");
        }
        if (document.at("sequence").is_null() ||
            require_u64(document.at("sequence"), "sequence") !=
                require_u64(payload.at("snapshot").at("revision"), "revision")) {
            fail("protocol_invalid", "event sequence must match snapshot revision");
        }
    } else if (kind == "error.response") {
        response_metadata();
        require_exact_keys(payload, {"code", "message", "retryable"}, "error response");
        static const std::unordered_set<std::string> codes = {
            "protocol_invalid", "protocol_unsupported", "frame_too_large", "handshake_required",
            "unauthorized", "operation_conflict", "backend_unavailable", "playback_failed",
        };
        const auto& code = require_text(payload.at("code"), 64U, "code");
        if (!codes.contains(code) || !payload.at("retryable").is_boolean()) {
            fail("protocol_invalid", "invalid error response");
        }
        static_cast<void>(require_text(payload.at("message"), 512U, "message"));
    } else if (kind == "ping" || kind == "pong") {
        if (kind == "ping") {
            request_metadata();
        } else {
            response_metadata();
        }
        require_exact_keys(payload, {"nonce"}, "heartbeat");
        require_nonce(payload.at("nonce"));
    } else {
        fail("protocol_invalid", "unknown message kind");
    }
}

json parse_rejecting_duplicates(const std::string_view input) {
    bool duplicate = false;
    std::vector<std::unordered_set<std::string>> object_keys;
    const auto callback = [&duplicate, &object_keys](const int,
                                                     const json::parse_event_t event,
                                                     json& parsed) {
        if (event == json::parse_event_t::object_start) {
            object_keys.emplace_back();
        } else if (event == json::parse_event_t::key) {
            if (object_keys.empty() ||
                !object_keys.back().insert(parsed.get<std::string>()).second) {
                duplicate = true;
            }
        } else if (event == json::parse_event_t::object_end && !object_keys.empty()) {
            object_keys.pop_back();
        }
        return true;
    };

    auto document = json::parse(input.begin(), input.end(), callback, true, false);
    if (duplicate) {
        fail("protocol_invalid", "duplicate JSON field");
    }
    return document;
}

}  // namespace

envelope decode_json(const std::string_view input) {
    try {
        auto document = parse_rejecting_duplicates(input);
        require_exact_keys(document,
                           {"protocolVersion", "messageId", "correlationId", "sentAtUnixMs",
                            "kind", "sequence", "payload"},
                           "envelope");
        const auto version = require_u64(document.at("protocolVersion"), "protocolVersion");
        if (version != protocol_version) {
            fail("protocol_unsupported", "unsupported protocol version");
        }
        require_uuid(document.at("messageId"), "messageId");
        require_nullable_uuid(document.at("correlationId"), "correlationId");
        static_cast<void>(require_u64(document.at("sentAtUnixMs"), "sentAtUnixMs"));
        if (!document.at("sequence").is_null()) {
            static_cast<void>(require_u64(document.at("sequence"), "sequence"));
        }
        const auto& kind = require_text(document.at("kind"), 64U, "kind");
        if (!document.at("payload").is_object()) {
            fail("protocol_invalid", "payload must be an object");
        }
        validate_payload(document, kind);
        return envelope{std::move(document), kind};
    } catch (const std::runtime_error& error) {
        const std::string_view message(error.what());
        if (message.starts_with("protocol_invalid:") ||
            message.starts_with("protocol_unsupported:") ||
            message.starts_with("frame_too_large:") ||
            message.starts_with("handshake_required:")) {
            throw;
        }
        fail("protocol_invalid", error.what());
    } catch (const std::exception& error) {
        fail("protocol_invalid", error.what());
    }
}

std::string encode_json(const envelope& value) {
    return value.document.dump();
}

handshake_gate::handshake_gate(const std::chrono::milliseconds timeout,
                               const clock::time_point started) noexcept
    : timeout_(timeout), started_(started) {}

void handshake_gate::accept(const envelope& value, const clock::time_point now) {
    if (!complete_) {
        if (expired(now)) {
            fail("handshake_required", "handshake deadline expired");
        }
        if (value.kind != "handshake.request") {
            fail("handshake_required", "handshake must precede all other messages");
        }
        complete_ = true;
        return;
    }
    if (value.kind == "handshake.request" || value.kind == "handshake.response") {
        fail("protocol_invalid", "handshake is already complete");
    }
}

bool handshake_gate::complete() const noexcept {
    return complete_;
}

bool handshake_gate::expired(const clock::time_point now) const noexcept {
    return !complete_ && now - started_ >= timeout_;
}

bool sequence_tracker::accept(const std::uint64_t sequence) noexcept {
    if (last_applied_ && sequence <= *last_applied_) {
        return false;
    }
    last_applied_ = sequence;
    return true;
}

std::optional<std::uint64_t> sequence_tracker::last_applied() const noexcept {
    return last_applied_;
}

void sequence_tracker::reset() noexcept {
    last_applied_.reset();
}

}  // namespace fishmuse::foobar
