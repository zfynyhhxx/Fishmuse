#include "playback_facade.hpp"

#include <stdexcept>
#include <string_view>
#include <utility>

namespace fishmuse::foobar {
namespace {

[[noreturn]] void fail(const std::string_view code, const std::string_view detail) {
    throw std::runtime_error(std::string(code) + ": " + std::string(detail));
}

std::optional<std::uint64_t> optional_u64(const nlohmann::json& value) {
    if (value.is_null()) {
        return std::nullopt;
    }
    return value.get<std::uint64_t>();
}

}  // namespace

playback_facade::playback_facade(std::shared_ptr<playback_backend> backend,
                                 std::shared_ptr<main_thread_dispatcher> dispatcher)
    : backend_(std::move(backend)), dispatcher_(std::move(dispatcher)) {
    if (!backend_ || !dispatcher_) {
        throw std::invalid_argument("playback facade requires backend and dispatcher");
    }
}

playback_snapshot playback_facade::execute(const envelope& request) {
    {
        std::scoped_lock lock(lifecycle_mutex_);
        if (!accepting_) {
            fail("backend_unavailable", "playback component is shutting down");
        }
    }
    if (request.kind != "command.request") {
        fail("protocol_invalid", "playback facade accepts command.request only");
    }

    const auto& command = request.document.at("payload").at("command");
    const auto name = command.at("name").get<std::string>();
    std::optional<playback_snapshot> result;

    dispatcher_->invoke([this, &command, &name, &result] {
        if (name == "play") {
            const auto subsong = optional_u64(command.at("subsongIndex"));
            backend_->play(play_request{
                command.at("trackId").get<std::string>(),
                command.at("path").get<std::string>(),
                subsong ? std::optional<std::uint32_t>(static_cast<std::uint32_t>(*subsong))
                        : std::nullopt,
                optional_u64(command.at("startMs")),
                optional_u64(command.at("endMs")),
            });
        } else if (name == "pause") {
            backend_->pause();
        } else if (name == "resume") {
            backend_->resume();
        } else if (name == "stop") {
            backend_->stop();
        } else if (name == "seek") {
            backend_->seek(command.at("positionMs").get<std::uint64_t>());
        } else if (name == "skip_next") {
            backend_->skip_next();
        } else if (name == "set_volume") {
            backend_->set_volume(command.at("volume").get<double>());
        } else if (name != "get_state") {
            fail("protocol_invalid", "unknown playback command");
        }
        result = backend_->snapshot();
    });

    if (!result) {
        fail("backend_unavailable", "main-thread dispatcher did not execute command");
    }
    return std::move(*result);
}

void playback_facade::begin_shutdown() {
    std::scoped_lock lock(lifecycle_mutex_);
    if (!accepting_) {
        return;
    }
    accepting_ = false;
    dispatcher_->clear_pending();
}

}  // namespace fishmuse::foobar
