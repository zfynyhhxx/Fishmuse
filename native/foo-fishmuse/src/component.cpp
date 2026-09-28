#include <foobar2000/SDK/foobar2000.h>

#include "component_service.hpp"
#include "pipe_security.hpp"
#include "pipe_server.hpp"
#include "playback_facade.hpp"
#include "sdk_adapter_support.hpp"

#define WIN32_LEAN_AND_MEAN
#define NOMINMAX
#include <windows.h>
#include <objbase.h>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <limits>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>

DECLARE_COMPONENT_VERSION(
    "FishMuse Playback Bridge",
    "0.1.0",
    "Same-user named-pipe playback bridge for FishMuse.\n"
    "Does not read or modify the foobar2000 media database.");
VALIDATE_COMPONENT_FILENAME("foo_fishmuse.dll");
FOOBAR2000_IMPLEMENT_CFG_VAR_DOWNGRADE;

namespace fishmuse::foobar {
namespace {

constexpr std::string_view component_version = "0.1.0";
constexpr char bridge_playlist_name[] = "FishMuse Playback Bridge";

[[noreturn]] void fail(const std::string_view detail) {
    throw std::runtime_error("playback_failed: " + std::string(detail));
}

std::string new_session_id() {
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
        const auto character = static_cast<char>(text[index]);
        result.push_back(character >= 'A' && character <= 'F'
                             ? static_cast<char>(character - 'A' + 'a')
                             : character);
    }
    return result;
}

std::uint64_t milliseconds_from_seconds(const double seconds) noexcept {
    if (!std::isfinite(seconds) || seconds <= 0.0) {
        return 0U;
    }
    constexpr auto maximum =
        static_cast<double>((std::numeric_limits<std::uint64_t>::max)());
    return seconds >= maximum / 1000.0
               ? (std::numeric_limits<std::uint64_t>::max)()
               : static_cast<std::uint64_t>(std::llround(seconds * 1000.0));
}

class foobar_playback_backend final : public playback_backend,
                                      public std::enable_shared_from_this<foobar_playback_backend> {
public:
    explicit foobar_playback_backend(std::string session_id)
        : session_id_(std::move(session_id)) {}

    void play(const play_request& request) override {
        pfc::string8 canonical_path;
        filesystem::g_get_canonical_path(request.path.c_str(), canonical_path);
        if (canonical_path.is_empty()) {
            fail("foobar2000 rejected the local media path");
        }

        const auto subsong = request.subsong_index.value_or(0U);
        const auto handle = metadb::get()->handle_create(canonical_path.c_str(), subsong);
        auto playlists = playlist_manager::get();
        const auto playlist = playlists->find_or_create_playlist_unlocked(bridge_playlist_name);
        if (playlist == SIZE_MAX) {
            fail("could not create the FishMuse playback playlist");
        }
        playlists->playlist_clear(playlist);
        const pfc::list_single_ref_t<metadb_handle_ptr> items(handle);
        const auto inserted = playlists->playlist_insert_items(
            playlist, 0U, items, pfc::bit_array_val(false));
        if (inserted == SIZE_MAX) {
            fail("foobar2000 refused to insert the local media item");
        }
        if (!playlists->playlist_execute_default_action(playlist, inserted)) {
            fail("foobar2000 refused to start the local media item");
        }

        commanded_handle_ = handle;
        commanded_track_id_ = request.track_id;
        pending_start_ms_ = request.start_ms;
        end_ms_ = request.end_ms;
        end_stop_queued_ = false;
        ++revision_;
    }

    void pause() override {
        playback_control::get()->pause(true);
        ++revision_;
    }

    void resume() override {
        playback_control::get()->pause(false);
        ++revision_;
    }

    void stop() override {
        playback_control::get()->stop();
        ++revision_;
    }

    void seek(const std::uint64_t position_ms) override {
        const auto control = playback_control::get();
        if (!control->playback_can_seek()) {
            fail("the current media item is not seekable");
        }
        control->playback_seek(static_cast<double>(position_ms) / 1000.0);
        ++revision_;
    }

    void skip_next() override {
        playback_control::get()->next();
        ++revision_;
    }

    void set_volume(const double volume) override {
        playback_control::get()->set_volume(foobar_db_from_normalized(volume));
        ++revision_;
    }

    [[nodiscard]] playback_snapshot snapshot() override {
        const auto control = playback_control::get();
        if (!control->is_playing()) {
            return {session_id_, revision_, playback_status::stopped, std::nullopt,
                    0U, std::nullopt,
                    normalized_from_foobar_db(control->get_volume())};
        }

        metadb_handle_ptr now_playing;
        std::optional<std::string> track_id;
        if (control->get_now_playing(now_playing) && !commanded_handle_.is_empty() &&
            now_playing == commanded_handle_) {
            track_id = commanded_track_id_;
        }

        auto position_ms = milliseconds_from_seconds(control->playback_get_position());
        const auto length_ms = milliseconds_from_seconds(control->playback_get_length_ex());
        std::optional<std::uint64_t> duration_ms;
        if (length_ms > 0U) {
            duration_ms = length_ms;
            position_ms = (std::min)(position_ms, length_ms);
        }
        return {
            session_id_,
            revision_,
            control->is_paused() ? playback_status::paused : playback_status::playing,
            std::move(track_id),
            position_ms,
            duration_ms,
            normalized_from_foobar_db(control->get_volume()),
        };
    }

    [[nodiscard]] playback_snapshot event_snapshot() {
        ++revision_;
        return snapshot();
    }

    [[nodiscard]] std::optional<std::uint64_t> observe_new_track(
        const metadb_handle_ptr& track) {
        end_stop_queued_ = false;
        if (commanded_handle_.is_empty() || track != commanded_handle_) {
            commanded_track_id_.reset();
            pending_start_ms_.reset();
            end_ms_.reset();
            return std::nullopt;
        }
        auto start = pending_start_ms_;
        pending_start_ms_.reset();
        return start;
    }

    void apply_segment_start(const metadb_handle_ptr& expected,
                             const std::uint64_t position_ms) {
        const auto control = playback_control::get();
        metadb_handle_ptr now_playing;
        if (control->get_now_playing(now_playing) && now_playing == expected &&
            control->playback_can_seek()) {
            control->playback_seek(static_cast<double>(position_ms) / 1000.0);
        }
    }

    [[nodiscard]] bool queue_segment_end_if_reached(const double position_seconds) {
        if (!end_ms_ || end_stop_queued_ || !std::isfinite(position_seconds) ||
            position_seconds * 1000.0 < static_cast<double>(*end_ms_)) {
            return false;
        }
        end_stop_queued_ = true;
        return true;
    }

    void stop_at_segment_end() {
        playback_control::get()->stop();
    }

private:
    std::string session_id_;
    std::uint64_t revision_ = 0U;
    metadb_handle_ptr commanded_handle_;
    std::optional<std::string> commanded_track_id_;
    std::optional<std::uint64_t> pending_start_ms_;
    std::optional<std::uint64_t> end_ms_;
    bool end_stop_queued_ = false;
};

class playback_listener final : public play_callback_impl_base {
public:
    playback_listener(std::shared_ptr<foobar_playback_backend> backend,
                      deferred_event_dispatcher& events)
        : play_callback_impl_base(play_callback::flag_on_playback_starting |
                                  play_callback::flag_on_playback_new_track |
                                  play_callback::flag_on_playback_stop |
                                  play_callback::flag_on_playback_seek |
                                  play_callback::flag_on_playback_pause |
                                  play_callback::flag_on_playback_time |
                                  play_callback::flag_on_volume_change),
          backend_(std::move(backend)), events_(events) {}

