# A project that uses an installed cleanrules, as a user's would.
cmake_minimum_required(VERSION 3.21)
project(consumer LANGUAGES CXX)

find_package(cleanrules CONFIG REQUIRED)

add_executable(consumer main.cpp)
target_link_libraries(consumer PRIVATE cleanrules::cleanrules)

# Windows finds a DLL next to the program.
get_target_property(_type cleanrules::c TYPE)
if(WIN32 AND _type STREQUAL "SHARED_LIBRARY")
  add_custom_command(TARGET consumer POST_BUILD
    COMMAND "${CMAKE_COMMAND}" -E copy_if_different "$<TARGET_FILE:cleanrules::c>"
            "$<TARGET_FILE_DIR:consumer>")
endif()
