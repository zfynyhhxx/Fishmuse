cmake_minimum_required(VERSION 3.25)

project(foo_fishmuse LANGUAGES CXX)

find_package(nlohmann_json CONFIG REQUIRED)

add_executable(
    foo_fishmuse_tests
    src/framing.cpp
    src/component_service.cpp
    src/pipe_security.cpp
    src/pipe_server.cpp
    src/playback_facade.cpp
    src/protocol.cpp
    src/sdk_adapter_support.cpp
    tests/protocol_tests.cpp
    tests/pipe_security_tests.cpp
    tests/idempotency_tests.cpp
    tests/playback_facade_tests.cpp
    tests/sdk_adapter_support_tests.cpp
    tests/component_service_tests.cpp
    tests/pipe_probe.cpp
)

target_compile_features(foo_fishmuse_tests PRIVATE cxx_std_20)
target_compile_options(foo_fishmuse_tests PRIVATE /W4 /WX /permissive- /utf-8)
target_include_directories(foo_fishmuse_tests PRIVATE src tests)
set_property(TARGET foo_fishmuse_tests PROPERTY VS_GLOBAL_TrackFileAccess "false")
target_compile_definitions(
    foo_fishmuse_tests
    PRIVATE FISHMUSE_PROTOCOL_DIR="${CMAKE_CURRENT_SOURCE_DIR}/../../protocol/foobar-v1"
)
target_link_libraries(
    foo_fishmuse_tests
    PRIVATE nlohmann_json::nlohmann_json advapi32 bcrypt ole32
)

enable_testing()
add_test(NAME foo_fishmuse_tests COMMAND foo_fishmuse_tests)
