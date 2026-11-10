cmake_minimum_required(VERSION 3.20)
project(housefire CXX)
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_EXPORT_COMPILE_COMMANDS ON)
if(NOT CMAKE_BUILD_TYPE)
  set(CMAKE_BUILD_TYPE RelWithDebInfo)
endif()

add_library(housefire src/iff.cpp src/bhav.cpp src/interp.cpp src/disc.cpp src/expression.cpp src/library.cpp src/world.cpp src/save.cpp src/primitives.cpp src/simulator.cpp src/motives.cpp src/autonomy.cpp)
target_include_directories(housefire PUBLIC include)
target_compile_options(housefire PRIVATE -Wall -Wextra)

enable_testing()
foreach(t test_interp test_disc test_expression test_lots test_primitives test_simulator)
  add_executable(${t} tests/${t}.cpp)
  target_link_libraries(${t} housefire)
  add_test(NAME ${t} COMMAND ${t})
endforeach()

# The native interpreter in lockstep with a log of the recompiled Bustin' Out's.
add_executable(lockstep tools/lockstep.cpp)
target_link_libraries(lockstep housefire)

# The club run natively over the log's ticks.
add_executable(replay tools/replay.cpp)
target_link_libraries(replay housefire)
