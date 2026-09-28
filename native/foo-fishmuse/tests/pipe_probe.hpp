#pragma once

#include <filesystem>
#include <string_view>

namespace fishmuse::test {

int run_auth_probe_server(const std::filesystem::path& ready_file,
                          const std::filesystem::path& stop_file);
int run_auth_probe_client(std::wstring_view pipe_name);

}  // namespace fishmuse::test
