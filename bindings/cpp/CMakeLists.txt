# cleanrules for C and C++: builds the C ABI (crates/ffi) with cargo and
# provides two targets, also after installing (find_package(cleanrules)):
#
#   cleanrules::c           the C library and cleanrules.h
#   cleanrules::cleanrules  the C++ header cleanrules.hpp (C++17), over it
#
#   cmake -S bindings/cpp -B build/cpp && ctest --test-dir build/cpp

cmake_minimum_required(VERSION 3.21)

get_filename_component(CLEANRULES_ROOT "${CMAKE_CURRENT_SOURCE_DIR}/../.." ABSOLUTE)

# One version for every package: the workspace's (Cargo.toml).
file(STRINGS "${CLEANRULES_ROOT}/Cargo.toml" _cleanrules_version REGEX "^version = \"" LIMIT_COUNT 1)
string(REGEX REPLACE "^version = \"([0-9.]+).*" "\\1" _cleanrules_version "${_cleanrules_version}")

project(cleanrules VERSION ${_cleanrules_version} LANGUAGES C CXX)

include(GNUInstallDirs)
include(CMakePackageConfigHelpers)

option(CLEANRULES_SHARED "Link the shared C library instead of the static one" OFF)
option(CLEANRULES_BUILD_TESTS "Build the tests and the example" ${PROJECT_IS_TOP_LEVEL})
option(CLEANRULES_INSTALL "Install the headers, the library and the CMake package"
       ${PROJECT_IS_TOP_LEVEL})
set(CLEANRULES_SANITIZERS "" CACHE STRING "Sanitizers for the tests, such as address,undefined")
set(CLEANRULES_CARGO_TARGET_DIR "${CMAKE_CURRENT_BINARY_DIR}/cargo" CACHE PATH
    "Where cargo builds the C library")
file(TO_CMAKE_PATH "${CLEANRULES_CARGO_TARGET_DIR}" _cargo_target_dir)
set(CLEANRULES_RUST_TARGET "" CACHE STRING
    "The Rust target triple; empty for the host's, or derived when cross-compiling")

get_property(_cleanrules_multi_config GLOBAL PROPERTY GENERATOR_IS_MULTI_CONFIG)
if(PROJECT_IS_TOP_LEVEL AND NOT _cleanrules_multi_config AND NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE Debug CACHE STRING "The build type" FORCE)
endif()

# --- Rust -------------------------------------------------------------------

# The Rust version the workspace asks for (rust-version in Cargo.toml).
file(STRINGS "${CLEANRULES_ROOT}/Cargo.toml" _rust_version REGEX "^rust-version = \"" LIMIT_COUNT 1)
string(REGEX REPLACE "^rust-version = \"([0-9.]+).*" "\\1" _rust_version "${_rust_version}")

# cargo, and the rustc next to it; CARGO in the environment or the cache wins.
if(DEFINED ENV{CARGO} AND NOT CARGO)
  set(CARGO "$ENV{CARGO}" CACHE FILEPATH "cargo")
endif()
# rustup's default place is a hint too: package managers such as vcpkg build
# with a reduced PATH.
find_program(CARGO cargo HINTS "$ENV{CARGO_HOME}/bin" "$ENV{HOME}/.cargo/bin"
                                "$ENV{USERPROFILE}/.cargo/bin")
if(NOT CARGO)
  message(FATAL_ERROR
    "cleanrules builds its C library with cargo, which was not found: install a Rust "
    "toolchain (${_rust_version} or later) with rustup (https://rustup.rs), or set CARGO to "
    "its path")
endif()
get_filename_component(_cargo_dir "${CARGO}" DIRECTORY)
find_program(CLEANRULES_RUSTC rustc HINTS "${_cargo_dir}" REQUIRED)
execute_process(COMMAND "${CLEANRULES_RUSTC}" -vV OUTPUT_VARIABLE _rustc_info RESULT_VARIABLE _rustc_result)
if(NOT _rustc_result EQUAL 0 OR NOT _rustc_info MATCHES "host: ([^\n]+)")
  message(FATAL_ERROR "${CLEANRULES_RUSTC} -vV did not name the host's Rust target")
endif()
set(_rust_host "${CMAKE_MATCH_1}")

