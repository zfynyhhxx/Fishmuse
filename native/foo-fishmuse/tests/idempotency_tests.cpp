#include "pipe_server.hpp"
#include "test_support.hpp"

#include <string>

void run_idempotency_tests() {
    using fishmuse::foobar::bounded_message_queue;
    using fishmuse::foobar::operation_cache;
    using fishmuse::foobar::operation_claim_kind;
    using fishmuse::test::require;

    operation_cache cache(1024);
    auto first = cache.claim("0199a1b2-c3d4-7003-8000-000000000001", "play:a");
    require(first.kind == operation_claim_kind::execute,
            "first operation claim must execute");
    const auto concurrent_retry =
        cache.claim("0199a1b2-c3d4-7003-8000-000000000001", "play:a");
    require(concurrent_retry.kind == operation_claim_kind::in_flight,
            "retry during execution must not repeat the side effect");
    cache.complete("0199a1b2-c3d4-7003-8000-000000000001", "play:a", "ack:playing");

    const auto lost_ack_retry =
        cache.claim("0199a1b2-c3d4-7003-8000-000000000001", "play:a");
    require(lost_ack_retry.kind == operation_claim_kind::replay,
            "ACK-loss retry must replay the recorded result");
    require(lost_ack_retry.result == "ack:playing", "replayed result must be stable");

    const auto conflict =
        cache.claim("0199a1b2-c3d4-7003-8000-000000000001", "seek:12");
    require(conflict.kind == operation_claim_kind::conflict,
            "same operation ID with another command must conflict");

    for (std::size_t index = 0; index < 1024; ++index) {
        const auto id = "operation-" + std::to_string(index);
        static_cast<void>(cache.claim(id, "pause"));
        cache.complete(id, "pause", "ack");
    }
    require(cache.size() == 1024, "operation LRU must retain at least 1024 final results");
    require(cache.claim("0199a1b2-c3d4-7003-8000-000000000001", "play:a").kind ==
                operation_claim_kind::execute,
            "oldest completed operation must be evicted after 1024 newer results");
    require(cache.claim("operation-1023", "pause").kind == operation_claim_kind::replay,
            "most recent completed operation must remain replayable");

    bounded_message_queue queue(2);
    require(queue.try_push_incremental("event-1"), "first event must enqueue");
    require(queue.try_push_incremental("event-2"), "second event must enqueue");
    require(!queue.try_push_incremental("event-3"), "slow client queue must be bounded");
    require(queue.snapshot_resync_required(),
            "dropping an incremental event must require snapshot resync");
    require(!queue.try_push_incremental("event-4"),
            "incremental events must remain blocked until snapshot resync");
    queue.replace_with_snapshot("snapshot");
    require(!queue.snapshot_resync_required(), "queued snapshot must satisfy resync");
    require(queue.pop() == "snapshot", "snapshot must replace stale queued events");
}