    void on_playback_starting(play_control::t_track_command, bool) override {
        publish("state_changed");
    }

    void on_playback_new_track(metadb_handle_ptr track) override {
        const auto start_ms = backend_->observe_new_track(track);
        publish("track_changed");
        if (start_ms && *start_ms > 0U) {
            const auto weak_backend = std::weak_ptr<foobar_playback_backend>(backend_);
            fb2k::inMainThread([weak_backend, track = std::move(track), start = *start_ms] {
                if (const auto backend = weak_backend.lock()) {
                    backend->apply_segment_start(track, start);
                }
            });
        }
    }

    void on_playback_stop(play_control::t_stop_reason) override {
        publish("state_changed");
    }

    void on_playback_seek(double) override {
        publish("position");
    }

    void on_playback_pause(bool) override {
        publish("state_changed");
    }

    void on_playback_time(const double position) override {
        publish("position");
        if (backend_->queue_segment_end_if_reached(position)) {
            const auto weak_backend = std::weak_ptr<foobar_playback_backend>(backend_);
            fb2k::inMainThread([weak_backend] {
                if (const auto backend = weak_backend.lock()) {
                    backend->stop_at_segment_end();
                }
            });
        }
    }

    void on_volume_change(float) override {
        publish("volume_changed");
    }

private:
    void publish(std::string event) noexcept {
        try {
            events_.enqueue(std::move(event));
        } catch (const std::exception&) {
        }
    }