# MSVC's C runtime: the DLL one (/MD), as CMake's default, or the static one
# (/MT) when CMAKE_MSVC_RUNTIME_LIBRARY asks for it. The Rust library is built
# to match (crt-static).
if(NOT DEFINED CLEANRULES_MSVC_STATIC_CRT)
  set(_static_crt OFF)
  if(MSVC AND CMAKE_MSVC_RUNTIME_LIBRARY AND NOT CMAKE_MSVC_RUNTIME_LIBRARY MATCHES "DLL")
    set(_static_crt ON)
  endif()
  set(CLEANRULES_MSVC_STATIC_CRT ${_static_crt} CACHE BOOL
      "Build the Rust library for MSVC's static C runtime (/MT)")
endif()
if(CLEANRULES_MSVC_STATIC_CRT AND NOT MSVC)
  message(FATAL_ERROR "CLEANRULES_MSVC_STATIC_CRT is for MSVC's C runtime only")
endif()

# The Rust target: the host's, unless the C and C++ code is built for another
# platform (cross-compiling, one macOS architecture that is not the host's, an
# Apple device, or MinGW, whose Rust target is not the host's default MSVC
# one).
if(CMAKE_OSX_ARCHITECTURES)
  list(LENGTH CMAKE_OSX_ARCHITECTURES _archs)
  if(_archs GREATER 1)
    message(FATAL_ERROR
      "cleanrules builds one architecture at a time, not ${CMAKE_OSX_ARCHITECTURES}: build each "
      "with its own CMAKE_OSX_ARCHITECTURES and join them with lipo")
  endif()
