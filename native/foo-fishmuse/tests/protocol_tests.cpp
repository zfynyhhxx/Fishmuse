#include "framing.hpp"
#include "pipe_probe.hpp"
#include "protocol.hpp"
#include "test_support.hpp"

#include <array>
#include <chrono>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <iterator>
#include <span>
#include <string>
#include <vector>

#include <nlohmann/json.hpp>

void run_pipe_security_tests();
void run_idempotency_tests();
void run_playback_facade_tests();
void run_sdk_adapter_support_tests();
void run_component_service_tests();

namespace {

using fishmuse::foobar::frame_decoder;
using fishmuse::foobar::decode_json;
using fishmuse::foobar::encode_frame;
using fishmuse::foobar::encode_json;
using fishmuse::foobar::handshake_gate;
using fishmuse::foobar::max_frame_size;
using fishmuse::foobar::sequence_tracker;
using fishmuse::test::require;
using fishmuse::test::require_protocol_error;

std::string read_vector(std::string_view name) {
    const auto path = std::filesystem::path(FISHMUSE_PROTOCOL_DIR) / "vectors" / name;
    std::ifstream input(path, std::ios::binary);
    require(input.good(), "golden vector must be readable");
    return {std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>()};
}

void golden_vectors_round_trip_semantically() {
    constexpr std::string_view names[] = {
        "handshake.request.json", "handshake.response.json", "play.request.json",
        "play.ack.json", "state.snapshot.json", "error.response.json",
    };
    for (const auto name : names) {
        const auto source = read_vector(name);
        const auto envelope = decode_json(source);
        require(nlohmann::json::parse(encode_json(envelope)) == nlohmann::json::parse(source),
                "golden vector semantic round trip changed");
    }
}

void active_foobar_track_can_be_unknown_to_fishmuse() {
    auto snapshot = nlohmann::json::parse(read_vector("state.snapshot.json"));
    snapshot["payload"]["trackId"] = nullptr;
    const auto decoded = decode_json(snapshot.dump());
    require(decoded.document.at("payload").at("trackId").is_null(),
            "active foobar playback may not have a FishMuse TrackId");
}

void malformed_and_unknown_json_is_rejected() {
    auto unknown = nlohmann::json::parse(read_vector("handshake.request.json"));
    unknown["kind"] = "administrator.execute";
    require_protocol_error([&] { static_cast<void>(decode_json(unknown.dump())); },
                           "protocol_invalid");

    auto permission = nlohmann::json::parse(read_vector("play.request.json"));
    permission["payload"]["command"]["permission"] = "arbitrary_local_path";
    require_protocol_error([&] { static_cast<void>(decode_json(permission.dump())); },
                           "protocol_invalid");

    auto unsupported = nlohmann::json::parse(read_vector("handshake.request.json"));
    unsupported["protocolVersion"] = 2;
    require_protocol_error([&] { static_cast<void>(decode_json(unsupported.dump())); },
                           "protocol_unsupported");

    const auto duplicate = R"json({
        "protocolVersion":1,
        "messageId":"0199a1b2-c3d4-7001-8000-000000000003",
        "correlationId":null,
        "sentAtUnixMs":0,
        "kind":"command.request",
        "sequence":null,
        "payload":{
            "operationId":"0199a1b2-c3d4-7003-8000-000000000001",
            "operationId":"0199a1b2-c3d4-7003-8000-000000000002",
            "command":{
                "name":"pause"
            }
        }
    })json";
    require_protocol_error([&] { static_cast<void>(decode_json(duplicate)); },
                           "protocol_invalid");

    auto forged_handshake = nlohmann::json::parse(read_vector("handshake.request.json"));
    forged_handshake["payload"]["nonce"] = "NOT-A-VALID-NONCE";
    require_protocol_error(
        [&] { static_cast<void>(decode_json(forged_handshake.dump())); },
        "protocol_invalid");
}

void handshake_and_sequence_are_gated() {
    using namespace std::chrono_literals;

    handshake_gate gate;
    const auto command = decode_json(read_vector("play.request.json"));
    require_protocol_error([&] { gate.accept(command); }, "handshake_required");

    const auto handshake = decode_json(read_vector("handshake.request.json"));
    gate.accept(handshake);
    require(gate.complete(), "valid handshake must complete the gate");
    gate.accept(command);

    sequence_tracker tracker;
    require(tracker.accept(42), "first sequence must be accepted");
    require(!tracker.accept(42), "duplicate sequence must be stale");
    require(!tracker.accept(41), "older sequence must be stale");
    require(tracker.accept(43), "newer sequence must be accepted");

    const auto started = handshake_gate::clock::now();
    handshake_gate expiring_gate(100ms, started);
    require(!expiring_gate.expired(started + 99ms),
            "handshake must remain open before its deadline");
    require(expiring_gate.expired(started + 100ms),
            "handshake must expire at its deadline");
    require_protocol_error(
        [&] { expiring_gate.accept(handshake, started + 100ms); },
        "handshake_required");
}

void frame_limits_and_stream_boundaries_are_enforced() {
    frame_decoder decoder;
    const std::array<std::byte, 4> oversized = {
        std::byte{0x01}, std::byte{0x00}, std::byte{0x10}, std::byte{0x00},
    };
    require_protocol_error([&] { static_cast<void>(decoder.push(oversized)); },
                           "frame_too_large");

    const std::array<std::byte, 4> empty = {};
    require_protocol_error([&] { static_cast<void>(decoder.push(empty)); },
                           "protocol_invalid");

    const auto first = encode_frame("first");
    const auto second = encode_frame("second");
    std::vector<std::byte> joined = first;
    joined.insert(joined.end(), second.begin(), second.end());
    frame_decoder joined_decoder;
    const auto frames = joined_decoder.push(joined);
    require(frames == std::vector<std::string>{"first", "second"},
            "coalesced frames must retain boundaries");
    joined_decoder.finish();

    frame_decoder truncated;
    static_cast<void>(truncated.push(std::span(first).first(2)));
    static_cast<void>(truncated.push(std::span(first).subspan(2, 4)));
    require_protocol_error([&] { truncated.finish(); }, "protocol_invalid");

    require(encode_frame(std::string(max_frame_size, 'x')).size() == max_frame_size + 4,
            "exact frame limit must be accepted");
    require_protocol_error(
        [&] { static_cast<void>(encode_frame(std::string(max_frame_size + 1, 'x'))); },
        "frame_too_large");
}

}  // namespace

int wmain(const int argc, wchar_t* argv[]) {
    try {
        if (argc == 4 && std::wstring_view(argv[1]) == L"--auth-probe-server") {
            return fishmuse::test::run_auth_probe_server(argv[2], argv[3]);
        }
        if (argc == 3 && std::wstring_view(argv[1]) == L"--auth-probe-client") {
            return fishmuse::test::run_auth_probe_client(argv[2]);
        }
        if (argc != 1) {
            std::cerr << "foo_fishmuse_tests: unknown arguments\n";
            return 64;
        }
        golden_vectors_round_trip_semantically();
        active_foobar_track_can_be_unknown_to_fishmuse();
        malformed_and_unknown_json_is_rejected();
        handshake_and_sequence_are_gated();
        frame_limits_and_stream_boundaries_are_enforced();
        run_pipe_security_tests();
        run_idempotency_tests();
        run_playback_facade_tests();
        run_sdk_adapter_support_tests();
        run_component_service_tests();
        std::cout << "foo_fishmuse_tests: PASS\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "foo_fishmuse_tests: FAIL: " << error.what() << '\n';
        return 1;
    }
}