    std::shared_ptr<foobar_playback_backend> backend_;
    deferred_event_dispatcher& events_;
};

class component_runtime final {
public:
    component_runtime() {
        const auto session_id = new_session_id();
        dispatcher_ = std::make_shared<queued_main_thread_dispatcher>(
            [](queued_main_thread_dispatcher::callback action) {
                fb2k::inMainThread(std::move(action));
            },
            [] { return core_api::is_main_thread(); });
        backend_ = std::make_shared<foobar_playback_backend>(session_id);
        facade_ = std::make_shared<playback_facade>(backend_, dispatcher_);
        service_ = std::make_shared<component_service>(
            facade_, std::string(component_version), GetCurrentProcessId(), session_id);
        const auto weak_service = std::weak_ptr<component_service>(service_);
        server_ = std::make_unique<named_pipe_server>(
            pipe_security::for_current_user(),
            [weak_service](const envelope& message) -> std::optional<std::string> {
                if (const auto service = weak_service.lock()) {
                    return service->handle(message);
                }
                return std::nullopt;
            });
        events_ = std::make_unique<deferred_event_dispatcher>(
            [](deferred_event_dispatcher::callback action) {
                fb2k::inMainThread(std::move(action));
            },
            [backend = backend_, service = service_, server = server_.get()](std::string event) {
                const auto snapshot = backend->event_snapshot();
                if (!server->publish_incremental(service->playback_event_message(
                        std::move(event), snapshot, std::nullopt))) {
                    server->publish_snapshot(service->state_snapshot_message(snapshot));
                }
            });
        listener_ = std::make_unique<playback_listener>(backend_, *events_);
        server_->start();
    }

    component_runtime(const component_runtime&) = delete;
    component_runtime& operator=(const component_runtime&) = delete;

    ~component_runtime() {
        shutdown();
    }

    void shutdown() noexcept {
        if (service_) {
            service_->begin_shutdown();
        }
        listener_.reset();
        if (events_) {
            events_->clear_pending();
        }
        if (server_) {
            server_->stop();
        }
        events_.reset();
        server_.reset();
        service_.reset();
        facade_.reset();
        backend_.reset();
        dispatcher_.reset();
    }

private:
    std::shared_ptr<queued_main_thread_dispatcher> dispatcher_;
    std::shared_ptr<foobar_playback_backend> backend_;
    std::shared_ptr<playback_facade> facade_;
    std::shared_ptr<component_service> service_;
    std::unique_ptr<named_pipe_server> server_;
    std::unique_ptr<deferred_event_dispatcher> events_;
    std::unique_ptr<playback_listener> listener_;
};

class component_lifecycle : public initquit {
public:
    void on_init() override {
        try {
            runtime_ = std::make_unique<component_runtime>();
        } catch (const std::exception& error) {
            FB2K_console_formatter() << "[foo_fishmuse] Initialization failed: " << error.what();
        }
    }

    void on_quit() override {
        runtime_.reset();
    }

private:
    std::unique_ptr<component_runtime> runtime_;
};

initquit_factory_t<component_lifecycle> component_lifecycle_factory;

}  // namespace
}  // namespace fishmuse::foobar
