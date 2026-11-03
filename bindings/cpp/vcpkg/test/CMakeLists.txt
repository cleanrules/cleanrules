# A manifest-mode project that takes cleanrules from the overlay port, in C++
# and C. From the repository root:
#
#   cmake -S bindings/cpp/vcpkg/test -B build/vcpkg-test \
#     -DCMAKE_TOOLCHAIN_FILE="$VCPKG_ROOT/scripts/buildsystems/vcpkg.cmake" \
#     -DVCPKG_OVERLAY_PORTS="$PWD/bindings/cpp/vcpkg/ports"
#   cmake --build build/vcpkg-test && ctest --test-dir build/vcpkg-test
cmake_minimum_required(VERSION 3.21)
project(cleanrules_vcpkg_test LANGUAGES C CXX)

find_package(cleanrules CONFIG REQUIRED)

add_executable(vcpkg_test main.cpp)
target_link_libraries(vcpkg_test PRIVATE cleanrules::cleanrules)

add_executable(vcpkg_test_c main.c)
target_link_libraries(vcpkg_test_c PRIVATE cleanrules::c)

enable_testing()
add_test(NAME vcpkg_test COMMAND vcpkg_test)
set_tests_properties(vcpkg_test PROPERTIES PASS_REGULAR_EXPRESSION "cleanrules [0-9.]+: vip")
add_test(NAME vcpkg_test_c COMMAND vcpkg_test_c)
set_tests_properties(vcpkg_test_c PROPERTIES PASS_REGULAR_EXPRESSION "from C: vip fired")