endif()
if(NOT CLEANRULES_RUST_TARGET)
  set(_processor "${CMAKE_SYSTEM_PROCESSOR}")
  if(APPLE AND CMAKE_OSX_ARCHITECTURES)
    set(_processor "${CMAKE_OSX_ARCHITECTURES}")
  endif()
  string(TOLOWER "${_processor}" _processor)
  if(_processor MATCHES "^(arm64|aarch64)$")
    set(_arch aarch64)
  elseif(_processor MATCHES "^(x86_64|amd64|x64)$")
    set(_arch x86_64)
  elseif(_processor MATCHES "^(i[3-6]86|x86)$")
    set(_arch i686)
  else()
    set(_arch "${_processor}")
  endif()
  if(IOS)
    # A simulator's SDK, or a device's.
    if(CMAKE_OSX_SYSROOT MATCHES "[Ss]imulator")
      if(_arch STREQUAL "aarch64")
        set(_derived "aarch64-apple-ios-sim")
      else()
        set(_derived "${_arch}-apple-ios")
      endif()
    else()
      set(_derived "${_arch}-apple-ios")
    endif()
  elseif(APPLE)
    set(_derived "${_arch}-apple-darwin")
  elseif(WIN32 AND MSVC)
    set(_derived "${_arch}-pc-windows-msvc")
  elseif(WIN32)
    set(_derived "${_arch}-pc-windows-gnu")
  elseif(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    set(_derived "${_arch}-unknown-linux-gnu")
  else()
    set(_derived "")
  endif()
  # A native Linux build keeps the host's target, which may be musl.
  if(CMAKE_CROSSCOMPILING OR APPLE OR WIN32)
    if(NOT _derived)
      message(FATAL_ERROR
        "cleanrules cannot tell the Rust target of ${CMAKE_SYSTEM_NAME} on ${_processor}: "
        "set CLEANRULES_RUST_TARGET, such as aarch64-linux-android")
    endif()
    if(NOT _derived STREQUAL _rust_host)
      set(CLEANRULES_RUST_TARGET "${_derived}")
    endif()
  endif()
endif()
# With the static C runtime the target is always explicit, so that the flag
# below reaches only the library, not the build scripts.
set(_rust_target "${CLEANRULES_RUST_TARGET}")
if(CLEANRULES_MSVC_STATIC_CRT AND NOT _rust_target)
  set(_rust_target "${_rust_host}")
endif()
set(_target_args "")
set(_target_subdir "")
if(_rust_target)
  set(_target_args --target "${_rust_target}")
  set(_target_subdir "/${_rust_target}")
  message(STATUS "cleanrules: Rust target ${_rust_target}")
endif()
set(_crt_args "")
if(CLEANRULES_MSVC_STATIC_CRT)
  set(_crt_args -C target-feature=+crt-static)
  # Another C runtime is another build: never in the same target directory.
  string(APPEND _cargo_target_dir "/crt-static")
  message(STATUS "cleanrules: MSVC's static C runtime (crt-static)")
endif()

# The system libraries a Rust static library needs, as rustc lists them for an
# empty one (the C ABI adds none of its own). Set CLEANRULES_NATIVE_LIBS to
# override them.
if(NOT DEFINED CLEANRULES_NATIVE_LIBS)
  set(_probe "${CMAKE_CURRENT_BINARY_DIR}/native-libs")
  file(MAKE_DIRECTORY "${_probe}")
  file(WRITE "${_probe}/probe.rs" "")
  execute_process(
    COMMAND "${CLEANRULES_RUSTC}" --crate-type staticlib --crate-name probe --color never
            ${_target_args} ${_crt_args} --print native-static-libs --out-dir "${_probe}" "${_probe}/probe.rs"
    WORKING_DIRECTORY "${CLEANRULES_ROOT}"
    ERROR_VARIABLE _probe_out OUTPUT_VARIABLE _probe_stdout
    RESULT_VARIABLE _probe_result)
  if(NOT _probe_result EQUAL 0)
    message(FATAL_ERROR "rustc could not list the native libraries:\n${_probe_out}")
  endif()
  string(REGEX MATCH "native-static-libs: ([^\n]*)" _ "${_probe_out}${_probe_stdout}")
  separate_arguments(_tokens NATIVE_COMMAND "${CMAKE_MATCH_1}")
  set(_libs "")
  set(_framework OFF)
  foreach(_token IN LISTS _tokens)
    if(_framework)
      list(APPEND _libs "-framework ${_token}")
      set(_framework OFF)
    elseif(_token STREQUAL "-framework")
      set(_framework ON)
    elseif(_token MATCHES "^-l(.+)$")
      list(APPEND _libs "${CMAKE_MATCH_1}")
    elseif(_token MATCHES "^(.+)\\.lib$")
      list(APPEND _libs "${CMAKE_MATCH_1}")
    endif()
    # Anything else, such as /defaultlib:msvcrt, is the C runtime: the
    # program's own.
  endforeach()
  if(APPLE)
    # Every program links them already.
    list(REMOVE_ITEM _libs System c)
  endif()
  list(REMOVE_DUPLICATES _libs)
  set(CLEANRULES_NATIVE_LIBS "${_libs}" CACHE STRING "The system libraries of the static library")
endif()

# The library's files: a static library, or a shared one, which on Windows is a
# DLL linked through an import library (MSVC's or MinGW's).
set(_cargo_out "${_cargo_target_dir}${_target_subdir}/$<IF:$<CONFIG:Debug>,debug,release>")
if(NOT CLEANRULES_SHARED)
  set(_lib_name "${CMAKE_STATIC_LIBRARY_PREFIX}cleanrules_ffi${CMAKE_STATIC_LIBRARY_SUFFIX}")
  set(_link_name "${_lib_name}")
  set(CLEANRULES_LIBRARY_KIND STATIC)
elseif(WIN32)
  set(_lib_name "cleanrules_ffi.dll")
  if(MSVC)
    set(_link_name "cleanrules_ffi.dll.lib")
  else()
    set(_link_name "libcleanrules_ffi.dll.a")
  endif()
  set(CLEANRULES_LIBRARY_KIND SHARED)
else()
  set(_lib_name "${CMAKE_SHARED_LIBRARY_PREFIX}cleanrules_ffi${CMAKE_SHARED_LIBRARY_SUFFIX}")
  set(_link_name "${_lib_name}")
  set(CLEANRULES_LIBRARY_KIND SHARED)
endif()
set(_lib_file "${_cargo_out}/${_lib_name}")
set(_link_file "${_cargo_out}/${_link_name}")
# The name the dynamic loader looks for (crates/ffi/build.rs sets it).
set(CLEANRULES_SONAME "")
if(CLEANRULES_SHARED AND APPLE)
  set(CLEANRULES_SONAME "@rpath/${_lib_name}")
elseif(CLEANRULES_SHARED AND NOT WIN32)
  set(CLEANRULES_SONAME "${_lib_name}")
endif()

# cargo builds the library on every build (a no-op when nothing changed), with
# the dev profile for Debug and release for the others, for the deployment
# target CMake builds for on macOS.
set(_cargo_env "CARGO_TARGET_DIR=${_cargo_target_dir}")
if(APPLE AND CMAKE_OSX_DEPLOYMENT_TARGET)
  list(APPEND _cargo_env "MACOSX_DEPLOYMENT_TARGET=${CMAKE_OSX_DEPLOYMENT_TARGET}")
endif()
# With an explicit --target, RUSTFLAGS reaches only the target's artifacts:
# build scripts and procedural macros keep the host's defaults.
if(CLEANRULES_MSVC_STATIC_CRT)
  string(STRIP "$ENV{RUSTFLAGS} -Ctarget-feature=+crt-static" _rustflags)
  list(APPEND _cargo_env "RUSTFLAGS=${_rustflags}")
endif()
# For another platform, cargo links with the C compiler CMake uses for it
# (Apple's and MSVC's linkers find their target on their own).
if(_rust_target AND NOT APPLE AND NOT MSVC)
  string(TOUPPER "${_rust_target}" _triple_var)
  string(REPLACE "-" "_" _triple_var "${_triple_var}")
  string(REPLACE "." "_" _triple_var "${_triple_var}")
  list(APPEND _cargo_env "CARGO_TARGET_${_triple_var}_LINKER=${CMAKE_C_COMPILER}")
endif()
add_custom_target(cleanrules_cargo ALL
  COMMAND "${CMAKE_COMMAND}" -E env ${_cargo_env}
          "${CARGO}" build --locked -p cleanrules-ffi ${_target_args}
          "$<$<NOT:$<CONFIG:Debug>>:--release>"
  WORKING_DIRECTORY "${CLEANRULES_ROOT}"
  BYPRODUCTS "${_lib_file}"
  COMMENT "Building the cleanrules C library with cargo"
  USES_TERMINAL VERBATIM COMMAND_EXPAND_LISTS)

# --- Targets ----------------------------------------------------------------

add_library(cleanrules_c INTERFACE)
add_library(cleanrules::c ALIAS cleanrules_c)
add_dependencies(cleanrules_c cleanrules_cargo)
target_include_directories(cleanrules_c INTERFACE
  "$<BUILD_INTERFACE:${CLEANRULES_ROOT}/crates/ffi/include>")
target_link_libraries(cleanrules_c INTERFACE "$<BUILD_INTERFACE:${_link_file}>")
if(NOT CLEANRULES_SHARED)
  target_link_libraries(cleanrules_c INTERFACE ${CLEANRULES_NATIVE_LIBS})
elseif(NOT WIN32)
  # In the build tree, programs find the shared library where cargo left it:
  # its install name or SONAME is relative, and CMake derives no rpath from a
  # path that depends on the configuration.
  target_link_options(cleanrules_c INTERFACE "$<BUILD_INTERFACE:LINKER:-rpath,${_cargo_out}>")
endif()

add_library(cleanrules_cpp INTERFACE)
add_library(cleanrules::cleanrules ALIAS cleanrules_cpp)
target_include_directories(cleanrules_cpp INTERFACE
  "$<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>")
target_compile_features(cleanrules_cpp INTERFACE cxx_std_17)
target_link_libraries(cleanrules_cpp INTERFACE cleanrules_c)

# --- Install ----------------------------------------------------------------

if(CLEANRULES_INSTALL)
  install(FILES "${CLEANRULES_ROOT}/crates/ffi/include/cleanrules.h"
                "${CMAKE_CURRENT_SOURCE_DIR}/include/cleanrules.hpp"
          DESTINATION "${CMAKE_INSTALL_INCLUDEDIR}")
  if(CLEANRULES_SHARED AND WIN32)
    install(FILES "${_lib_file}" DESTINATION "${CMAKE_INSTALL_BINDIR}")
    install(FILES "${_link_file}" DESTINATION "${CMAKE_INSTALL_LIBDIR}")
  else()
    install(FILES "${_lib_file}" DESTINATION "${CMAKE_INSTALL_LIBDIR}")
  endif()
  foreach(_license IN ITEMS LICENSE NOTICE)
    install(FILES "${CLEANRULES_ROOT}/${_license}"
            DESTINATION "${CMAKE_INSTALL_DATADIR}/cleanrules")
  endforeach()

  set(CLEANRULES_LIB_NAME "${_lib_name}")
  set(CLEANRULES_LINK_NAME "${_link_name}")
  if(CLEANRULES_SHARED)
    set(CLEANRULES_INSTALLED_NATIVE_LIBS "")
  else()
    set(CLEANRULES_INSTALLED_NATIVE_LIBS "${CLEANRULES_NATIVE_LIBS}")
  endif()
  configure_package_config_file(
    "${CMAKE_CURRENT_SOURCE_DIR}/cmake/cleanrulesConfig.cmake.in"
    "${CMAKE_CURRENT_BINARY_DIR}/cleanrulesConfig.cmake"
    INSTALL_DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/cleanrules"
    PATH_VARS CMAKE_INSTALL_INCLUDEDIR CMAKE_INSTALL_LIBDIR CMAKE_INSTALL_BINDIR)
  write_basic_package_version_file(
    "${CMAKE_CURRENT_BINARY_DIR}/cleanrulesConfigVersion.cmake"
    COMPATIBILITY SameMinorVersion)
  install(FILES "${CMAKE_CURRENT_BINARY_DIR}/cleanrulesConfig.cmake"
                "${CMAKE_CURRENT_BINARY_DIR}/cleanrulesConfigVersion.cmake"
          DESTINATION "${CMAKE_INSTALL_LIBDIR}/cmake/cleanrules")
endif()

# --- Tests ------------------------------------------------------------------

if(CLEANRULES_BUILD_TESTS)
  enable_testing()

  # The Rust objects use the release C runtime: so do the tests.
  if(CLEANRULES_MSVC_STATIC_CRT)
    set(CMAKE_MSVC_RUNTIME_LIBRARY MultiThreaded)
  else()
    set(CMAKE_MSVC_RUNTIME_LIBRARY MultiThreadedDLL)
  endif()

  if(MSVC)
    set(_warnings /W4 /WX /permissive- /utf-8)
  else()
    set(_warnings -Wall -Wextra -Werror -pedantic -Wconversion -Wsign-conversion -Wshadow
                  -Wold-style-cast)
  endif()
  set(_sanitize "")
  if(CLEANRULES_SANITIZERS)
    set(_sanitize "-fsanitize=${CLEANRULES_SANITIZERS}" -fno-omit-frame-pointer)
  endif()

  function(cleanrules_test name source)
    add_executable(${name} "${source}")
    target_link_libraries(${name} PRIVATE cleanrules::cleanrules)
    target_compile_options(${name} PRIVATE ${_warnings} ${_sanitize})
    target_link_options(${name} PRIVATE ${_sanitize})
    if(NOT MSVC)
      find_package(Threads REQUIRED)
      target_link_libraries(${name} PRIVATE Threads::Threads)
    endif()
    # Windows finds a DLL next to the program.
    if(CLEANRULES_SHARED AND WIN32)
      add_custom_command(TARGET ${name} POST_BUILD
        COMMAND "${CMAKE_COMMAND}" -E copy_if_different "${_lib_file}" "$<TARGET_FILE_DIR:${name}>")
    endif()
  endfunction()

  # The exact command `cmake -S ... -B ... && ctest --test-dir ...` builds
  # first: every test needs this one.
  add_test(NAME build
           COMMAND "${CMAKE_COMMAND}" --build "${CMAKE_BINARY_DIR}" --config "$<CONFIG>")
  set_tests_properties(build PROPERTIES FIXTURES_SETUP built)

  cleanrules_test(cleanrules_api tests/api.cpp)
  add_test(NAME api COMMAND cleanrules_api)

  # The header without exceptions: only the error_info overloads exist.
  cleanrules_test(cleanrules_no_exceptions tests/no_exceptions.cpp)
  if(MSVC)
    target_compile_options(cleanrules_no_exceptions PRIVATE /EHs-c- /D_HAS_EXCEPTIONS=0)
  else()
    target_compile_options(cleanrules_no_exceptions PRIVATE -fno-exceptions)
  endif()
  add_test(NAME no_exceptions COMMAND cleanrules_no_exceptions)

  # Both kinds of translation unit in one program.
  cleanrules_test(cleanrules_mixed tests/mixed_main.cpp)
  target_sources(cleanrules_mixed PRIVATE tests/mixed_noexcept.cpp)
  if(MSVC)
    set_source_files_properties(tests/mixed_noexcept.cpp PROPERTIES COMPILE_OPTIONS "/EHs-c-")
  else()
    set_source_files_properties(tests/mixed_noexcept.cpp PROPERTIES COMPILE_OPTIONS "-fno-exceptions")
  endif()
  add_test(NAME mixed COMMAND cleanrules_mixed)

  cleanrules_test(discounts "${CLEANRULES_ROOT}/docs/examples/discounts.cpp")
  add_test(NAME example COMMAND discounts "${CLEANRULES_ROOT}/docs/examples/discounts.rules")
  set_tests_properties(example PROPERTIES PASS_REGULAR_EXPRESSION "notify_sales: rule vip")

  set_tests_properties(api no_exceptions mixed example PROPERTIES FIXTURES_REQUIRED built)
  if(CLEANRULES_SANITIZERS)
    # Any report fails the test.
    set_tests_properties(api no_exceptions mixed example PROPERTIES
      ENVIRONMENT "UBSAN_OPTIONS=halt_on_error=1:print_stacktrace=1")
  endif()

  if(CLEANRULES_INSTALL)
    # Install into the build tree, then build a project that finds the package.
    set(_prefix "${CMAKE_BINARY_DIR}/installed")
    # The consumer builds for the same platform.
    set(_consumer_options "-DCMAKE_PREFIX_PATH=${_prefix}" "-DCMAKE_BUILD_TYPE=$<CONFIG>"
                          "-DCMAKE_MSVC_RUNTIME_LIBRARY=${CMAKE_MSVC_RUNTIME_LIBRARY}")
    foreach(_var IN ITEMS CMAKE_TOOLCHAIN_FILE CMAKE_OSX_ARCHITECTURES CMAKE_OSX_DEPLOYMENT_TARGET)
      if(${_var})
        list(APPEND _consumer_options "-D${_var}=${${_var}}")
      endif()
    endforeach()
    add_test(NAME install
             COMMAND "${CMAKE_COMMAND}" --install "${CMAKE_BINARY_DIR}" --config "$<CONFIG>"
                     --prefix "${_prefix}")
    set_tests_properties(install PROPERTIES FIXTURES_REQUIRED built FIXTURES_SETUP installed)
    add_test(NAME find_package
             COMMAND "${CMAKE_CTEST_COMMAND}" --build-and-test
                     "${CMAKE_CURRENT_SOURCE_DIR}/tests/consumer" "${CMAKE_BINARY_DIR}/consumer"
                     --build-generator "${CMAKE_GENERATOR}"
                     --build-config "$<CONFIG>"
                     --build-options ${_consumer_options}
                     --test-command consumer)
    set_tests_properties(find_package PROPERTIES FIXTURES_REQUIRED installed
                         PASS_REGULAR_EXPRESSION "consumer: vip")

    # The installed shared library names itself as the loader finds it once
    # installed (an @rpath install name, a SONAME), not by the path it was
    # built at, which only works on the build machine.
    if(CLEANRULES_SHARED AND APPLE)
      find_program(CLEANRULES_OTOOL otool REQUIRED)
      add_test(NAME library_name
               COMMAND "${CLEANRULES_OTOOL}" -D "${_prefix}/${CMAKE_INSTALL_LIBDIR}/${_lib_name}")
      set_tests_properties(library_name PROPERTIES FIXTURES_REQUIRED installed
                           PASS_REGULAR_EXPRESSION "\n@rpath/${_lib_name}")
    elseif(CLEANRULES_SHARED AND NOT WIN32)
      find_program(CLEANRULES_READELF readelf REQUIRED)
      add_test(NAME library_name
               COMMAND "${CLEANRULES_READELF}" -d "${_prefix}/${CMAKE_INSTALL_LIBDIR}/${_lib_name}")
      set_tests_properties(library_name PROPERTIES FIXTURES_REQUIRED installed
                           PASS_REGULAR_EXPRESSION "SONAME[^\n]*\\[${_lib_name}\\]")
    endif()
  endif()
endif()
