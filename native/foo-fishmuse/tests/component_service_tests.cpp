#include "component_service.hpp"
#include "playback_facade.hpp"
#include "protocol.hpp"
#include "test_support.hpp"

#include <filesystem>
#include <fstream>
#include <functional>
#include <iterator>
#include <memory>
#include <optional>
#include <string>

#include <nlohmann/json.hpp>

namespace {

using fishmuse::test::require;

std::string read_vector(const std::string_view name) {
    const auto path = std::filesystem::path(FISHMUSE_PROTOCOL_DIR) / "vectors" / name;
    std::ifstream input(path, std::ios::binary);
    require(input.good(), "golden vector must be readable");
    return {std::istreambuf_iterator<char>(input), std::istreambuf_iterator<char>()};
}

class immediate_dispatcher final : public fishmuse::foobar::main_thread_dispatcher {
public:
    void invoke(std::function<void()> action) override { action(); }
    void clear_pending() noexcept override { cleared = true; }
    bool cleared = false;
};

class counting_backend final : public fishmuse::foobar::playback_backend {
public:
    void play(const fishmuse::foobar::play_request& request) override {
        ++play_calls;
        snapshot_.status = fishmuse::foobar::playback_status::playing;
        snapshot_.track_id = request.track_id;
        ++snapshot_.revision;
    }
    void pause() override { ++pause_calls; }
    void resume() override {}
    void stop() override {}
    void seek(std::uint64_t) override {}
    void skip_next() override {}
    void set_volume(double) override {}
    fishmuse::foobar::playback_snapshot snapshot() override { return snapshot_; }

    std::size_t play_calls = 0;
    std::size_t pause_calls = 0;

private:
    fishmuse::foobar::playback_snapshot snapshot_{
        "0199a1b2-c3d4-7002-8000-000000000001",
        41U,
        fishmuse::foobar::playback_status::stopped,
        std::nullopt,
        0U,
        411000U,
        0.8,
    };
};

void handshake_and_idempotent_commands_form_valid_protocol_messages() {
    auto dispatcher = std::make_shared<immediate_dispatcher>();
    auto backend = std::make_shared<counting_backend>();
    auto facade = std::make_shared<fishmuse::foobar::playback_facade>(backend, dispatcher);
    fishmuse::foobar::component_service service(
        facade, "0.1.0", 4343U, "0199a1b2-c3d4-7002-8000-000000000001");

    const auto handshake = fishmuse::foobar::decode_json(read_vector("handshake.request.json"));
    const auto handshake_response = service.handle(handshake);
    require(handshake_response.has_value(), "handshake must return a response");
    const auto decoded_handshake = fishmuse::foobar::decode_json(*handshake_response);
    require(decoded_handshake.kind == "handshake.response",
            "component must produce a valid handshake response");
    require(decoded_handshake.document["payload"]["nonce"] ==
                handshake.document["payload"]["nonce"],
            "handshake response must echo client nonce");
    require(decoded_handshake.document["payload"]["capabilities"].size() == 8U,
            "handshake must advertise the closed v1 capability set");

    const auto play = fishmuse::foobar::decode_json(read_vector("play.request.json"));
    const auto first_ack = service.handle(play);
    const auto replayed_ack = service.handle(play);
    require(first_ack.has_value() && replayed_ack.has_value(),
            "command and ACK-loss retry must both return responses");
    require(*first_ack == *replayed_ack, "ACK-loss retry must replay byte-identical final result");
    const auto decoded_ack = fishmuse::foobar::decode_json(*first_ack);
    require(decoded_ack.kind == "command.ack", "successful command must return command.ack");
    require(decoded_ack.document["payload"]["snapshot"]["revision"] == 42U,
            "ACK must contain post-command authoritative snapshot");
    require(backend->play_calls == 1U, "ACK-loss retry must not repeat playback side effect");

    auto conflict_document = play.document;
    conflict_document["payload"]["command"] = {{"name", "pause"}};
    const auto conflict = service.handle(fishmuse::foobar::decode_json(conflict_document.dump()));
    require(conflict.has_value(), "operation conflict must return a response");
    const auto decoded_conflict = fishmuse::foobar::decode_json(*conflict);
    require(decoded_conflict.kind == "error.response",
            "operation conflict must return error.response");
    require(decoded_conflict.document["payload"]["code"] == "operation_conflict",
            "operation conflict must use the stable error code");
    require(backend->pause_calls == 0U, "conflicting retry must not execute another side effect");

    const fishmuse::foobar::playback_snapshot event_snapshot{
        "0199a1b2-c3d4-7002-8000-000000000001",
        43U,
        fishmuse::foobar::playback_status::playing,
        std::nullopt,
        1250U,
        411000U,
        0.8,
    };
    const auto event = fishmuse::foobar::decode_json(
        service.playback_event_message("state_changed", event_snapshot, std::nullopt));
    require(event.kind == "playback.event", "SDK callback must produce playback.event");
    require(event.document["sequence"] == 43U,
            "playback event sequence must match snapshot revision");
    require(event.document["payload"]["snapshot"]["trackId"].is_null(),
            "event may describe active playback unknown to FishMuse");
    require(event.document["payload"]["errorCode"].is_null(),
            "ordinary playback event must not invent an error code");

    service.begin_shutdown();
    require(dispatcher->cleared, "component shutdown must stop facade work");
}

}  // namespace

void run_component_service_tests() {
    handshake_and_idempotent_commands_form_valid_protocol_messages();
}
